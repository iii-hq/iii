package iii

import (
	"context"
	"encoding/json"
	"errors"
	"testing"
	"time"

	"github.com/coder/websocket"
	"github.com/google/uuid"
)

// connectClient starts the client against the mock engine and waits for the connection,
// registering cleanup. Tests get a connected client and the mock to assert on.
func connectClient(t *testing.T, m *mockEngine, opts ...Option) *Client {
	t.Helper()
	c := New(m.url, opts...)
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	if err := c.Connect(ctx); err != nil {
		t.Fatalf("Connect: %v", err)
	}
	t.Cleanup(func() { _ = c.Close() })
	return c
}

// TestConnectRegistersWorkerMetadata verifies the connect handshake ends with a
// fire-and-forget engine::workers::register tagged runtime "go".
func TestConnectRegistersWorkerMetadata(t *testing.T) {
	m := newMockEngine(t)
	connectClient(t, m)

	got := m.waitFor(func(msgs []map[string]json.RawMessage) bool {
		for _, msg := range msgs {
			if stringField(msg, "function_id") == FnRegisterWorker {
				return true
			}
		}
		return false
	}, 2*time.Second)

	reg := firstWhere(got, func(msg map[string]json.RawMessage) bool {
		return stringField(msg, "function_id") == FnRegisterWorker
	})
	if reg == nil {
		t.Fatal("no engine::workers::register frame sent on connect")
	}
	if messageType(reg) != string(MsgInvokeFunction) {
		t.Errorf("worker register type = %q, want invokefunction", messageType(reg))
	}
	// Fire-and-forget: void action, no invocation_id.
	if _, hasID := reg["invocation_id"]; hasID {
		t.Error("worker register must be fire-and-forget (no invocation_id)")
	}
	var meta workerMetadata
	if err := json.Unmarshal(reg["data"], &meta); err != nil {
		t.Fatalf("decode worker metadata: %v", err)
	}
	if meta.Runtime != "go" {
		t.Errorf("runtime = %q, want go", meta.Runtime)
	}
}

// TestRegistrationsSentInOrderOnConnect verifies the connect order: trigger types,
// functions, triggers, then worker metadata last.
func TestRegistrationsSentInOrderOnConnect(t *testing.T) {
	m := newMockEngine(t)
	c := New(m.url)
	// Register before connecting so all replay on connect.
	_ = c.RegisterFunction("hello::greet", func(ctx context.Context, _ json.RawMessage) (any, error) {
		return map[string]string{"msg": "hi"}, nil
	})
	_ = c.RegisterTrigger("t1", "http", "hello::greet", json.RawMessage(`{"path":"/x"}`), nil)

	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	if err := c.Connect(ctx); err != nil {
		t.Fatalf("Connect: %v", err)
	}
	t.Cleanup(func() { _ = c.Close() })

	got := m.waitFor(func(msgs []map[string]json.RawMessage) bool {
		return countType(msgs, string(MsgInvokeFunction)) >= 1 && // worker metadata
			countType(msgs, string(MsgRegisterFunction)) >= 1 &&
			countType(msgs, string(MsgRegisterTrigger)) >= 1
	}, 2*time.Second)

	idxFn := indexOfType(got, string(MsgRegisterFunction))
	idxTrig := indexOfType(got, string(MsgRegisterTrigger))
	idxMeta := indexOfFunctionID(got, FnRegisterWorker)

	if idxFn < 0 || idxTrig < 0 || idxMeta < 0 {
		t.Fatalf("missing frames: fn=%d trig=%d meta=%d", idxFn, idxTrig, idxMeta)
	}
	// Function registration precedes trigger registration; worker metadata is last.
	if !(idxFn < idxTrig) {
		t.Errorf("registerfunction (%d) should precede registertrigger (%d)", idxFn, idxTrig)
	}
	if !(idxMeta > idxFn && idxMeta > idxTrig) {
		t.Errorf("worker metadata (%d) should come after registrations (fn=%d trig=%d)", idxMeta, idxFn, idxTrig)
	}
}

// TestInboundInvokeRoundtrip exercises the core path: the engine invokes a registered
// function and the worker replies with an InvocationResult carrying the handler's
// output, echoing the trace context.
func TestInboundInvokeRoundtrip(t *testing.T) {
	m := newMockEngine(t)

	// The engine replies to the worker-metadata register (ignore it), then, once the
	// function is registered, sends an invokefunction and records the worker's reply.
	tp := "00-trace-span-01"
	m.onReceive = func(conn *websocket.Conn, msg map[string]json.RawMessage) {
		if messageType(msg) == string(MsgRegisterFunction) && messageID(msg) == "echo::fn" {
			id := mustUUID(t, "22222222-2222-2222-2222-222222222222")
			ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
			defer cancel()
			_ = m.send(ctx, conn, &InvokeFunctionMessage{
				InvocationID: &id,
				FunctionID:   "echo::fn",
				Data:         json.RawMessage(`{"in":42}`),
				Traceparent:  &tp,
			})
		}
	}

	c := connectClient(t, m)
	_ = c.RegisterFunction("echo::fn", func(ctx context.Context, data json.RawMessage) (any, error) {
		return json.RawMessage(data), nil // echo the input back
	})

	got := m.waitFor(func(msgs []map[string]json.RawMessage) bool {
		return firstOfType(msgs, string(MsgInvocationResult)) != nil
	}, 3*time.Second)

	res := firstOfType(got, string(MsgInvocationResult))
	if res == nil {
		t.Fatal("worker sent no invocationresult")
	}
	if stringField(res, "invocation_id") != "22222222-2222-2222-2222-222222222222" {
		t.Errorf("invocation_id = %q, want the engine's id", stringField(res, "invocation_id"))
	}
	if got := string(res["result"]); got != `{"in":42}` {
		t.Errorf("result = %s, want {\"in\":42}", got)
	}
	if stringField(res, "traceparent") != tp {
		t.Errorf("traceparent not echoed: got %q, want %q", stringField(res, "traceparent"), tp)
	}
}

// TestInboundInvokeHandlerError maps a handler error to an InvocationResult.error with
// code "invocation_failed".
func TestInboundInvokeHandlerError(t *testing.T) {
	m := newMockEngine(t)
	m.onReceive = func(conn *websocket.Conn, msg map[string]json.RawMessage) {
		if messageType(msg) == string(MsgRegisterFunction) && messageID(msg) == "boom::fn" {
			id := mustUUID(t, "33333333-3333-3333-3333-333333333333")
			ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
			defer cancel()
			_ = m.send(ctx, conn, &InvokeFunctionMessage{InvocationID: &id, FunctionID: "boom::fn", Data: json.RawMessage(`{}`)})
		}
	}

	c := connectClient(t, m)
	_ = c.RegisterFunction("boom::fn", func(ctx context.Context, _ json.RawMessage) (any, error) {
		return nil, errors.New("kaboom")
	})

	got := m.waitFor(func(msgs []map[string]json.RawMessage) bool {
		return firstOfType(msgs, string(MsgInvocationResult)) != nil
	}, 3*time.Second)
	res := firstOfType(got, string(MsgInvocationResult))
	if res == nil {
		t.Fatal("no invocationresult on handler error")
	}
	var body ErrorBody
	if err := json.Unmarshal(res["error"], &body); err != nil {
		t.Fatalf("decode error body: %v", err)
	}
	if body.Code != "invocation_failed" {
		t.Errorf("code = %q, want invocation_failed", body.Code)
	}
	if body.Message != "kaboom" {
		t.Errorf("message = %q, want kaboom", body.Message)
	}
}

// TestInboundInvokeUnknownFunction replies with function_not_found when the engine
// invokes a function this worker never registered.
func TestInboundInvokeUnknownFunction(t *testing.T) {
	m := newMockEngine(t)
	c := connectClient(t, m)
	_ = c // keep alive
	// Send an invoke for an unregistered function after connect.
	go func() {
		m.waitFor(func(msgs []map[string]json.RawMessage) bool {
			return firstOfType(msgs, string(MsgInvokeFunction)) != nil // worker metadata sent => connected
		}, 2*time.Second)
		m.mu.Lock()
		conn := m.active
		m.mu.Unlock()
		if conn != nil {
			id := mustUUID(t, "44444444-4444-4444-4444-444444444444")
			ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
			defer cancel()
			_ = m.send(ctx, conn, &InvokeFunctionMessage{InvocationID: &id, FunctionID: "ghost::fn", Data: json.RawMessage(`{}`)})
		}
	}()

	got := m.waitFor(func(msgs []map[string]json.RawMessage) bool {
		return firstOfType(msgs, string(MsgInvocationResult)) != nil
	}, 3*time.Second)
	res := firstOfType(got, string(MsgInvocationResult))
	if res == nil {
		t.Fatal("no invocationresult for unknown function")
	}
	var body ErrorBody
	_ = json.Unmarshal(res["error"], &body)
	if body.Code != "function_not_found" {
		t.Errorf("code = %q, want function_not_found", body.Code)
	}
}

// TestTriggerAwaitResult covers the default outbound path: the worker triggers a
// function, the engine replies with an invocationresult keyed by the invocation_id, and
// Trigger returns the result.
func TestTriggerAwaitResult(t *testing.T) {
	m := newMockEngine(t)
	// Echo back an invocationresult for any invokefunction carrying an invocation_id.
	m.onReceive = func(conn *websocket.Conn, msg map[string]json.RawMessage) {
		if messageType(msg) != string(MsgInvokeFunction) {
			return
		}
		idRaw, ok := msg["invocation_id"]
		if !ok {
			return // fire-and-forget (e.g. worker metadata)
		}
		var uid uuid.UUID
		if err := json.Unmarshal(idRaw, &uid); err != nil {
			return
		}
		ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
		defer cancel()
		_ = m.send(ctx, conn, &InvocationResultMessage{
			InvocationID: uid,
			FunctionID:   stringField(msg, "function_id"),
			Result:       json.RawMessage(`{"pong":true}`),
		})
	}

	c := connectClient(t, m)
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	res, err := c.Trigger(ctx, TriggerRequest{FunctionID: "svc::ping", Data: json.RawMessage(`{}`)})
	if err != nil {
		t.Fatalf("Trigger: %v", err)
	}
	if string(res) != `{"pong":true}` {
		t.Errorf("result = %s, want {\"pong\":true}", res)
	}
}

// TestTriggerAwaitRemoteError maps an engine error result to *InvocationError.
func TestTriggerAwaitRemoteError(t *testing.T) {
	m := newMockEngine(t)
	m.onReceive = func(conn *websocket.Conn, msg map[string]json.RawMessage) {
		if messageType(msg) != string(MsgInvokeFunction) {
			return
		}
		idRaw, ok := msg["invocation_id"]
		if !ok {
			return
		}
		var uid uuid.UUID
		if json.Unmarshal(idRaw, &uid) != nil {
			return
		}
		ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
		defer cancel()
		_ = m.send(ctx, conn, &InvocationResultMessage{
			InvocationID: uid,
			FunctionID:   stringField(msg, "function_id"),
			Error:        &ErrorBody{Code: "FORBIDDEN", Message: "no access"},
		})
	}

	c := connectClient(t, m)
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	_, err := c.Trigger(ctx, TriggerRequest{FunctionID: "svc::secret"})
	if err == nil {
		t.Fatal("expected an error from a remote-error result")
	}
	var ie *InvocationError
	if !errors.As(err, &ie) {
		t.Fatalf("error is not *InvocationError: %v", err)
	}
	if ie.Code != "FORBIDDEN" {
		t.Errorf("code = %q, want FORBIDDEN", ie.Code)
	}
}

// TestTriggerVoidFireAndForget sends a void invocation with no invocation_id and
// returns immediately without waiting for a reply.
func TestTriggerVoidFireAndForget(t *testing.T) {
	m := newMockEngine(t)
	c := connectClient(t, m)

	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	res, err := c.Trigger(ctx, TriggerRequest{
		FunctionID: LogInfo,
		Data:       json.RawMessage(`{"msg":"hello"}`),
		Action:     VoidAction(),
	})
	if err != nil {
		t.Fatalf("void Trigger: %v", err)
	}
	if res != nil {
		t.Errorf("void Trigger result = %s, want nil", res)
	}

	got := m.waitFor(func(msgs []map[string]json.RawMessage) bool {
		return firstWhere(msgs, func(m map[string]json.RawMessage) bool {
			return stringField(m, "function_id") == LogInfo
		}) != nil
	}, 2*time.Second)
	frame := firstWhere(got, func(m map[string]json.RawMessage) bool {
		return stringField(m, "function_id") == LogInfo
	})
	if frame == nil {
		t.Fatal("void invocation not sent")
	}
	if _, hasID := frame["invocation_id"]; hasID {
		t.Error("void invocation must omit invocation_id")
	}
	if string(frame["action"]) != `{"type":"void"}` {
		t.Errorf("action = %s, want void", frame["action"])
	}
}

// TestTriggerEnqueueAwaitsReceipt covers the enqueue path: the worker triggers through
// a named queue (action:enqueue) and awaits the engine's receipt, which arrives as an
// ordinary invocationresult keyed by the invocation_id.
func TestTriggerEnqueueAwaitsReceipt(t *testing.T) {
	m := newMockEngine(t)
	m.onReceive = func(conn *websocket.Conn, msg map[string]json.RawMessage) {
		if messageType(msg) != string(MsgInvokeFunction) {
			return
		}
		idRaw, ok := msg["invocation_id"]
		if !ok {
			return // worker metadata (void) — ignore
		}
		// The enqueue invocation must carry the queue action on the wire.
		if string(msg["action"]) != `{"type":"enqueue","queue":"jobs"}` {
			t.Errorf("enqueue action = %s, want enqueue/jobs", msg["action"])
		}
		var uid uuid.UUID
		if json.Unmarshal(idRaw, &uid) != nil {
			return
		}
		ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
		defer cancel()
		_ = m.send(ctx, conn, &InvocationResultMessage{
			InvocationID: uid,
			FunctionID:   stringField(msg, "function_id"),
			Result:       json.RawMessage(`{"messageReceiptId":"r-1"}`),
		})
	}

	c := connectClient(t, m)
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	res, err := c.Trigger(ctx, TriggerRequest{
		FunctionID: "svc::work",
		Action:     EnqueueAction("jobs"),
	})
	if err != nil {
		t.Fatalf("enqueue Trigger: %v", err)
	}
	if string(res) != `{"messageReceiptId":"r-1"}` {
		t.Errorf("receipt = %s, want the enqueue receipt", res)
	}
}

// TestTriggerTimeout returns ErrTimeout when no result arrives in time.
func TestTriggerTimeout(t *testing.T) {
	m := newMockEngine(t)
	c := connectClient(t, m) // engine never replies to invocations

	ctx := context.Background()
	_, err := c.Trigger(ctx, TriggerRequest{FunctionID: "svc::silent", Timeout: 150 * time.Millisecond})
	if !errors.Is(err, ErrTimeout) {
		t.Fatalf("error = %v, want ErrTimeout", err)
	}
}

// TestReconnectResendsRegistrations forces the connection to drop and verifies the
// client reconnects and re-sends the function registration on the new connection.
func TestReconnectResendsRegistrations(t *testing.T) {
	m := newMockEngine(t)
	c := New(m.url, WithReconnectConfig(ReconnectConfig{
		InitialDelay:      10 * time.Millisecond,
		MaxDelay:          50 * time.Millisecond,
		BackoffMultiplier: 2,
		JitterFactor:      0,
		MaxRetries:        -1,
	}))
	_ = c.RegisterFunction("persist::fn", func(ctx context.Context, _ json.RawMessage) (any, error) {
		return nil, nil
	})
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	if err := c.Connect(ctx); err != nil {
		t.Fatalf("Connect: %v", err)
	}
	t.Cleanup(func() { _ = c.Close() })

	// Wait for the first registration, then drop the connection and clear the buffer.
	m.waitFor(func(msgs []map[string]json.RawMessage) bool {
		return countRegister(msgs, string(MsgRegisterFunction), "persist::fn") >= 1
	}, 2*time.Second)
	m.clear()
	m.closeActiveConnection()

	// After reconnect, the function must be registered again on the fresh connection.
	got := m.waitFor(func(msgs []map[string]json.RawMessage) bool {
		return countRegister(msgs, string(MsgRegisterFunction), "persist::fn") >= 1
	}, 3*time.Second)
	if countRegister(got, string(MsgRegisterFunction), "persist::fn") < 1 {
		t.Error("registration not re-sent after reconnect")
	}
}

// TestPingPong verifies the client replies to an engine ping with a pong at the
// application-protocol level (matching the Rust SDK).
func TestPingPong(t *testing.T) {
	m := newMockEngine(t)
	c := connectClient(t, m)
	_ = c

	// Send a ping once connected.
	m.waitFor(func(msgs []map[string]json.RawMessage) bool {
		return firstOfType(msgs, string(MsgInvokeFunction)) != nil
	}, 2*time.Second)
	m.mu.Lock()
	conn := m.active
	m.mu.Unlock()
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	_ = m.send(ctx, conn, &PingMessage{})

	got := m.waitFor(func(msgs []map[string]json.RawMessage) bool {
		return firstOfType(msgs, string(MsgPong)) != nil
	}, 2*time.Second)
	if firstOfType(got, string(MsgPong)) == nil {
		t.Error("client did not reply to ping with pong")
	}
}

// TestCloseCancelsPending verifies Close unblocks an in-flight Trigger with
// ErrNotConnected.
func TestCloseCancelsPending(t *testing.T) {
	m := newMockEngine(t)
	c := connectClient(t, m) // engine never replies

	errCh := make(chan error, 1)
	go func() {
		_, err := c.Trigger(context.Background(), TriggerRequest{FunctionID: "svc::hang", Timeout: 5 * time.Second})
		errCh <- err
	}()

	// Give the trigger a moment to register its pending entry, then close.
	m.waitFor(func(msgs []map[string]json.RawMessage) bool {
		return firstWhere(msgs, func(mm map[string]json.RawMessage) bool {
			return stringField(mm, "function_id") == "svc::hang"
		}) != nil
	}, 2*time.Second)
	_ = c.Close()

	select {
	case err := <-errCh:
		if !errors.Is(err, ErrNotConnected) {
			t.Errorf("Trigger after Close = %v, want ErrNotConnected", err)
		}
	case <-time.After(2 * time.Second):
		t.Error("Trigger did not unblock after Close")
	}
}

// stubTriggerHandler records register/unregister calls for assertions.
type stubTriggerHandler struct {
	registered chan TriggerConfig
	failWith   error
}

func (s *stubTriggerHandler) RegisterTrigger(ctx context.Context, cfg TriggerConfig) error {
	if s.registered != nil {
		s.registered <- cfg
	}
	return s.failWith
}
func (s *stubTriggerHandler) UnregisterTrigger(ctx context.Context, cfg TriggerConfig) error {
	return nil
}

// TestInboundRegisterTrigger routes an engine RegisterTrigger to the trigger-type
// handler and replies with a successful TriggerRegistrationResult.
func TestInboundRegisterTrigger(t *testing.T) {
	m := newMockEngine(t)
	handler := &stubTriggerHandler{registered: make(chan TriggerConfig, 1)}

	m.onReceive = func(conn *websocket.Conn, msg map[string]json.RawMessage) {
		if messageType(msg) == string(MsgRegisterTriggerType) && messageID(msg) == "cron" {
			ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
			defer cancel()
			_ = m.send(ctx, conn, &RegisterTriggerMessage{
				ID:          "inst-1",
				TriggerType: "cron",
				FunctionID:  "job::run",
				Config:      json.RawMessage(`{"every":"5m"}`),
			})
		}
	}

	c := connectClient(t, m)
	if err := c.RegisterTriggerType("cron", "periodic", handler); err != nil {
		t.Fatalf("RegisterTriggerType: %v", err)
	}

	select {
	case cfg := <-handler.registered:
		if cfg.ID != "inst-1" || cfg.FunctionID != "job::run" {
			t.Errorf("handler got cfg %+v", cfg)
		}
	case <-time.After(3 * time.Second):
		t.Fatal("trigger handler was not called")
	}

	got := m.waitFor(func(msgs []map[string]json.RawMessage) bool {
		return firstOfType(msgs, string(MsgTriggerRegistrationResult)) != nil
	}, 2*time.Second)
	res := firstOfType(got, string(MsgTriggerRegistrationResult))
	if res == nil {
		t.Fatal("no triggerregistrationresult sent")
	}
	if stringField(res, "trigger_type") != "cron" {
		t.Errorf("trigger_type = %q, want cron", stringField(res, "trigger_type"))
	}
	if _, hasErr := res["error"]; hasErr {
		t.Error("successful registration must omit error")
	}
}

// TestInboundRegisterTriggerUnknownType replies trigger_type_not_found when no handler
// is registered for the type.
func TestInboundRegisterTriggerUnknownType(t *testing.T) {
	m := newMockEngine(t)
	c := connectClient(t, m)
	_ = c

	m.waitFor(func(msgs []map[string]json.RawMessage) bool {
		return firstOfType(msgs, string(MsgInvokeFunction)) != nil
	}, 2*time.Second)
	m.mu.Lock()
	conn := m.active
	m.mu.Unlock()
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	_ = m.send(ctx, conn, &RegisterTriggerMessage{ID: "x", TriggerType: "ghost", FunctionID: "f", Config: json.RawMessage(`{}`)})

	got := m.waitFor(func(msgs []map[string]json.RawMessage) bool {
		return firstOfType(msgs, string(MsgTriggerRegistrationResult)) != nil
	}, 2*time.Second)
	res := firstOfType(got, string(MsgTriggerRegistrationResult))
	if res == nil {
		t.Fatal("no triggerregistrationresult sent")
	}
	var body ErrorBody
	_ = json.Unmarshal(res["error"], &body)
	if body.Code != "trigger_type_not_found" {
		t.Errorf("code = %q, want trigger_type_not_found", body.Code)
	}
}

// TestHandlerInvocationErrorPassthrough verifies a handler returning a typed
// *InvocationError preserves its code on the wire (not "invocation_failed").
func TestHandlerInvocationErrorPassthrough(t *testing.T) {
	m := newMockEngine(t)
	m.onReceive = func(conn *websocket.Conn, msg map[string]json.RawMessage) {
		if messageType(msg) == string(MsgRegisterFunction) && messageID(msg) == "rbac::fn" {
			id := mustUUID(t, "55555555-5555-5555-5555-555555555555")
			ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
			defer cancel()
			_ = m.send(ctx, conn, &InvokeFunctionMessage{InvocationID: &id, FunctionID: "rbac::fn", Data: json.RawMessage(`{}`)})
		}
	}

	c := connectClient(t, m)
	_ = c.RegisterFunction("rbac::fn", func(ctx context.Context, _ json.RawMessage) (any, error) {
		return nil, &InvocationError{Code: "FORBIDDEN", Message: "denied"}
	})

	got := m.waitFor(func(msgs []map[string]json.RawMessage) bool {
		return firstOfType(msgs, string(MsgInvocationResult)) != nil
	}, 3*time.Second)
	res := firstOfType(got, string(MsgInvocationResult))
	var body ErrorBody
	_ = json.Unmarshal(res["error"], &body)
	if body.Code != "FORBIDDEN" {
		t.Errorf("code = %q, want FORBIDDEN (passthrough)", body.Code)
	}
}

// TestOptionsAndState covers the small accessors.
func TestOptionsAndState(t *testing.T) {
	m := newMockEngine(t)
	c := New(m.url, WithName("worker-x"))
	if c.name != "worker-x" {
		t.Errorf("WithName not applied: %q", c.name)
	}
	if c.State() != StateDisconnected {
		t.Errorf("initial State = %q, want disconnected", c.State())
	}
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	if err := c.Connect(ctx); err != nil {
		t.Fatalf("Connect: %v", err)
	}
	t.Cleanup(func() { _ = c.Close() })
	if c.State() != StateConnected {
		t.Errorf("State after Connect = %q, want connected", c.State())
	}
}

// --- small test helpers ---

func mustUUID(t *testing.T, s string) uuid.UUID {
	t.Helper()
	id, err := uuid.Parse(s)
	if err != nil {
		t.Fatalf("parse uuid %q: %v", s, err)
	}
	return id
}

func firstWhere(msgs []map[string]json.RawMessage, pred func(map[string]json.RawMessage) bool) map[string]json.RawMessage {
	for _, m := range msgs {
		if pred(m) {
			return m
		}
	}
	return nil
}

func indexOfType(msgs []map[string]json.RawMessage, msgType string) int {
	for i, m := range msgs {
		if messageType(m) == msgType {
			return i
		}
	}
	return -1
}

func indexOfFunctionID(msgs []map[string]json.RawMessage, fid string) int {
	for i, m := range msgs {
		if stringField(m, "function_id") == fid {
			return i
		}
	}
	return -1
}

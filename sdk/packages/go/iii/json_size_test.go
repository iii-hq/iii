package iii

import (
	"context"
	"encoding/json"
	"errors"
	"github.com/coder/websocket"
	"github.com/google/uuid"
	"strings"
	"testing"
	"time"
)

func TestJSONSizeBoundaryEscapesAndNull(t *testing.T) {
	base := &InvokeFunctionMessage{FunctionID: "f", Data: json.RawMessage(`""`)}
	wire, _ := MarshalMessage(base)
	for _, delta := range []int{-1, 0, 1} {
		base.Data = json.RawMessage(`"` + strings.Repeat("x", maxJSONFrameBytes-len(wire)+delta) + `"`)
		frame, err := prepareJSON(base)
		if delta > 0 {
			var ie *InvocationError
			if !errors.As(err, &ie) || ie.Code != "payload_too_large" {
				t.Fatalf("above: %v", err)
			}
		} else if err != nil || len(frame) != maxJSONFrameBytes+delta {
			t.Fatalf("boundary: %d %v", len(frame), err)
		}
	}
	escaped, _ := json.Marshal(strings.Repeat("😀é<\n\"\\", 1500000))
	if _, err := prepareJSON(&InvokeFunctionMessage{FunctionID: "f", Data: escaped}); err == nil {
		t.Fatal("escapes accepted")
	}
	id := uuid.New()
	for _, result := range []json.RawMessage{nil, json.RawMessage(`null`)} {
		f, err := prepareJSON(&InvocationResultMessage{InvocationID: id, FunctionID: "f", Result: result})
		if err != nil {
			t.Fatal(err)
		}
		var m map[string]json.RawMessage
		json.Unmarshal(f, &m)
		_, present := m["result"]
		if present != (result != nil) {
			t.Fatalf("null/absent: %s", f)
		}
	}
	secret := "PRIVATE_MARKER"
	f, err := prepareJSON(&InvocationResultMessage{InvocationID: id, FunctionID: "f", Result: base.Data, Baggage: &secret})
	if err != nil {
		t.Fatal(err)
	}
	var m map[string]json.RawMessage
	json.Unmarshal(f, &m)
	if stringField(m, "invocation_id") != id.String() || stringField(m, "function_id") != "f" || strings.Contains(string(f), secret) || m["result"] != nil {
		t.Fatalf("fallback: %s", f)
	}
	var body ErrorBody
	json.Unmarshal(m["error"], &body)
	if body.Code != "payload_too_large" {
		t.Fatalf("fallback: %s", f)
	}
}

func TestJSONLocalQueuesAndRegistrations(t *testing.T) {
	c := New("ws://127.0.0.1:0")
	defer c.Close()
	huge := json.RawMessage(`"` + strings.Repeat("x", maxJSONFrameBytes) + `"`)
	for _, action := range []*TriggerAction{nil, VoidAction(), EnqueueAction("q")} {
		_, err := c.Trigger(context.Background(), TriggerRequest{FunctionID: "f", Data: huge, Action: action})
		var ie *InvocationError
		if !errors.As(err, &ie) || ie.Code != "payload_too_large" {
			t.Fatalf("local: %v", err)
		}
	}
	if len(c.pending) != 0 || len(c.offline) != 0 {
		t.Fatal("pending/offline leak")
	}
	h := func(context.Context, json.RawMessage) (any, error) { return nil, nil }
	if err := c.RegisterFunction("f", h, RegisterFunctionOptions{Metadata: huge}); err == nil {
		t.Fatal("registration accepted")
	}
	typedErr := RegisterFunctionTyped(c, "typed", func(_ context.Context, req string) (string, error) {
		return req, nil
	}, RegisterFunctionOptions{Metadata: huge})
	var typedSizeErr *InvocationError
	if !errors.As(typedErr, &typedSizeErr) || typedSizeErr.Code != "payload_too_large" {
		t.Fatalf("typed registration: %v", typedErr)
	}
	if len(c.functions) != 0 || len(c.offline) != 0 || len(c.outbound) != 0 {
		t.Fatal("typed registration retained or enqueued oversized envelope")
	}
	if err := c.RegisterTriggerType("t", strings.Repeat("x", maxJSONFrameBytes), &stubTriggerHandler{}); err == nil {
		t.Fatal("trigger type accepted")
	}
	if err := c.RegisterTrigger("t", "t", "f", huge); err == nil {
		t.Fatal("trigger accepted")
	}
	if len(c.functions)+len(c.triggers)+len(c.triggerTypes) != 0 {
		t.Fatal("durable leak")
	}
	raw, _ := MarshalMessage(&InvokeFunctionMessage{FunctionID: "f", Data: huge})
	c.enqueueOutbound(raw)
	if len(c.offline) != 0 {
		t.Fatal("raw bypass")
	}
	c.reply = make(chan []byte, 2)
	id := uuid.New()
	raw, _ = MarshalMessage(&InvocationResultMessage{InvocationID: id, FunctionID: "f", Result: huge})
	c.enqueueOutboundDirect(raw)
	var result map[string]json.RawMessage
	json.Unmarshal(<-c.reply, &result)
	if result["result"] != nil || result["error"] == nil {
		t.Fatal("direct fallback")
	}
}

func TestJSONSocketReceiveAndContinuation(t *testing.T) {
	m := newMockEngine(t)
	huge, _ := json.Marshal(strings.Repeat("x", maxJSONFrameBytes+1))
	m.mu.Lock()
	m.onReceive = func(conn *websocket.Conn, msg map[string]json.RawMessage) {
		if messageType(msg) == "invokefunction" && stringField(msg, "function_id") == "remote" {
			var id uuid.UUID
			json.Unmarshal(msg["invocation_id"], &id)
			if err := m.send(context.Background(), conn, &InvocationResultMessage{InvocationID: id, FunctionID: "remote", Result: huge}); err != nil {
				t.Error(err)
			}
		}
	}
	m.mu.Unlock()
	c := connectClient(t, m)
	if err := c.RegisterFunction("length", func(_ context.Context, data json.RawMessage) (any, error) {
		var s string
		json.Unmarshal(data, &s)
		return len(s), nil
	}); err != nil {
		t.Fatal(err)
	}
	m.waitFor(func(msgs []map[string]json.RawMessage) bool { return countType(msgs, "registerfunction") > 0 }, time.Second)
	m.mu.Lock()
	socket := m.active
	m.mu.Unlock()
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	out, err := c.Trigger(ctx, TriggerRequest{FunctionID: "remote"})
	if err != nil || len(out) != len(huge) {
		t.Fatalf("incoming result: %d %v", len(out), err)
	}
	for _, data := range []json.RawMessage{huge, json.RawMessage(`"small"`)} {
		id := uuid.New()
		if err := m.send(ctx, socket, &InvokeFunctionMessage{InvocationID: &id, FunctionID: "length", Data: data}); err != nil {
			t.Fatal(err)
		}
		msgs := m.waitFor(func(msgs []map[string]json.RawMessage) bool {
			return firstWhere(msgs, func(msg map[string]json.RawMessage) bool { return stringField(msg, "invocation_id") == id.String() }) != nil
		}, 5*time.Second)
		frame := firstWhere(msgs, func(msg map[string]json.RawMessage) bool { return stringField(msg, "invocation_id") == id.String() })
		if frame == nil || frame["error"] != nil {
			t.Fatalf("incoming invocation: %v", frame)
		}
	}
	m.mu.Lock()
	same := socket == m.active
	m.mu.Unlock()
	if !same {
		t.Fatal("socket changed")
	}
	if maxJSONMessageBytes != 64*1024*1024 {
		t.Fatal("receive bound")
	}
}

func TestJSONReplayWriterAndReattachGuards(t *testing.T) {
	m := newMockEngine(t)
	c := connectClient(t, m)
	if err := c.RegisterFunction("retained", func(context.Context, json.RawMessage) (any, error) { return nil, nil }); err != nil {
		t.Fatal(err)
	}
	m.waitFor(func(msgs []map[string]json.RawMessage) bool { return countType(msgs, "registerfunction") == 1 }, time.Second)
	m.mu.Lock()
	socket := m.active
	m.mu.Unlock()
	huge := json.RawMessage(`"` + strings.Repeat("x", maxJSONFrameBytes) + `"`)
	id := uuid.New()
	raw, _ := MarshalMessage(&InvokeFunctionMessage{InvocationID: &id, FunctionID: "f", Data: huge})
	pending := make(chan invocationOutcome, 1)
	c.mu.Lock()
	c.pending[id] = pending
	c.offline = [][]byte{raw}
	c.mu.Unlock()
	c.onConnect()
	select {
	case out := <-pending:
		var ie *InvocationError
		if !errors.As(out.err, &ie) {
			t.Fatal(out.err)
		}
	case <-time.After(time.Second):
		t.Fatal("replay pending leak")
	}
	// Deliberately bypass enqueue to exercise the final writer guard.
	c.outbound <- raw
	small, _ := MarshalMessage(&PongMessage{})
	c.outbound <- small
	msgs := m.waitFor(func(msgs []map[string]json.RawMessage) bool { return countType(msgs, "pong") > 0 }, time.Second)
	if countType(msgs, "pong") == 0 {
		t.Fatal("writer stopped on permanent rejection")
	}
	m.mu.Lock()
	same := socket == m.active
	m.mu.Unlock()
	if !same {
		t.Fatal("reconnected")
	}
	if _, err := prepareJSON(&ReattachMessage{PreviousWorkerID: strings.Repeat("x", maxJSONFrameBytes)}); err == nil {
		t.Fatal("reattach bypass")
	}
	c.mu.Lock()
	leak := len(c.pending)
	c.mu.Unlock()
	if leak != 0 {
		t.Fatal("pending leaked")
	}
}

func TestJSONReceiveLimitRejectsAbove64MiB(t *testing.T) {
	m := newMockEngine(t)
	connectClient(t, m)
	m.waitFor(func(msgs []map[string]json.RawMessage) bool { return len(msgs) > 0 }, time.Second)
	m.mu.Lock()
	socket := m.active
	m.mu.Unlock()
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	frame := []byte(`{"type":"unknown","padding":"` + strings.Repeat("x", maxJSONMessageBytes) + `"}`)
	_ = m.sendRaw(ctx, socket, frame)
	deadline := time.Now().Add(5 * time.Second)
	for time.Now().Before(deadline) {
		m.mu.Lock()
		replaced := m.active != socket
		m.mu.Unlock()
		if replaced {
			return
		}
		time.Sleep(10 * time.Millisecond)
	}
	t.Fatal("receive limit did not disconnect oversized inbound message")
}

func TestJSONNullAbsentOnWire(t *testing.T) {
	m := newMockEngine(t)
	c := connectClient(t, m)
	if err := c.RegisterFunction("null", func(context.Context, json.RawMessage) (any, error) { return nil, nil }); err != nil {
		t.Fatal(err)
	}
	m.waitFor(func(msgs []map[string]json.RawMessage) bool { return countType(msgs, "registerfunction") > 0 }, time.Second)
	m.mu.Lock()
	socket := m.active
	m.mu.Unlock()
	for _, function := range []string{"null", "missing"} {
		id := uuid.New()
		if err := m.send(context.Background(), socket, &InvokeFunctionMessage{InvocationID: &id, FunctionID: function, Data: json.RawMessage(`{}`)}); err != nil {
			t.Fatal(err)
		}
		msgs := m.waitFor(func(msgs []map[string]json.RawMessage) bool {
			return firstWhere(msgs, func(m map[string]json.RawMessage) bool { return stringField(m, "invocation_id") == id.String() }) != nil
		}, time.Second)
		frame := firstWhere(msgs, func(m map[string]json.RawMessage) bool { return stringField(m, "invocation_id") == id.String() })
		if frame == nil {
			t.Fatal("missing reply")
		}
		_, present := frame["result"]
		if function == "null" {
			if !present || string(frame["result"]) != "null" {
				t.Fatalf("null lost: %v", frame)
			}
		} else if present {
			t.Fatalf("absent changed: %v", frame)
		}
	}
}

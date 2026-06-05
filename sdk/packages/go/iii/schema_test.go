package iii

import (
	"context"
	"encoding/json"
	"testing"
	"time"

	"github.com/coder/websocket"
)

type greetReq struct {
	Name  string `json:"name" jsonschema:"required"`
	Count int    `json:"count"`
}

type greetResp struct {
	Message string `json:"message"`
}

// TestInferSchema checks that a struct reflects into a JSON Schema describing its fields
// and types — the Go counterpart of Rust's JsonSchema derive.
func TestInferSchema(t *testing.T) {
	raw, err := InferSchema[greetReq]()
	if err != nil {
		t.Fatalf("InferSchema: %v", err)
	}
	var schema map[string]any
	if err := json.Unmarshal(raw, &schema); err != nil {
		t.Fatalf("schema is not valid JSON: %v\n%s", err, raw)
	}
	if schema["type"] != "object" {
		t.Errorf("schema type = %v, want object", schema["type"])
	}
	props, ok := schema["properties"].(map[string]any)
	if !ok {
		t.Fatalf("schema has no properties object: %s", raw)
	}
	if _, ok := props["name"]; !ok {
		t.Error("schema missing 'name' property")
	}
	if _, ok := props["count"]; !ok {
		t.Error("schema missing 'count' property")
	}
	// `required` from the jsonschema tag should surface name.
	if req, ok := schema["required"].([]any); ok {
		found := false
		for _, r := range req {
			if r == "name" {
				found = true
			}
		}
		if !found {
			t.Error("'name' should be required")
		}
	} else {
		t.Error("schema has no required list")
	}
}

// TestRegisterFunctionTypedSendsSchema verifies the typed registration sends a
// registerfunction frame carrying the inferred request/response schemas, and that the
// handler receives the unmarshaled typed request.
func TestRegisterFunctionTypedSendsSchema(t *testing.T) {
	m := newMockEngine(t)

	gotReq := make(chan greetReq, 1)
	m.onReceive = func(conn *websocket.Conn, msg map[string]json.RawMessage) {
		if messageType(msg) == string(MsgRegisterFunction) && messageID(msg) == "typed::greet" {
			// The frame must carry a non-null request_format describing the struct.
			if string(msg["request_format"]) == "null" || len(msg["request_format"]) == 0 {
				t.Error("registerfunction did not carry an inferred request_format")
			}
			id := mustUUID(t, "66666666-6666-6666-6666-666666666666")
			ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
			defer cancel()
			_ = m.send(ctx, conn, &InvokeFunctionMessage{
				InvocationID: &id,
				FunctionID:   "typed::greet",
				Data:         json.RawMessage(`{"name":"ada","count":2}`),
			})
		}
	}

	c := connectClient(t, m)
	err := RegisterFunctionTyped[greetReq, greetResp](c, "typed::greet",
		func(ctx context.Context, req greetReq) (greetResp, error) {
			gotReq <- req
			return greetResp{Message: "hi " + req.Name}, nil
		})
	if err != nil {
		t.Fatalf("RegisterFunctionTyped: %v", err)
	}

	select {
	case req := <-gotReq:
		if req.Name != "ada" || req.Count != 2 {
			t.Errorf("handler got %+v, want name=ada count=2", req)
		}
	case <-time.After(3 * time.Second):
		t.Fatal("typed handler was not invoked")
	}
}

// TestRegisterWorkerConnects verifies RegisterWorker starts the lifecycle so the client
// reaches connected without a separate Connect call.
func TestRegisterWorkerConnects(t *testing.T) {
	m := newMockEngine(t)
	c := RegisterWorker(m.url)
	t.Cleanup(func() { _ = c.Close() })

	// Wait for connection by polling State (RegisterWorker connects in the background).
	deadline := time.Now().Add(3 * time.Second)
	for time.Now().Before(deadline) {
		if c.State() == StateConnected {
			return
		}
		time.Sleep(20 * time.Millisecond)
	}
	t.Fatalf("RegisterWorker did not reach connected; state = %s", c.State())
}

// TestConnectFailsFastWhenRetriesExhausted verifies Connect returns (rather than
// blocking forever) when the supervisor gives up after MaxRetries.
func TestConnectFailsFastWhenRetriesExhausted(t *testing.T) {
	// Point at a dead address with a tiny, finite retry budget.
	c := New("ws://127.0.0.1:1", WithReconnectConfig(ReconnectConfig{
		InitialDelay:      time.Millisecond,
		MaxDelay:          time.Millisecond,
		BackoffMultiplier: 1,
		JitterFactor:      0,
		MaxRetries:        2,
	}))
	t.Cleanup(func() { _ = c.Close() })

	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	err := c.Connect(ctx)
	if err == nil {
		t.Fatal("Connect should fail when the engine is unreachable and retries are exhausted")
	}
	if ctx.Err() != nil {
		t.Fatal("Connect blocked until ctx timeout instead of failing fast on exhausted retries")
	}
}

// TestConnectIdempotentSingleSupervisor verifies multiple Connect calls don't spawn
// multiple supervisors (no panic / race; the second call observes the same outcome).
func TestConnectIdempotentSingleSupervisor(t *testing.T) {
	m := newMockEngine(t)
	c := New(m.url)
	t.Cleanup(func() { _ = c.Close() })

	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	if err := c.Connect(ctx); err != nil {
		t.Fatalf("first Connect: %v", err)
	}
	// Second call must return promptly (already connected), not start a new supervisor.
	if err := c.Connect(ctx); err != nil {
		t.Fatalf("second Connect: %v", err)
	}
}

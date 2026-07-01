//go:build integration

package iii_test

import (
	"context"
	"encoding/json"
	"sync"
	"testing"
	"time"

	iii "github.com/iii-hq/iii/sdk/packages/go/iii"
)

// Mirrors sdk/packages/rust/iii/tests/bridge.rs.

// TestConnectSuccessfully confirms the client connects and can invoke an engine builtin
// (engine::functions::list returns a functions array).
func TestConnectSuccessfully(t *testing.T) {
	c := connect(t)
	res, err := c.Trigger(ctxFor(t, 5*time.Second), iii.TriggerRequest{
		FunctionID: iii.FnListFunctions,
		Data:       json.RawMessage(`{}`),
	})
	if err != nil {
		t.Fatalf("engine::functions::list: %v", err)
	}
	var out struct {
		Functions []json.RawMessage `json:"functions"`
	}
	if err := json.Unmarshal(res, &out); err != nil {
		t.Fatalf("decode functions list: %v\nraw: %s", err, res)
	}
	// A valid list (possibly empty) is enough; we only assert the shape.
}

// TestRegisterAndInvokeFunction registers a function and invokes it round-trip.
func TestRegisterAndInvokeFunction(t *testing.T) {
	c := connect(t)

	var (
		mu       sync.Mutex
		received []map[string]any
	)
	if err := c.RegisterFunction("test::bridge::go::echo", func(ctx context.Context, data json.RawMessage) (any, error) {
		var in map[string]any
		_ = json.Unmarshal(data, &in)
		mu.Lock()
		received = append(received, in)
		mu.Unlock()
		return map[string]any{"echoed": in}, nil
	}); err != nil {
		t.Fatalf("RegisterFunction: %v", err)
	}
	settle()

	res, err := c.Trigger(ctxFor(t, 5*time.Second), iii.TriggerRequest{
		FunctionID: "test::bridge::go::echo",
		Data:       json.RawMessage(`{"message":"hello"}`),
	})
	if err != nil {
		t.Fatalf("Trigger: %v", err)
	}
	var out struct {
		Echoed struct {
			Message string `json:"message"`
		} `json:"echoed"`
	}
	if err := json.Unmarshal(res, &out); err != nil {
		t.Fatalf("decode result: %v\nraw: %s", err, res)
	}
	if out.Echoed.Message != "hello" {
		t.Errorf("echoed.message = %q, want hello", out.Echoed.Message)
	}
	mu.Lock()
	defer mu.Unlock()
	if len(received) == 0 || received[0]["message"] != "hello" {
		t.Errorf("handler received %v, want a message=hello", received)
	}
}

// TestInvokeFireAndForget sends a void invocation; the handler runs but no reply is
// awaited. We confirm the handler was reached.
func TestInvokeFireAndForget(t *testing.T) {
	c := connect(t)

	done := make(chan map[string]any, 1)
	if err := c.RegisterFunction("test::bridge::go::sink", func(ctx context.Context, data json.RawMessage) (any, error) {
		var in map[string]any
		_ = json.Unmarshal(data, &in)
		select {
		case done <- in:
		default:
		}
		return nil, nil
	}); err != nil {
		t.Fatalf("RegisterFunction: %v", err)
	}
	settle()

	res, err := c.Trigger(ctxFor(t, 5*time.Second), iii.TriggerRequest{
		FunctionID: "test::bridge::go::sink",
		Data:       json.RawMessage(`{"event":"ping"}`),
		Action:     iii.VoidAction(),
	})
	if err != nil {
		t.Fatalf("void Trigger: %v", err)
	}
	if res != nil {
		t.Errorf("void Trigger result = %s, want nil", res)
	}

	select {
	case got := <-done:
		if got["event"] != "ping" {
			t.Errorf("handler got %v, want event=ping", got)
		}
	case <-time.After(3 * time.Second):
		t.Fatal("fire-and-forget handler was not invoked")
	}
}

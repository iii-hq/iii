//go:build integration

package iii_test

import (
	"context"
	"encoding/json"
	"testing"
	"time"

	iii "github.com/iii-hq/iii/sdk/packages/go/iii"
)

// Mirrors sdk/packages/rust/iii/tests/queue_integration.rs: an enqueue-action trigger
// routes the invocation through a named queue and the call awaits the engine's receipt
// (an ordinary invocation result). We register the target function, enqueue a call, and
// assert the handler eventually runs and the enqueue returns without error.

func TestEnqueueRoutesThroughQueue(t *testing.T) {
	c := connect(t)

	done := make(chan map[string]any, 1)
	if err := c.RegisterFunction("test::queue::go::worker", func(ctx context.Context, data, _ json.RawMessage) (any, error) {
		var in map[string]any
		_ = json.Unmarshal(data, &in)
		select {
		case done <- in:
		default:
		}
		return map[string]any{"processed": true}, nil
	}); err != nil {
		t.Fatalf("RegisterFunction: %v", err)
	}
	settle()

	// Enqueue: the result is the engine's enqueue receipt, not the function output.
	_, err := c.Trigger(ctxFor(t, 10*time.Second), iii.TriggerRequest{
		FunctionID: "test::queue::go::worker",
		Data:       json.RawMessage(`{"job":"a"}`),
		Action:     iii.EnqueueAction("default"),
		Timeout:    10 * time.Second,
	})
	if err != nil {
		t.Fatalf("enqueue Trigger: %v", err)
	}

	// The queued job should be delivered to the handler.
	select {
	case got := <-done:
		if got["job"] != "a" {
			t.Errorf("handler got %v, want job=a", got)
		}
	case <-time.After(8 * time.Second):
		t.Fatal("queued job was not delivered to the handler")
	}

	// The handler returning isn't enough: its invocation result (the queue's ack) must
	// reach the engine before the cleanup unregisters the function, or the queue redrives
	// the "stopped" job into the DLQ. Settle briefly so the ack flushes.
	settle()
}

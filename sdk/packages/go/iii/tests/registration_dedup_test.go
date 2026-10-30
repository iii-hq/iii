//go:build integration

package iii_test

import (
	"context"
	"encoding/json"
	"strings"
	"testing"
	"time"

	iii "github.com/iii-hq/iii/sdk/packages/go/iii"
)

// Registering a function id that is already registered fails (as in the Node, Python and
// Rust SDKs), and re-registering it after Unregister must not create duplicate entries
// on the engine: the function appears exactly once in engine::functions::list.

func TestRegistrationDoesNotDuplicate(t *testing.T) {
	c := connect(t)
	const fnID = "test::dedup::go::fn"

	handler := func(_ context.Context, _ json.RawMessage) (any, error) { return nil, nil }
	first, err := c.RegisterFunction(fnID, handler)
	if err != nil {
		t.Fatalf("RegisterFunction #1: %v", err)
	}
	if _, err := c.RegisterFunction(fnID, handler); err == nil || !strings.Contains(err.Error(), "function id already registered") {
		t.Fatalf("RegisterFunction #2: %v, want function id already registered", err)
	}
	settle()
	if err := first.Unregister(); err != nil {
		t.Fatalf("Unregister: %v", err)
	}
	if _, err := c.RegisterFunction(fnID, handler); err != nil {
		t.Fatalf("RegisterFunction after Unregister: %v", err)
	}
	settle()

	res, err := c.Trigger(ctxFor(t, 5*time.Second), iii.TriggerRequest{
		FunctionID: iii.FnListFunctions,
		Payload:    json.RawMessage(`{}`),
	})
	if err != nil {
		t.Fatalf("engine::functions::list: %v", err)
	}
	var out struct {
		Functions []struct {
			FunctionID string `json:"function_id"`
		} `json:"functions"`
	}
	if err := json.Unmarshal(res, &out); err != nil {
		t.Fatalf("decode functions: %v\nraw: %s", err, res)
	}

	count := 0
	for _, f := range out.Functions {
		if f.FunctionID == fnID {
			count++
		}
	}
	if count != 1 {
		t.Errorf("%q appears %d times in engine::functions::list, want exactly 1", fnID, count)
	}
}

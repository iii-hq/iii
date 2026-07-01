//go:build integration

package iii_test

import (
	"context"
	"encoding/json"
	"testing"
	"time"

	iii "github.com/iii-hq/iii/sdk/packages/go/iii"
)

// Mirrors sdk/packages/rust/iii/tests/registration_dedup.rs: registering the same
// function id twice (e.g. across a re-register) must not create duplicate entries on the
// engine. We assert the function appears exactly once in engine::functions::list.

func TestRegistrationDoesNotDuplicate(t *testing.T) {
	c := connect(t)
	const fnID = "test::dedup::go::fn"

	handler := func(_ context.Context, _, _ json.RawMessage) (any, error) { return nil, nil }
	// Register the same id twice; the second replaces the first, not duplicates it.
	if err := c.RegisterFunction(fnID, handler); err != nil {
		t.Fatalf("RegisterFunction #1: %v", err)
	}
	if err := c.RegisterFunction(fnID, handler); err != nil {
		t.Fatalf("RegisterFunction #2: %v", err)
	}
	settle()

	res, err := c.Trigger(ctxFor(t, 5*time.Second), iii.TriggerRequest{
		FunctionID: iii.FnListFunctions,
		Data:       json.RawMessage(`{}`),
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

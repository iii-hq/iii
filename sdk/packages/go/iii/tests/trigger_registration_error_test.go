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

// Mirrors sdk/packages/node/iii/tests/trigger-registration-error.test.ts: registering a
// trigger whose trigger type is not handled by any worker yields an error from the
// engine rather than silently succeeding.

func TestRegisterTriggerUnknownType(t *testing.T) {
	c := connect(t)
	// Register a function the trigger would target, but never register a handler for the
	// (made-up) trigger type, so the engine has nowhere to route the registration.
	if err := c.RegisterFunction("test::trigerr::go::fn", func(_ context.Context, _, _ json.RawMessage) (any, error) {
		return nil, nil
	}); err != nil {
		t.Fatalf("RegisterFunction: %v", err)
	}
	triggerID := "test-trigerr-go-" + uniqueSuffix(t)
	if err := c.RegisterTrigger(
		triggerID,
		"this-trigger-type-does-not-exist",
		"test::trigerr::go::fn",
		json.RawMessage(`{}`),
		nil,
	); err != nil {
		t.Fatalf("RegisterTrigger (client-side enqueue): %v", err)
	}
	settle()

	// The engine rejects a trigger whose type no worker provides (it surfaces the failure
	// asynchronously as a triggerregistrationresult error, which the SDK logs rather than
	// returning — matching the reference SDKs). The observable, version-stable signal is
	// that the rejected trigger never becomes an ACTIVE registered trigger: it must not
	// appear in engine::registered-triggers::list.
	res, err := c.Trigger(ctxFor(t, 5*time.Second), iii.TriggerRequest{
		FunctionID: iii.FnListRegisteredTriggers,
		Data:       json.RawMessage(`{}`),
	})
	if err != nil {
		t.Fatalf("engine::registered-triggers::list: %v", err)
	}
	var out struct {
		Triggers []struct {
			ID string `json:"id"`
		} `json:"triggers"`
	}
	// The response key may be "triggers" or a bare array depending on engine version;
	// decode leniently and scan for our id.
	if err := json.Unmarshal(res, &out); err != nil {
		// Fall back: ensure our id simply isn't present anywhere in the payload.
		if strings.Contains(string(res), triggerID) {
			t.Fatalf("rejected trigger %q appears in registered-triggers list: %s", triggerID, res)
		}
		return
	}
	for _, tr := range out.Triggers {
		if tr.ID == triggerID {
			t.Errorf("rejected trigger %q must not be an active registered trigger", triggerID)
		}
	}

	// And the worker is still usable after the rejection.
	if _, err := c.Trigger(ctxFor(t, 5*time.Second), iii.TriggerRequest{
		FunctionID: iii.FnListFunctions,
		Data:       json.RawMessage(`{}`),
	}); err != nil {
		t.Fatalf("worker unusable after failed trigger registration: %v", err)
	}
}

//go:build integration

package iii_test

import (
	"context"
	"encoding/json"
	"errors"
	"testing"
	"time"

	iii "github.com/iii-hq/iii/sdk/packages/go/iii"
)

// Mirrors sdk/packages/node/iii/tests/errors.test.ts: a handler error surfaces to the
// caller as *InvocationError, and a call to a nonexistent function / a timeout map to
// the right typed errors.

func TestHandlerErrorSurfacesAsInvocationError(t *testing.T) {
	c := connect(t)
	if err := c.RegisterFunction("test::errors::go::boom", func(ctx context.Context, _, _ json.RawMessage) (any, error) {
		return nil, errors.New("kaboom")
	}); err != nil {
		t.Fatalf("RegisterFunction: %v", err)
	}
	settle()

	_, err := c.Trigger(ctxFor(t, 5*time.Second), iii.TriggerRequest{
		FunctionID: "test::errors::go::boom",
		Data:       json.RawMessage(`{}`),
	})
	if err == nil {
		t.Fatal("expected an error from a failing handler")
	}
	var ie *iii.InvocationError
	if !errors.As(err, &ie) {
		t.Fatalf("error is not *InvocationError: %v", err)
	}
	// The engine reports a handler failure as code "invocation_failed".
	if ie.Code == "" {
		t.Error("InvocationError.Code is empty")
	}
}

func TestTypedInvocationErrorPropagates(t *testing.T) {
	c := connect(t)
	if err := c.RegisterFunction("test::errors::go::forbidden", func(ctx context.Context, _, _ json.RawMessage) (any, error) {
		return nil, &iii.InvocationError{Code: "FORBIDDEN", Message: "nope"}
	}); err != nil {
		t.Fatalf("RegisterFunction: %v", err)
	}
	settle()

	_, err := c.Trigger(ctxFor(t, 5*time.Second), iii.TriggerRequest{
		FunctionID: "test::errors::go::forbidden",
		Data:       json.RawMessage(`{}`),
	})
	var ie *iii.InvocationError
	if !errors.As(err, &ie) {
		t.Fatalf("error is not *InvocationError: %v", err)
	}
	if ie.Code != "FORBIDDEN" {
		t.Errorf("code = %q, want FORBIDDEN (typed error should round-trip)", ie.Code)
	}
}

func TestInvokeUnknownFunctionErrors(t *testing.T) {
	c := connect(t)
	_, err := c.Trigger(ctxFor(t, 5*time.Second), iii.TriggerRequest{
		FunctionID: "test::errors::go::does-not-exist",
		Data:       json.RawMessage(`{}`),
		Timeout:    3 * time.Second,
	})
	if err == nil {
		t.Fatal("expected an error invoking an unregistered function")
	}
}

package iii

import (
	"errors"
	"fmt"
	"strings"
	"testing"
)

// ErrorBody itself implements error (it is the wire shape); its string form is
// "code: message", which the richer InvocationError builds on.
func TestErrorBodyError(t *testing.T) {
	body := &ErrorBody{Code: "FORBIDDEN", Message: "no access"}
	if got := body.Error(); got != "FORBIDDEN: no access" {
		t.Errorf("ErrorBody.Error() = %q, want %q", got, "FORBIDDEN: no access")
	}
}

func TestInvocationErrorFromBody(t *testing.T) {
	trace := "at handler (foo.go:10)"
	tests := []struct {
		name       string
		body       *ErrorBody
		functionID string
		wantCode   string
		wantTrace  string
		wantInMsg  []string
	}{
		{
			name:       "with function id and stacktrace",
			body:       &ErrorBody{Code: "FORBIDDEN", Message: "no access", Stacktrace: &trace},
			functionID: "secret::read",
			wantCode:   "FORBIDDEN",
			wantTrace:  trace,
			// The function id and code must both be in the message so a rejection is
			// self-describing (the reason Node introduced IIIInvocationError).
			wantInMsg: []string{"secret::read", "FORBIDDEN", "no access"},
		},
		{
			name:      "without function id omits it from message",
			body:      &ErrorBody{Code: "INTERNAL", Message: "boom"},
			wantCode:  "INTERNAL",
			wantTrace: "",
			wantInMsg: []string{"INTERNAL", "boom"},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			err := newInvocationError(tt.body, tt.functionID)
			if err.Code != tt.wantCode {
				t.Errorf("Code = %q, want %q", err.Code, tt.wantCode)
			}
			if err.Stacktrace != tt.wantTrace {
				t.Errorf("Stacktrace = %q, want %q", err.Stacktrace, tt.wantTrace)
			}
			msg := err.Error()
			for _, want := range tt.wantInMsg {
				if !strings.Contains(msg, want) {
					t.Errorf("Error() = %q, want it to contain %q", msg, want)
				}
			}
		})
	}
}

// errors.As must recover the concrete InvocationError through a wrapping chain, so
// callers can branch on Code regardless of how deep the error was wrapped.
func TestInvocationErrorAsThroughWrap(t *testing.T) {
	base := newInvocationError(&ErrorBody{Code: "FORBIDDEN", Message: "denied"}, "fn")
	wrapped := fmt.Errorf("dispatch failed: %w", base)

	var ie *InvocationError
	if !errors.As(wrapped, &ie) {
		t.Fatal("errors.As did not recover *InvocationError")
	}
	if ie.Code != "FORBIDDEN" {
		t.Errorf("Code = %q, want FORBIDDEN", ie.Code)
	}
}

// The payloadless failure modes are sentinels, matchable with errors.Is even when
// wrapped — the Go counterpart of Rust's IIIError::NotConnected / ::Timeout.
func TestSentinelErrorsMatchWhenWrapped(t *testing.T) {
	for _, sentinel := range []error{ErrNotConnected, ErrTimeout} {
		wrapped := fmt.Errorf("trigger: %w", sentinel)
		if !errors.Is(wrapped, sentinel) {
			t.Errorf("errors.Is failed to match wrapped %v", sentinel)
		}
	}
}

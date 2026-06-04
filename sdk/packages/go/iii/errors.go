package iii

import (
	"errors"
	"fmt"
)

// This file is the ergonomic error surface over the wire ErrorBody (protocol.go).
// It mirrors the two reference SDKs' error models, mapped to Go idiom:
//
//   - Node's IIIInvocationError (errors.ts) — a single typed error carrying the
//     engine's {code, message, stacktrace} plus the targeted function_id — becomes
//     InvocationError below.
//   - Rust's IIIError enum (error.rs) — NotConnected, Timeout, Remote{...} — becomes
//     the ErrNotConnected / ErrTimeout sentinels plus InvocationError for the Remote
//     case, so callers use errors.Is for the flag-like failures and errors.As to read
//     a remote error's code/stacktrace.

// Sentinel errors for the two failure modes that carry no payload. Compare with
// errors.Is. These line up with the Rust IIIError::NotConnected / ::Timeout variants.
var (
	// ErrNotConnected is returned by Trigger when the client has no live connection
	// and the call cannot be buffered (e.g. an await-style Trigger needs a round trip).
	ErrNotConnected = errors.New("iii: not connected")
	// ErrTimeout is returned when no InvocationResult arrives within the call's timeout
	// (DefaultInvocationTimeout unless overridden).
	ErrTimeout = errors.New("iii: invocation timed out")
)

// InvocationError wraps a remote ErrorBody returned by the engine in
// InvocationResult.error, annotated with the function that was invoked. It is the Go
// counterpart of Node's IIIInvocationError: one error type across all remote failure
// modes (RBAC FORBIDDEN, handler error, …), disambiguated by Code.
//
// Recover the details with errors.As:
//
//	var ie *iii.InvocationError
//	if errors.As(err, &ie) && ie.Code == "FORBIDDEN" { ... }
type InvocationError struct {
	// Code is the engine's machine-readable error code (e.g. "FORBIDDEN").
	Code string
	// Message is the human-readable description.
	Message string
	// FunctionID is the function whose invocation failed; empty if not known.
	FunctionID string
	// Stacktrace is the remote stack trace, when the engine provides one.
	Stacktrace string
}

func (e *InvocationError) Error() string {
	if e.FunctionID != "" {
		return fmt.Sprintf("iii: invocation of %q failed: %s: %s", e.FunctionID, e.Code, e.Message)
	}
	return fmt.Sprintf("iii: invocation failed: %s: %s", e.Code, e.Message)
}

// newInvocationError builds an InvocationError from a wire ErrorBody and the function
// it targeted. body must be non-nil.
func newInvocationError(body *ErrorBody, functionID string) *InvocationError {
	e := &InvocationError{
		Code:       body.Code,
		Message:    body.Message,
		FunctionID: functionID,
	}
	if body.Stacktrace != nil {
		e.Stacktrace = *body.Stacktrace
	}
	return e
}

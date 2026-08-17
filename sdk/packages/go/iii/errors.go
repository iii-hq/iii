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

// RegistrationRejectedError is the terminal error the engine returns when a
// worker's registration is refused for good — typically a WORKER_NAMESPACE_CONFLICT,
// where another live worker already owns this name in the namespace. It is
// delivered to every pending invocation and reported by [Client.FatalError];
// the worker stops and does not reconnect. Mirrors the Node/Python/Rust SDKs.
//
// Recover the details with errors.As:
//
//	var re *iii.RegistrationRejectedError
//	if errors.As(err, &re) { ... re.Code, re.Namespace ... }
type RegistrationRejectedError struct {
	// Code is the rejection code, e.g. "WORKER_NAMESPACE_CONFLICT".
	Code string
	// Namespace is the namespace the conflict occurred in.
	Namespace string
	// WorkerName is the rejected worker name for a worker conflict.
	WorkerName string
	// FunctionID is the rejected function id for a function conflict.
	FunctionID string
	// OwnerWorkerID is the id of the worker that already owns the name.
	OwnerWorkerID string
}

func (e *RegistrationRejectedError) Error() string {
	if e.FunctionID != "" {
		return fmt.Sprintf("iii: registration rejected (%s): function %q in namespace %q already owned by worker %s",
			e.Code, e.FunctionID, e.Namespace, e.OwnerWorkerID)
	}
	return fmt.Sprintf("iii: registration rejected (%s): worker %q in namespace %q already owned by worker %s",
		e.Code, e.WorkerName, e.Namespace, e.OwnerWorkerID)
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

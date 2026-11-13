package iii

import (
	"errors"
	"fmt"
)

// This file is the ergonomic error surface over the wire ErrorBody (protocol.go).
// It mirrors the two reference SDKs' error models, mapped to Go idiom:
//
//   - Node's IIIInvocationError (errors.ts), a single typed error carrying the
//     engine's {code, message, stacktrace} plus the targeted function_id, becomes
//     InvocationError below.
//   - Rust's IIIError enum (error.rs: NotConnected, Timeout, Remote{...}) becomes
//     the ErrNotConnected / ErrTimeout sentinels plus InvocationError for the Remote
//     case, so callers use errors.Is for the flag-like failures and errors.As to read
//     a remote error's code/stacktrace.

// Sentinel errors for the two failure modes that carry no payload. Compare with
// errors.Is. These line up with the Rust IIIError::NotConnected / ::Timeout variants.
var (
	// ErrNotConnected is returned by Call when the client has no live connection and
	// the call cannot be buffered (a call through an observer, or one cancelled by
	// shutdown).
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
// worker's registration is refused for good, typically a WORKER_NAMESPACE_CONFLICT,
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

// The codes of the stream errors: an [*InvocationError] with one of these Code values,
// returned by stream operations and by a call whose streams failed. Recover the code
// with errors.As:
//
//	var ie *iii.InvocationError
//	if errors.As(err, &ie) && ie.Code == iii.CodeStreamAborted {
//	    // the writer aborted: the data read so far is incomplete
//	}
const (
	// CodeStreamAborted: the stream ended with an abort (its writer called
	// [OutputStream.Abort], failed, or went away before it closed), or could not be
	// opened. The data read before it is not the whole stream. A call whose sink
	// received an aborted stream fails with it too, even when the function succeeded.
	// Through an engine pipe the Message is the engine's reason (channel writer aborted:
	// closed with code 1011); in-process it is the writer's own.
	CodeStreamAborted = "STREAM_ABORTED"
	// CodeStreamNotAllowed: a payload holding a stream sent with [EnqueueAction]. A
	// queued job can run after the pipe expired, and a retry cannot read the stream
	// again. Nothing was sent.
	CodeStreamNotAllowed = "STREAM_NOT_ALLOWED"
	// CodeStreamClosed: a write or a send on a stream already closed or aborted, or
	// after its reader left.
	CodeStreamClosed = "STREAM_CLOSED"
	// CodeStreamInUse: a stream already in use: a second read of an [InputStream], the
	// same stream twice in one payload, or a stream sent on after it was read to the end
	// or already sent.
	CodeStreamInUse = "STREAM_IN_USE"
)

func streamError(code, message string) *InvocationError {
	return &InvocationError{Code: code, Message: message}
}

func streamAbortedError(reason string) *InvocationError {
	return streamError(CodeStreamAborted, reason)
}

func streamClosedError(message string) *InvocationError {
	return streamError(CodeStreamClosed, message)
}

func streamInUseError(message string) *InvocationError {
	return streamError(CodeStreamInUse, message)
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

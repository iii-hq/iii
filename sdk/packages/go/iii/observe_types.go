package iii

import (
	"encoding/json"
	"fmt"
	"time"
)

// This file holds the public types of observe: the observing side ([Source],
// [ObserverOptions], [ObserverHandle]) and the hosting side ([Observer], [Call] and the
// hook types). The wire shapes are in protocol.go.

// Capture names a part of an observed invocation shipped to an observer's target.
// The default is input, output and caller.
type Capture string

const (
	CaptureInput    Capture = "input"
	CaptureOutput   Capture = "output"
	CaptureError    Capture = "error"
	CaptureCaller   Capture = "caller"
	CaptureMetadata Capture = "metadata"
)

// On selects which calls of a function reach an observer, by their status. The default
// is [OnSuccess].
type On string

const (
	OnSuccess  On = "success"
	OnError    On = "error"
	OnComplete On = "complete"
)

// Deliver selects what an observer's target receives: the whole [Observed] envelope
// (the default), or only the observed input or output.
type Deliver string

const (
	DeliverEnvelope Deliver = "envelope"
	DeliverInput    Deliver = "input"
	DeliverOutput   Deliver = "output"
)

// ObserverState is an observer's lifecycle: pending (no host yet, waiting for the
// host's ack, or no target yet) -> active or error; suspended while its worker or
// target is away; disposed at the end.
type ObserverState string

const (
	ObserverPending   ObserverState = "pending"
	ObserverActive    ObserverState = "active"
	ObserverSuspended ObserverState = "suspended"
	ObserverError     ObserverState = "error"
	ObserverDisposed  ObserverState = "disposed"
)

// Dispatch selects how [Client.Call] reaches a function this worker hosts.
type Dispatch string

const (
	// DispatchAuto (the default, also the zero value) runs a function this worker
	// hosts in-process once the engine confirmed it for local dispatch.
	DispatchAuto Dispatch = "auto"
	// DispatchRemote always goes through the engine.
	DispatchRemote Dispatch = "remote"
)

// Args are an observer's arguments, for the worker that hosts the observed function:
// validated against the function's args_format by the engine, and read by its hooks.
type Args map[string]any

// Filters is a declarative filter over input, output, error, caller and metadata, with
// explicit operators only (the language of the filter subpackage):
//
//	iii.Filters{"input.amount": iii.Op{"$gte": 500}}
//
// The engine evaluates them for the calls it routes, and the worker that hosts the
// function for the calls it makes to it in-process. Both match through an index of the
// observers' equality conditions: each observer is filed under one $eq of a scalar, or
// one $in of scalars, on a path of its filter's top-level $and, and costs a call nothing
// when the call's value at that path is none of that condition's values. A call
// evaluates the whole filter of the observers filed under its value and of those with
// no such condition.
type Filters map[string]any

// Op is the operator object of one filter path: iii.Op{"$eq": "insert"}.
type Op map[string]any

// Source is the left side of [Client.Observe]: what is observed.
type Source struct {
	// FunctionID is the observed function.
	FunctionID string
	// Namespace defaults to this worker's (engine::* and configuration::* resolve to
	// the engine's default namespace).
	Namespace string
	// Args go to the worker that hosts the function, which reads them in its hooks
	// (a cron expression, a route, a table). A function without args_format takes none.
	Args Args
	// Filters decide which calls reach the target; nil matches every call.
	Filters Filters
	// On selects calls by status; empty means [OnSuccess].
	On On
	// Capture is what the envelope carries. nil means the default (input, output,
	// caller); an empty, non-nil slice captures nothing.
	Capture []Capture
	// Deliver is what the target receives when iii delivers; empty means
	// [DeliverEnvelope].
	Deliver Deliver
}

// ObserverOptions are the observer's own settings: its id, policy and metadata. A zero
// field is unset.
type ObserverOptions struct {
	// ID defaults to a random UUID, stable across replays.
	ID string
	// MaxFires disposes the observer after this many activations.
	MaxFires uint64
	// ExpiresIn disposes the observer after this long (millisecond precision).
	ExpiresIn time.Duration
	// MaxInFlight bounds the observer's concurrent activations.
	MaxInFlight uint64
	// Metadata is delivered as the metadata sidecar of every activation (a JSON
	// object).
	Metadata json.RawMessage
	// TargetNamespace is the namespace of the target function; it defaults to this
	// worker's.
	TargetNamespace string
}

// ObserveReason is why an observer is in its state: a machine-readable code and a
// message.
type ObserveReason struct {
	Code    string
	Message string
}

// ObserveRejectionError is an observe error carrying its code: what
// [ObserverHandle.Ready] returns when the observer turns error or disposed, and what an
// [AddedHook] returns (built with [ObserveRejection]) to reject an observer with a code
// of its own. Recover it with errors.As.
type ObserveRejectionError struct {
	Code    string
	Message string
}

func (e *ObserveRejectionError) Error() string {
	return fmt.Sprintf("iii: observe rejected: %s: %s", e.Code, e.Message)
}

// ObserveRejection builds the error an [AddedHook] returns to reject an observer with
// code (any other error rejects it with OBSERVE_REJECTED).
func ObserveRejection(code, message string) error {
	return &ObserveRejectionError{Code: code, Message: message}
}

// FunctionOptions is another name for [RegisterFunctionOptions], the one the cross-SDK
// specification uses.
type FunctionOptions = RegisterFunctionOptions

// Observed is what an observer's target receives with [DeliverEnvelope] (the default)
// when iii delivers. Fields not in the observer's capture are absent.
// Decode a target's payload into it:
//
//	var ev iii.Observed
//	_ = json.Unmarshal(data, &ev)
type Observed struct {
	ObserverID   string          `json:"observer_id"`
	ActivationID string          `json:"activation_id"`
	FunctionID   string          `json:"function_id"`
	Namespace    string          `json:"namespace"`
	InvocationID string          `json:"invocation_id"`
	Status       string          `json:"status"`
	Input        json.RawMessage `json:"input,omitempty"`
	Output       json.RawMessage `json:"output,omitempty"`
	Error        *ErrorBody      `json:"error,omitempty"`
	Caller       *Caller         `json:"caller,omitempty"`
	Metadata     json.RawMessage `json:"metadata,omitempty"`
	StartedAtMs  int64           `json:"started_at_ms"`
	DurationMs   float64         `json:"duration_ms"`
}

// Invocation is the invocation a handler runs in, remote or local
// ([InvocationFromContext]).
type Invocation struct {
	FunctionID   string
	InvocationID string
	// Caller is who made the call; nil when the engine sent none (before 0.25).
	Caller *Caller
	// Activation is set when this invocation is the activation of an observer's
	// target.
	Activation *Activation
	// ObserveDepth is the depth of the causal chain in observe hops.
	ObserveDepth uint32
}

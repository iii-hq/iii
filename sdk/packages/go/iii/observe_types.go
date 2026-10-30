package iii

import (
	"context"
	"encoding/json"
	"fmt"
	"time"
)

// This file holds the public types of observe (tech-specs/2026-10-13-observe/sdk.md):
// the observing side ([Source], [ObserverOptions], [ObserverHandle]) and the owner side
// ([ObserveHooks], [Observer]). The wire shapes are in protocol.go.

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

// On selects which invocations of an auto function reach an observer. The default is
// [OnSuccess].
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

// Delivery is how a function's observers are delivered.
type Delivery string

const (
	// DeliveryAuto (the default): every invocation is offered to every observer.
	DeliveryAuto Delivery = "auto"
	// DeliveryOwner: the function's handler hands each event to the observers it
	// belongs to, with [Observer.Emit] or [Observer.Call].
	DeliveryOwner Delivery = "owner"
)

// ObserverState is an observer's lifecycle: pending (no owner yet, waiting for the
// owner's ack, or no target yet) -> active or error; suspended while its worker or
// target is away; disposed at the end.
type ObserverState string

const (
	ObserverPending   ObserverState = "pending"
	ObserverActive    ObserverState = "active"
	ObserverSuspended ObserverState = "suspended"
	ObserverError     ObserverState = "error"
	ObserverDisposed  ObserverState = "disposed"
)

// Dispatch selects how [Client.Trigger] reaches a function this worker hosts.
type Dispatch string

const (
	// DispatchAuto (the default, also the zero value) runs a function this worker
	// hosts in-process once the engine confirmed it for local dispatch.
	DispatchAuto Dispatch = "auto"
	// DispatchRemote always goes through the engine.
	DispatchRemote Dispatch = "remote"
)

// Args are an observer's arguments, for the observed function's owner (owner-delivered
// functions only; validated against its args_format by the engine).
type Args map[string]any

// Filters is a declarative filter over input, output, error, caller and metadata, with
// explicit operators only (the language of the filter subpackage):
//
//	iii.Filters{"input.amount": iii.Op{"$gte": 500}}
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
	// Args go to the owner of an owner-delivered function.
	Args Args
	// Filters decide which events reach the target; nil matches every event.
	Filters Filters
	// On is for auto functions only; empty means [OnSuccess].
	On On
	// Capture is for auto functions and obs.Emit. nil means the default (input,
	// output, caller); an empty, non-nil slice captures nothing.
	Capture []Capture
	// Deliver is for auto functions and obs.Emit; empty means [DeliverEnvelope].
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
// [ObserveHooks.OnObserverAdded] hook returns (built with [ObserveRejection]) to reject
// an observer with a code of its own. Recover it with errors.As.
type ObserveRejectionError struct {
	Code    string
	Message string
}

func (e *ObserveRejectionError) Error() string {
	return fmt.Sprintf("iii: observe rejected: %s: %s", e.Code, e.Message)
}

// ObserveRejection builds the error an OnObserverAdded hook returns to reject an
// observer with code (any other error rejects it with OBSERVE_REJECTED).
func ObserveRejection(code, message string) error {
	return &ObserveRejectionError{Code: code, Message: message}
}

// ObserveHooks is the Observe option of [RegisterFunctionOptions]: how the function's
// observers are delivered, and the callbacks that run when one is added or removed.
// Only Delivery, ArgsFormat, LegacyTriggerType and Allow travel to the engine.
//
// The callbacks of one function run one at a time, in revision order. A callback still
// running after the 30 s timeout no longer holds the queue (its observer is rejected
// with OWNER_TIMEOUT and its ctx cancelled), so a hung callback cannot stall the
// function forever: if it ignores ctx, it may overlap with the next callbacks.
type ObserveHooks struct {
	// Delivery is [DeliveryAuto] (empty) or [DeliveryOwner].
	Delivery Delivery
	// ArgsFormat is the JSON Schema of the observers' args (owner delivery only: the
	// engine answers INVALID_ARGS). With auto delivery, RegisterFunction fails with
	// INVALID_OBSERVE_DESCRIPTOR.
	ArgsFormat json.RawMessage
	// LegacyTriggerType accepts old RegisterTrigger calls of this trigger type
	// through the engine's legacy shim.
	LegacyTriggerType string
	// Allow false refuses every observer (NOT_OBSERVABLE); nil means true.
	Allow *bool
	// OnObserverAdded runs once per added observer, before the SDK acks it, one at a
	// time per function. Returning an error rejects the observer (an error built with
	// [ObserveRejection] sets the code, any other error is OBSERVE_REJECTED); after
	// 30 s it is rejected with OWNER_TIMEOUT and ctx is cancelled.
	OnObserverAdded func(ctx context.Context, obs *Observer) error
	// OnObserverRemoved runs once per removed observer (unobserve, dispose, the
	// observing worker gone for good, a reconnect diff). It cannot fail.
	OnObserverRemoved func(ctx context.Context, obs *Observer)
}

// FunctionOptions is another name for [RegisterFunctionOptions], the one the cross-SDK
// specification uses.
type FunctionOptions = RegisterFunctionOptions

// ObserveDoc is the document an observer's filters read ([Observer.Matches]). A nil
// field is absent; use json.RawMessage(`null`) for a JSON null.
type ObserveDoc struct {
	Input    any
	Output   any
	Error    any
	Caller   *Caller
	Metadata any
}

// Observed is what the target of an auto observer, or of obs.Emit, receives with
// [DeliverEnvelope] (the default). Fields not in the observer's capture are absent.
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

// This file is the wire protocol: the message envelope and every message variant,
// ported from the engine's source of truth at engine/src/protocol.rs. The package
// overview lives in doc.go. The Rust engine declares its Message enum as:
//
//	#[serde(tag = "type", rename_all = "lowercase")]
//
// meaning every frame is a single JSON object whose lowercase "type" field selects
// the variant ("registerfunction", "invokefunction", ...). Go has no tagged unions,
// so we model each variant as its own struct and do the tag dispatch by hand in
// Marshal/Unmarshal below.
//
// Wire-fidelity rules that the engine's deserializer depends on (get these wrong and
// you change semantics, not just formatting):
//
//   - Absent is not null. The engine uses serde's skip_serializing_if = "Option::is_none",
//     so optional fields must be OMITTED when empty, never sent as "field":null. The most
//     load-bearing case is InvokeFunction.invocation_id: absent means fire-and-forget (no
//     reply expected). We model optional fields as pointers with ",omitempty".
//   - Trigger messages use the wire field "trigger_type", not "type" (see RegisterTrigger).
//   - InvocationResult.invocation_id is REQUIRED (a result with no id has nowhere to route),
//     while InvokeFunction.invocation_id is optional.
package iii

import (
	"encoding/json"
	"fmt"

	"github.com/google/uuid"
)

// MessageType is the value of the envelope "type" field. Values are lowercase to
// match the engine's serde rename_all = "lowercase" (engine/src/protocol.rs:41).
type MessageType string

const (
	MsgRegisterTriggerType       MessageType = "registertriggertype"
	MsgRegisterTrigger           MessageType = "registertrigger"
	MsgTriggerRegistrationResult MessageType = "triggerregistrationresult"
	MsgUnregisterTrigger         MessageType = "unregistertrigger"
	MsgRegisterFunction          MessageType = "registerfunction"
	MsgUnregisterFunction        MessageType = "unregisterfunction"
	MsgInvokeFunction            MessageType = "invokefunction"
	MsgInvocationResult          MessageType = "invocationresult"
	MsgRegisterService           MessageType = "registerservice"
	MsgPing                      MessageType = "ping"
	MsgPong                      MessageType = "pong"
	MsgWorkerRegistered          MessageType = "workerregistered"
	MsgReattach                  MessageType = "reattach"
	MsgRegistrationRejected      MessageType = "registrationrejected"
	MsgUnregisterTriggerType     MessageType = "unregistertriggertype"

	// observe (tech-specs/2026-10-13-observe/protocol.md §2). The engine pushes the
	// engine -> worker variants only to connections that declared observe.v1.
	MsgObserve              MessageType = "observe"
	MsgUnobserve            MessageType = "unobserve"
	MsgObserveResult        MessageType = "observeresult"
	MsgObserverStateChanged MessageType = "observerstatechanged"
	MsgFunctionRegistered   MessageType = "functionregistered"
	MsgObserverAdded        MessageType = "observeradded"
	MsgObserverRemoved      MessageType = "observerremoved"
	MsgObserverAck          MessageType = "observerack"
	MsgInvocationsObserved  MessageType = "invocationsobserved"
	MsgLocalInvocationStats MessageType = "localinvocationstats"
)

// ErrorBody is the wire representation of a remote error
// (engine/src/protocol.rs:174-180). It is intentionally minimal; the ergonomic,
// errors.Is-able client error that wraps it lives in errors.go.
type ErrorBody struct {
	Code       string  `json:"code"`
	Message    string  `json:"message"`
	Stacktrace *string `json:"stacktrace,omitempty"`
}

func (e *ErrorBody) Error() string {
	return fmt.Sprintf("%s: %s", e.Code, e.Message)
}

// TriggerAction mirrors the engine's TriggerAction enum (engine/src/protocol.rs:33-38),
// itself a "type"-tagged lowercase union: {"type":"void"} or {"type":"enqueue","queue":"..."}.
// It rides on InvokeFunction.Action and selects the delivery semantics of a Trigger call.
type TriggerAction struct {
	// Type is "void" (fire-and-forget) or "enqueue" (route via a named queue).
	Type string `json:"type"`
	// Queue is set only when Type == "enqueue".
	Queue string `json:"queue,omitempty"`
}

// VoidAction returns the fire-and-forget action. A trigger with this action carries
// no invocation_id, so the engine sends no reply (see Trigger in client.go).
func VoidAction() *TriggerAction { return &TriggerAction{Type: "void"} }

// EnqueueAction routes an invocation through a named queue; the engine acknowledges
// with an enqueue receipt rather than the function's result.
func EnqueueAction(queue string) *TriggerAction {
	return &TriggerAction{Type: "enqueue", Queue: queue}
}

// HTTPInvocationRef describes an externally-hosted function the engine can call
// directly over HTTP (engine/src/protocol.rs:14-31). Workers that only register
// in-process handlers never set this; it is here for wire completeness so
// RegisterFunction round-trips byte-for-byte. Auth is left as raw JSON because the
// engine's HttpAuthConfig is itself a tagged union the v1 SDK has no reason to model.
type HTTPInvocationRef struct {
	URL       string            `json:"url"`
	Method    string            `json:"method,omitempty"`
	TimeoutMs *uint64           `json:"timeout_ms,omitempty"`
	Headers   map[string]string `json:"headers,omitempty"`
	Auth      json.RawMessage   `json:"auth,omitempty"`
}

// ChannelDirection is the end of a streaming channel a StreamChannelRef refers to.
// The engine serializes it lowercase (engine/src/protocol.rs:198-201).
type ChannelDirection string

const (
	ChannelRead  ChannelDirection = "read"
	ChannelWrite ChannelDirection = "write"
)

// StreamChannelRef is a capability handle to one end of a streaming data channel
// (engine/src/protocol.rs:203-211). The engine's engine::channels::create returns a
// writer ref and a reader ref that share a channel_id and access_key; a ref travels as
// plain JSON inside a trigger payload or result, and the holder opens its own WebSocket
// to the channel with it (see channels.go).
type StreamChannelRef struct {
	ChannelID string           `json:"channel_id"`
	AccessKey string           `json:"access_key"`
	Direction ChannelDirection `json:"direction"`
}

// The message structs below each correspond to one variant of the engine's Message
// enum (engine/src/protocol.rs:42-126). They deliberately do NOT carry the "type"
// field themselves: it is injected on marshal and stripped on unmarshal by the
// envelope functions, so a struct value never disagrees with its tag.

// RegisterTriggerType registers a trigger *template* (e.g. "http", "cron"). The two
// *_format fields are optional JSON Schemas describing the trigger config and the
// resulting function-call payload (engine/src/protocol.rs:43-50).
type RegisterTriggerTypeMessage struct {
	ID                   string          `json:"id"`
	Description          string          `json:"description"`
	TriggerRequestFormat json.RawMessage `json:"trigger_request_format,omitempty"`
	CallRequestFormat    json.RawMessage `json:"call_request_format,omitempty"`
}

// RegisterTrigger registers a trigger *instance* that fires FunctionID when its
// trigger type matches. Note the wire field "trigger_type" (engine/src/protocol.rs:53):
// the engine names it that on the wire, so we do too — we do not inherit the Node SDK's
// internal "type"/toWireFormat rename.
type RegisterTriggerMessage struct {
	ID          string          `json:"id"`
	TriggerType string          `json:"trigger_type"`
	FunctionID  string          `json:"function_id"`
	Config      json.RawMessage `json:"config"`
	Metadata    json.RawMessage `json:"metadata,omitempty"`
	// Namespace the trigger's target FunctionID resolves in. Absent means the
	// engine's default namespace, independent of the connection's namespace.
	Namespace string `json:"namespace,omitempty"`
}

// TriggerRegistrationResult is the engine's ack for a RegisterTrigger; Error is set
// only on failure (engine/src/protocol.rs:59-65).
type TriggerRegistrationResultMessage struct {
	ID          string     `json:"id"`
	TriggerType string     `json:"trigger_type"`
	FunctionID  string     `json:"function_id"`
	Error       *ErrorBody `json:"error,omitempty"`
}

// UnregisterTrigger removes a trigger instance. TriggerType is optional here
// (engine/src/protocol.rs:66-70, serde default), so it is a pointer.
type UnregisterTriggerMessage struct {
	ID          string  `json:"id"`
	TriggerType *string `json:"trigger_type,omitempty"`
}

// RegisterFunction announces a function this worker can handle. RequestFormat and
// ResponseFormat are optional JSON Schemas; Invocation is set only for externally
// HTTP-hosted functions (engine/src/protocol.rs).
//
// Optional fields are omitted when unset, as the engine writes them
// (skip_serializing_if); the engine reads an absent format and a JSON null alike.
// Observe is the function's observe descriptor (protocol.md §2.2): how its observers
// are delivered. Absent means delivery auto, observable, no args.
type RegisterFunctionMessage struct {
	ID             string             `json:"id"`
	Description    *string            `json:"description,omitempty"`
	RequestFormat  json.RawMessage    `json:"request_format,omitempty"`
	ResponseFormat json.RawMessage    `json:"response_format,omitempty"`
	Metadata       json.RawMessage    `json:"metadata,omitempty"`
	Invocation     *HTTPInvocationRef `json:"invocation,omitempty"`
	Observe        *ObserveDescriptor `json:"observe,omitempty"`
}

// ObserveDescriptor is RegisterFunction.observe: the four fields of
// [ObserveHooks] that travel. The callbacks stay in the SDK.
type ObserveDescriptor struct {
	// Delivery is "auto" (absent) or "owner".
	Delivery Delivery `json:"delivery,omitempty"`
	// ArgsFormat is the JSON Schema of the observers' args (owner delivery only).
	ArgsFormat json.RawMessage `json:"args_format,omitempty"`
	// LegacyTriggerType accepts old RegisterTrigger calls of this trigger type.
	LegacyTriggerType string `json:"legacy_trigger_type,omitempty"`
	// Allow false refuses every observer (NOT_OBSERVABLE); absent means true.
	Allow *bool `json:"allow,omitempty"`
}

// UnregisterFunction removes a previously registered function (engine/src/protocol.rs:82-84).
type UnregisterFunctionMessage struct {
	ID string `json:"id"`
}

// InvokeFunction is the engine->worker (and worker->engine, via Trigger) call.
//
// InvocationID is a POINTER and omitempty: when absent, the call is fire-and-forget
// and no InvocationResult is expected (engine/src/protocol.rs:86). Traceparent/Baggage
// carry W3C trace context across the hop. Action selects void/enqueue semantics.
//
// Metadata is OPTIONAL per-invocation metadata (arbitrary JSON) that travels as its own
// channel alongside Data, NOT folded into the payload. Like the engine's
// skip_serializing_if optional fields it is omitted when nil; an inbound frame without a
// metadata key decodes to nil ("no metadata"), so older engines that never send it stay
// backward compatible. Distinct from RegisterFunction/RegisterTrigger's registration-level
// Metadata: this one is scoped to a single call.
type InvokeFunctionMessage struct {
	InvocationID *uuid.UUID      `json:"invocation_id,omitempty"`
	FunctionID   string          `json:"function_id"`
	Data         json.RawMessage `json:"data"`
	Metadata     json.RawMessage `json:"metadata,omitempty"`
	Traceparent  *string         `json:"traceparent,omitempty"`
	Baggage      *string         `json:"baggage,omitempty"`
	Action       *TriggerAction  `json:"action,omitempty"`
	// Namespace routes this invocation to a specific namespace. Absent means the
	// engine's default namespace, independent of the caller's own namespace.
	Namespace string `json:"namespace,omitempty"`
	// Caller is who made the call (protocol.md §2.4). Worker -> engine: the handler the
	// call was made from and its own caller; engine -> worker: also the calling worker,
	// stamped by the engine. Sent and received only with observe.v1.
	Caller *Caller `json:"caller,omitempty"`
	// ObserveDepth is the depth of the causal chain in observe hops; 0 is omitted.
	ObserveDepth uint32 `json:"observe_depth,omitempty"`
	// Activation marks an observer activation (engine -> worker) or an owner delivery,
	// obs.Call (worker -> engine).
	Activation *Activation `json:"activation,omitempty"`
}

// UnmarshalJSON decodes an InvokeFunction, reading the observe fields (caller,
// observe_depth, activation) leniently: a malformed value is treated as absent rather
// than failing the frame, so the invocation is still answered.
func (m *InvokeFunctionMessage) UnmarshalJSON(data []byte) error {
	type plain InvokeFunctionMessage
	var aux struct {
		*plain
		Caller       json.RawMessage `json:"caller"`
		ObserveDepth json.RawMessage `json:"observe_depth"`
		Activation   json.RawMessage `json:"activation"`
	}
	aux.plain = (*plain)(m)
	if err := json.Unmarshal(data, &aux); err != nil {
		return err
	}
	m.Caller = decodeCaller(aux.Caller, 0)
	m.ObserveDepth = 0
	if len(aux.ObserveDepth) > 0 {
		var depth uint32
		if json.Unmarshal(aux.ObserveDepth, &depth) == nil {
			m.ObserveDepth = depth
		}
	}
	m.Activation = nil
	if len(aux.Activation) > 0 {
		var fields map[string]json.RawMessage
		if json.Unmarshal(aux.Activation, &fields) == nil {
			if id, ok := jsonString(fields["observer_id"]); ok {
				m.Activation = &Activation{ObserverID: id}
				m.Activation.ActivationID, _ = jsonString(fields["activation_id"])
			}
		}
	}
	return nil
}

// Caller is who started an invocation: the worker (WorkerID / WorkerName, stamped by
// the engine and never forgeable), and the function and invocation the call was made
// from. Parent is the caller of that handler, one level only. Every field may be
// empty: a top-level call has no function.
type Caller struct {
	WorkerID     string  `json:"worker_id,omitempty"`
	WorkerName   string  `json:"worker_name,omitempty"`
	FunctionID   string  `json:"function_id,omitempty"`
	InvocationID string  `json:"invocation_id,omitempty"`
	Parent       *Caller `json:"parent,omitempty"`
}

// Activation marks the invocation of an observer's target.
type Activation struct {
	ObserverID   string `json:"observer_id"`
	ActivationID string `json:"activation_id,omitempty"`
}

// decodeCaller copies a wire caller, dropping values that are not strings and a
// parent inside the parent (one level only). A value that is not an object is absent.
func decodeCaller(raw json.RawMessage, depth int) *Caller {
	if len(raw) == 0 {
		return nil
	}
	var fields map[string]json.RawMessage
	if json.Unmarshal(raw, &fields) != nil || fields == nil {
		return nil
	}
	c := &Caller{}
	c.WorkerID, _ = jsonString(fields["worker_id"])
	c.WorkerName, _ = jsonString(fields["worker_name"])
	c.FunctionID, _ = jsonString(fields["function_id"])
	c.InvocationID, _ = jsonString(fields["invocation_id"])
	if depth == 0 {
		c.Parent = decodeCaller(fields["parent"], 1)
	}
	return c
}

func jsonString(raw json.RawMessage) (string, bool) {
	if len(raw) == 0 {
		return "", false
	}
	var s string
	if json.Unmarshal(raw, &s) != nil {
		return "", false
	}
	return s, true
}

// RegistrationRejectedMessage is the engine's refusal of a registration.
// A FUNCTION_NAMESPACE_CONFLICT costs one function and leaves the connection
// open; a WORKER_NAMESPACE_CONFLICT is terminal — another live worker owns this
// name in the namespace (engine/src/protocol.rs).
type RegistrationRejectedMessage struct {
	Code          string `json:"code"`
	Namespace     string `json:"namespace"`
	WorkerName    string `json:"worker_name,omitempty"`
	FunctionID    string `json:"function_id,omitempty"`
	OwnerWorkerID string `json:"owner_worker_id"`
}

// InvocationResult is the worker's reply to a non-fire-and-forget InvokeFunction.
// InvocationID is REQUIRED (a value, not a pointer): a result with no id cannot be
// routed back to its caller (engine/src/protocol.rs:99). Exactly one of Result/Error
// is set.
type InvocationResultMessage struct {
	InvocationID uuid.UUID       `json:"invocation_id"`
	FunctionID   string          `json:"function_id"`
	Result       json.RawMessage `json:"result,omitempty"`
	Error        *ErrorBody      `json:"error,omitempty"`
	Traceparent  *string         `json:"traceparent,omitempty"`
	Baggage      *string         `json:"baggage,omitempty"`
}

// RegisterService groups functions under a service for the dashboard
// (engine/src/protocol.rs:112-120).
type RegisterServiceMessage struct {
	ID              string  `json:"id"`
	Name            string  `json:"name"`
	Description     *string `json:"description,omitempty"`
	ParentServiceID *string `json:"parent_service_id,omitempty"`
}

// WorkerRegistered is sent engine->worker on connect to announce the worker's id
// (engine/src/protocol.rs:123-125). The worker treats it as informational.
type WorkerRegisteredMessage struct {
	WorkerID string `json:"worker_id"`
	// ReattachToken is the secret to present in a ReattachMessage on reconnect;
	// empty on older engines.
	ReattachToken string `json:"reattach_token,omitempty"`
	// Features the engine supports, e.g. "observe.v1" and "local-dispatch.v1". Absent
	// on engines before 0.25: observe is then unsupported.
	Features []string `json:"features,omitempty"`
}

// Reattach is sent worker->engine as the first message of a reconnect, before the
// registration replay: PreviousWorkerID is the id the engine assigned via
// WorkerRegistered on the previous connection. The engine retires that connection so
// the replay lands on a clean slate instead of racing its cleanup.
type ReattachMessage struct {
	PreviousWorkerID string `json:"previous_worker_id"`
	// ReattachToken authorizes retiring the previous connection: worker ids are
	// publicly listable, the token was only ever sent over that connection's own
	// socket.
	ReattachToken string `json:"reattach_token,omitempty"`
}

// PingMessage is the payloadless keepalive the engine sends; it serializes to
// {"type":"ping"} (engine/src/protocol.rs:121-122). The client replies with a PongMessage.
type PingMessage struct{}

// PongMessage is the payloadless reply to a PingMessage; it serializes to
// {"type":"pong"} (engine/src/protocol.rs:121-122).
type PongMessage struct{}

// UnregisterTriggerTypeMessage retires a trigger type this connection provides. The
// engine removes it only when the sending connection is its provider. Namespace absent
// means the sending connection's namespace.
type UnregisterTriggerTypeMessage struct {
	ID        string `json:"id"`
	Namespace string `json:"namespace,omitempty"`
}

// --- observe (protocol.md §2.2, §2.3; golden JSON in sdk/fixtures/protocol/observe) ---

// ObserveMessage creates, or idempotently replays, an observer owned by this
// connection: Source is the left side of [Client.Observe], Target the right side.
type ObserveMessage struct {
	ID          string               `json:"id"`
	Source      ObserveMessageSource `json:"source"`
	Target      ObserveMessageTarget `json:"target"`
	Metadata    json.RawMessage      `json:"metadata,omitempty"`
	MaxFires    *uint64              `json:"max_fires,omitempty"`
	ExpiresInMs *uint64              `json:"expires_in_ms,omitempty"`
	MaxInFlight *uint64              `json:"max_in_flight,omitempty"`
}

// ObserveMessageSource is what an observer observes. Capture is omitted when nil and
// sent when empty (capture: [] is valid: ids, status and timings only).
type ObserveMessageSource struct {
	FunctionID string          `json:"function_id"`
	Namespace  string          `json:"namespace,omitempty"`
	Args       json.RawMessage `json:"args,omitempty"`
	Filters    json.RawMessage `json:"filters,omitempty"`
	Capture    []Capture       `json:"capture,omitzero"`
	On         On              `json:"on,omitempty"`
	Deliver    Deliver         `json:"deliver,omitempty"`
}

// ObserveMessageTarget is the function an observer invokes.
type ObserveMessageTarget struct {
	FunctionID string `json:"function_id"`
	Namespace  string `json:"namespace,omitempty"`
}

// UnobserveMessage removes an observer this connection owns.
type UnobserveMessage struct {
	ID string `json:"id"`
}

// ObserverAckMessage is the owner's answer to one added observer, sent after its
// OnObserverAdded hook returned. Error rejects it; an owner may re-send it with an
// error for an active observer (a late rejection).
type ObserverAckMessage struct {
	FunctionID string     `json:"function_id"`
	Namespace  string     `json:"namespace,omitempty"`
	ID         string     `json:"id"`
	Revision   uint64     `json:"revision"`
	Error      *ErrorBody `json:"error,omitempty"`
}

// InvocationsObservedMessage carries matched local invocations of auto functions and
// obs.Emit events of owner functions, batched.
type InvocationsObservedMessage struct {
	Items []ObservedItem `json:"items"`
}

// ObservedItem is one reported invocation or event. Input, Output, Error and Metadata
// keep JSON null apart from absent: "output": null is a function that returned null,
// a nil Output was not captured.
type ObservedItem struct {
	FunctionID   string          `json:"function_id"`
	Namespace    string          `json:"namespace,omitempty"`
	InvocationID string          `json:"invocation_id"`
	Matched      []string        `json:"matched"`
	Status       string          `json:"status"`
	Input        json.RawMessage `json:"input,omitempty"`
	Output       json.RawMessage `json:"output,omitempty"`
	Error        json.RawMessage `json:"error,omitempty"`
	Caller       *Caller         `json:"caller,omitempty"`
	Metadata     json.RawMessage `json:"metadata,omitempty"`
	StartedAtMs  int64           `json:"started_at_ms"`
	DurationMs   float64         `json:"duration_ms"`
	ObserveDepth uint32          `json:"observe_depth"`
	Traceparent  *string         `json:"traceparent,omitempty"`
	Baggage      *string         `json:"baggage,omitempty"`
}

// LocalInvocationStatsMessage reports local invocations and SDK-side report drops,
// periodically and only when non-zero.
type LocalInvocationStatsMessage struct {
	WindowMs       uint64               `json:"window_ms"`
	Functions      []LocalFunctionStats `json:"functions,omitempty"`
	DroppedReports []DroppedReports     `json:"dropped_reports,omitempty"`
}

// LocalFunctionStats counts one function's local invocations in a stats window.
type LocalFunctionStats struct {
	FunctionID    string  `json:"function_id"`
	Count         uint64  `json:"count"`
	Errors        uint64  `json:"errors"`
	DurationMsSum float64 `json:"duration_ms_sum"`
}

// DroppedReports counts report items the SDK dropped (BUFFER_FULL, TOO_LARGE).
type DroppedReports struct {
	FunctionID string `json:"function_id"`
	Reason     string `json:"reason"`
	Count      uint64 `json:"count"`
}

// FunctionRegisteredMessage is the RegisterFunction success ack, with the full current
// observer set. Always accepted: it replaces the set the SDK holds.
type FunctionRegisteredMessage struct {
	ID            string         `json:"id"`
	Namespace     string         `json:"namespace"`
	Revision      uint64         `json:"revision"`
	LocalDispatch bool           `json:"local_dispatch"`
	Observers     []ObserverInfo `json:"observers"`
}

// ObserverAddedMessage attaches one observer to a function this connection hosts.
type ObserverAddedMessage struct {
	FunctionID string       `json:"function_id"`
	Namespace  string       `json:"namespace"`
	Revision   uint64       `json:"revision"`
	Observer   ObserverInfo `json:"observer"`
}

// ObserverRemovedMessage detaches one observer.
type ObserverRemovedMessage struct {
	FunctionID string `json:"function_id"`
	Namespace  string `json:"namespace"`
	Revision   uint64 `json:"revision"`
	ID         string `json:"id"`
}

// ObserverInfo is an observer as the owner sees it: never its metadata, policy or the
// observing worker. Filters are in normal form.
type ObserverInfo struct {
	ID      string          `json:"id"`
	Args    json.RawMessage `json:"args,omitempty"`
	Filters json.RawMessage `json:"filters,omitempty"`
	Target  string          `json:"target"`
	Capture []Capture       `json:"capture,omitzero"`
	On      On              `json:"on,omitempty"`
	Legacy  *LegacyTrigger  `json:"legacy,omitempty"`
}

// LegacyTrigger is the old trigger an observer was created from by the engine's legacy
// shim (protocol.md §6): the owner reads Config as the observer's args.
type LegacyTrigger struct {
	TriggerType string          `json:"trigger_type"`
	Config      json.RawMessage `json:"config"`
}

// ObserveResultMessage is the engine's reply to an observe (also on a replay).
type ObserveResultMessage struct {
	ID       string     `json:"id"`
	State    string     `json:"state"`
	Error    *ErrorBody `json:"error,omitempty"`
	Warnings []string   `json:"warnings,omitempty"`
}

// ObserverStateChangedMessage is a lifecycle transition of an observer this
// connection owns.
type ObserverStateChangedMessage struct {
	ID     string     `json:"id"`
	State  string     `json:"state"`
	Reason *ErrorBody `json:"reason,omitempty"`
	Fires  uint64     `json:"fires"`
}

// marshalEnvelope serializes a variant struct into a single tagged JSON object by
// merging {"type": msgType} with the struct's own fields. This is how we reproduce
// the engine's #[serde(tag = "type")] from Go: there is no separate "type" field on
// the structs, so the value can never disagree with the tag.
func marshalEnvelope(msgType MessageType, payload any) ([]byte, error) {
	// Marshal the payload, then splice the type field in. We go through a generic
	// map so field order is irrelevant (JSON objects are unordered) and so any
	// json.RawMessage / omitempty behavior on the payload is preserved exactly.
	body, err := json.Marshal(payload)
	if err != nil {
		return nil, err
	}
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(body, &fields); err != nil {
		return nil, err
	}
	if fields == nil {
		fields = map[string]json.RawMessage{}
	}
	typeBytes, _ := json.Marshal(msgType)
	fields["type"] = typeBytes
	return json.Marshal(fields)
}

// MarshalMessage encodes a variant struct as a wire frame, injecting the "type" tag.
// Pass one of the *Message structs (by pointer or value) from this file.
func MarshalMessage(msg any) ([]byte, error) {
	switch m := msg.(type) {
	case *RegisterTriggerTypeMessage:
		return marshalEnvelope(MsgRegisterTriggerType, m)
	case *RegisterTriggerMessage:
		return marshalEnvelope(MsgRegisterTrigger, m)
	case *TriggerRegistrationResultMessage:
		return marshalEnvelope(MsgTriggerRegistrationResult, m)
	case *UnregisterTriggerMessage:
		return marshalEnvelope(MsgUnregisterTrigger, m)
	case *RegisterFunctionMessage:
		return marshalEnvelope(MsgRegisterFunction, m)
	case *UnregisterFunctionMessage:
		return marshalEnvelope(MsgUnregisterFunction, m)
	case *InvokeFunctionMessage:
		return marshalEnvelope(MsgInvokeFunction, m)
	case *InvocationResultMessage:
		return marshalEnvelope(MsgInvocationResult, m)
	case *RegisterServiceMessage:
		return marshalEnvelope(MsgRegisterService, m)
	case *WorkerRegisteredMessage:
		return marshalEnvelope(MsgWorkerRegistered, m)
	case *RegistrationRejectedMessage:
		return marshalEnvelope(MsgRegistrationRejected, m)
	case *ReattachMessage:
		return marshalEnvelope(MsgReattach, m)
	case *PingMessage:
		return marshalEnvelope(MsgPing, struct{}{})
	case *PongMessage:
		return marshalEnvelope(MsgPong, struct{}{})
	case *UnregisterTriggerTypeMessage:
		return marshalEnvelope(MsgUnregisterTriggerType, m)
	case *ObserveMessage:
		return marshalEnvelope(MsgObserve, m)
	case *UnobserveMessage:
		return marshalEnvelope(MsgUnobserve, m)
	case *ObserverAckMessage:
		return marshalEnvelope(MsgObserverAck, m)
	case *InvocationsObservedMessage:
		return marshalEnvelope(MsgInvocationsObserved, m)
	case *LocalInvocationStatsMessage:
		return marshalEnvelope(MsgLocalInvocationStats, m)
	case *FunctionRegisteredMessage:
		return marshalEnvelope(MsgFunctionRegistered, m)
	case *ObserverAddedMessage:
		return marshalEnvelope(MsgObserverAdded, m)
	case *ObserverRemovedMessage:
		return marshalEnvelope(MsgObserverRemoved, m)
	case *ObserveResultMessage:
		return marshalEnvelope(MsgObserveResult, m)
	case *ObserverStateChangedMessage:
		return marshalEnvelope(MsgObserverStateChanged, m)

	// Value forms, so the documented "by pointer or value" contract holds (e.g.
	// MarshalMessage(PingMessage{})). Each delegates to the pointer case above.
	case RegisterTriggerTypeMessage:
		return marshalEnvelope(MsgRegisterTriggerType, &m)
	case RegisterTriggerMessage:
		return marshalEnvelope(MsgRegisterTrigger, &m)
	case TriggerRegistrationResultMessage:
		return marshalEnvelope(MsgTriggerRegistrationResult, &m)
	case UnregisterTriggerMessage:
		return marshalEnvelope(MsgUnregisterTrigger, &m)
	case RegisterFunctionMessage:
		return marshalEnvelope(MsgRegisterFunction, &m)
	case UnregisterFunctionMessage:
		return marshalEnvelope(MsgUnregisterFunction, &m)
	case InvokeFunctionMessage:
		return marshalEnvelope(MsgInvokeFunction, &m)
	case InvocationResultMessage:
		return marshalEnvelope(MsgInvocationResult, &m)
	case RegisterServiceMessage:
		return marshalEnvelope(MsgRegisterService, &m)
	case WorkerRegisteredMessage:
		return marshalEnvelope(MsgWorkerRegistered, &m)
	case RegistrationRejectedMessage:
		return marshalEnvelope(MsgRegistrationRejected, &m)
	case ReattachMessage:
		return marshalEnvelope(MsgReattach, &m)
	case PingMessage:
		return marshalEnvelope(MsgPing, struct{}{})
	case PongMessage:
		return marshalEnvelope(MsgPong, struct{}{})
	case UnregisterTriggerTypeMessage:
		return marshalEnvelope(MsgUnregisterTriggerType, &m)
	case ObserveMessage:
		return marshalEnvelope(MsgObserve, &m)
	case UnobserveMessage:
		return marshalEnvelope(MsgUnobserve, &m)
	case ObserverAckMessage:
		return marshalEnvelope(MsgObserverAck, &m)
	case InvocationsObservedMessage:
		return marshalEnvelope(MsgInvocationsObserved, &m)
	case LocalInvocationStatsMessage:
		return marshalEnvelope(MsgLocalInvocationStats, &m)
	case FunctionRegisteredMessage:
		return marshalEnvelope(MsgFunctionRegistered, &m)
	case ObserverAddedMessage:
		return marshalEnvelope(MsgObserverAdded, &m)
	case ObserverRemovedMessage:
		return marshalEnvelope(MsgObserverRemoved, &m)
	case ObserveResultMessage:
		return marshalEnvelope(MsgObserveResult, &m)
	case ObserverStateChangedMessage:
		return marshalEnvelope(MsgObserverStateChanged, &m)

	default:
		return nil, fmt.Errorf("iii: cannot marshal unknown message type %T", msg)
	}
}

// DecodedMessage is the result of UnmarshalMessage: the decoded "type" plus exactly
// one populated variant pointer (the rest are nil). Callers switch on Type, or simply
// check which pointer is non-nil.
type DecodedMessage struct {
	Type MessageType

	RegisterTriggerType       *RegisterTriggerTypeMessage
	RegisterTrigger           *RegisterTriggerMessage
	TriggerRegistrationResult *TriggerRegistrationResultMessage
	UnregisterTrigger         *UnregisterTriggerMessage
	RegisterFunction          *RegisterFunctionMessage
	UnregisterFunction        *UnregisterFunctionMessage
	InvokeFunction            *InvokeFunctionMessage
	InvocationResult          *InvocationResultMessage
	RegisterService           *RegisterServiceMessage
	WorkerRegistered          *WorkerRegisteredMessage
	RegistrationRejected      *RegistrationRejectedMessage
	Ping                      *PingMessage
	Pong                      *PongMessage

	UnregisterTriggerType *UnregisterTriggerTypeMessage
	Observe               *ObserveMessage
	Unobserve             *UnobserveMessage
	ObserverAck           *ObserverAckMessage
	InvocationsObserved   *InvocationsObservedMessage
	LocalInvocationStats  *LocalInvocationStatsMessage
	FunctionRegistered    *FunctionRegisteredMessage
	ObserverAdded         *ObserverAddedMessage
	ObserverRemoved       *ObserverRemovedMessage
	ObserveResult         *ObserveResultMessage
	ObserverStateChanged  *ObserverStateChangedMessage
}

// UnmarshalMessage decodes a wire frame by peeking at "type" and then decoding into
// the matching variant struct — the Go counterpart of serde's tag dispatch.
func UnmarshalMessage(data []byte) (*DecodedMessage, error) {
	var envelope struct {
		Type MessageType `json:"type"`
	}
	if err := json.Unmarshal(data, &envelope); err != nil {
		return nil, fmt.Errorf("iii: decoding message envelope: %w", err)
	}

	out := &DecodedMessage{Type: envelope.Type}
	switch envelope.Type {
	case MsgRegisterTriggerType:
		out.RegisterTriggerType = &RegisterTriggerTypeMessage{}
		return out, decodeInto(data, out.RegisterTriggerType)
	case MsgRegisterTrigger:
		out.RegisterTrigger = &RegisterTriggerMessage{}
		return out, decodeInto(data, out.RegisterTrigger)
	case MsgTriggerRegistrationResult:
		out.TriggerRegistrationResult = &TriggerRegistrationResultMessage{}
		return out, decodeInto(data, out.TriggerRegistrationResult)
	case MsgUnregisterTrigger:
		out.UnregisterTrigger = &UnregisterTriggerMessage{}
		return out, decodeInto(data, out.UnregisterTrigger)
	case MsgRegisterFunction:
		out.RegisterFunction = &RegisterFunctionMessage{}
		return out, decodeInto(data, out.RegisterFunction)
	case MsgUnregisterFunction:
		out.UnregisterFunction = &UnregisterFunctionMessage{}
		return out, decodeInto(data, out.UnregisterFunction)
	case MsgInvokeFunction:
		out.InvokeFunction = &InvokeFunctionMessage{}
		return out, decodeInto(data, out.InvokeFunction)
	case MsgInvocationResult:
		out.InvocationResult = &InvocationResultMessage{}
		return out, decodeInto(data, out.InvocationResult)
	case MsgRegisterService:
		out.RegisterService = &RegisterServiceMessage{}
		return out, decodeInto(data, out.RegisterService)
	case MsgWorkerRegistered:
		out.WorkerRegistered = &WorkerRegisteredMessage{}
		return out, decodeInto(data, out.WorkerRegistered)
	case MsgRegistrationRejected:
		out.RegistrationRejected = &RegistrationRejectedMessage{}
		return out, decodeInto(data, out.RegistrationRejected)
	case MsgPing:
		out.Ping = &PingMessage{}
		return out, nil
	case MsgPong:
		out.Pong = &PongMessage{}
		return out, nil
	case MsgUnregisterTriggerType:
		out.UnregisterTriggerType = &UnregisterTriggerTypeMessage{}
		return out, decodeInto(data, out.UnregisterTriggerType)
	case MsgObserve:
		out.Observe = &ObserveMessage{}
		return out, decodeInto(data, out.Observe)
	case MsgUnobserve:
		out.Unobserve = &UnobserveMessage{}
		return out, decodeInto(data, out.Unobserve)
	case MsgObserverAck:
		out.ObserverAck = &ObserverAckMessage{}
		return out, decodeInto(data, out.ObserverAck)
	case MsgInvocationsObserved:
		out.InvocationsObserved = &InvocationsObservedMessage{}
		return out, decodeInto(data, out.InvocationsObserved)
	case MsgLocalInvocationStats:
		out.LocalInvocationStats = &LocalInvocationStatsMessage{}
		return out, decodeInto(data, out.LocalInvocationStats)
	case MsgFunctionRegistered:
		out.FunctionRegistered = &FunctionRegisteredMessage{}
		return out, decodeInto(data, out.FunctionRegistered)
	case MsgObserverAdded:
		out.ObserverAdded = &ObserverAddedMessage{}
		return out, decodeInto(data, out.ObserverAdded)
	case MsgObserverRemoved:
		out.ObserverRemoved = &ObserverRemovedMessage{}
		return out, decodeInto(data, out.ObserverRemoved)
	case MsgObserveResult:
		out.ObserveResult = &ObserveResultMessage{}
		return out, decodeInto(data, out.ObserveResult)
	case MsgObserverStateChanged:
		out.ObserverStateChanged = &ObserverStateChangedMessage{}
		return out, decodeInto(data, out.ObserverStateChanged)
	default:
		return nil, fmt.Errorf("iii: unknown message type %q", envelope.Type)
	}
}

func decodeInto(data []byte, target any) error {
	if err := json.Unmarshal(data, target); err != nil {
		return fmt.Errorf("iii: decoding message body: %w", err)
	}
	return nil
}

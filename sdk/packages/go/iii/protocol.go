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
// HTTP-hosted functions (engine/src/protocol.rs:71-81).
//
// Note: per the engine, request_format/response_format are serialized even when null
// (they lack skip_serializing_if), so these are RawMessage values that emit JSON null
// when unset — matching the engine byte-for-byte.
type RegisterFunctionMessage struct {
	ID             string             `json:"id"`
	Description    *string            `json:"description,omitempty"`
	RequestFormat  json.RawMessage    `json:"request_format"`
	ResponseFormat json.RawMessage    `json:"response_format"`
	Metadata       json.RawMessage    `json:"metadata,omitempty"`
	Invocation     *HTTPInvocationRef `json:"invocation,omitempty"`
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
type InvokeFunctionMessage struct {
	InvocationID *uuid.UUID      `json:"invocation_id,omitempty"`
	FunctionID   string          `json:"function_id"`
	Data         json.RawMessage `json:"data"`
	Traceparent  *string         `json:"traceparent,omitempty"`
	Baggage      *string         `json:"baggage,omitempty"`
	Action       *TriggerAction  `json:"action,omitempty"`
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
}

// PingMessage is the payloadless keepalive the engine sends; it serializes to
// {"type":"ping"} (engine/src/protocol.rs:121-122). The client replies with a PongMessage.
type PingMessage struct{}

// PongMessage is the payloadless reply to a PingMessage; it serializes to
// {"type":"pong"} (engine/src/protocol.rs:121-122).
type PongMessage struct{}

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
	case *PingMessage:
		return marshalEnvelope(MsgPing, struct{}{})
	case *PongMessage:
		return marshalEnvelope(MsgPong, struct{}{})

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
	case PingMessage:
		return marshalEnvelope(MsgPing, struct{}{})
	case PongMessage:
		return marshalEnvelope(MsgPong, struct{}{})

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
	Ping                      *PingMessage
	Pong                      *PongMessage
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
	case MsgPing:
		out.Ping = &PingMessage{}
		return out, nil
	case MsgPong:
		out.Pong = &PongMessage{}
		return out, nil
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

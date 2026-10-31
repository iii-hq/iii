package iii

import "encoding/json"

// MiddlewareFunctionInput is the payload the engine sends to the RBAC middleware
// function (the worker-manager rbac.middleware_function_id) on every function
// invocation that arrives through the RBAC port. The middleware can inspect, modify or
// reject the call before it reaches the target function: forward it with
// [Client.Call], or return an error to reject it.
//
// It mirrors MiddlewareFunctionInput in the Node and Rust SDKs field for field; the
// JSON names are the wire contract. Register the
// middleware with [RegisterFunctionTyped] to receive it decoded, or unmarshal the raw
// handler payload into it.
//
// When forwarding, note that an empty Namespace means the engine's default namespace,
// whereas an empty [CallRequest.Namespace] means this worker's own namespace: pass
// "default" explicitly when Namespace is empty and the worker is namespaced.
type MiddlewareFunctionInput struct {
	// FunctionID is the ID of the function being invoked.
	FunctionID string `json:"function_id"`
	// Payload is the payload sent by the caller, verbatim. Forward it as
	// [CallRequest.Payload] or decode it to inspect the call.
	Payload json.RawMessage `json:"payload"`
	// Action is the routing action of the call, if any (nil for a synchronous
	// request/response). Forward it as [CallRequest.Action] to keep enqueue/void
	// semantics.
	Action *CallAction `json:"action,omitempty"`
	// Context is the auth context the RBAC auth function returned for this session
	// (AuthResult.context in package helpers/workerconnection), as raw JSON.
	Context json.RawMessage `json:"context"`
	// Namespace is the target namespace the invocation addressed; forward the call there
	// to stay in the caller's namespace. Empty means the engine's default namespace.
	Namespace string `json:"namespace,omitempty"`
}

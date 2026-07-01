package iii

import (
	"context"
	"encoding/json"
	"fmt"

	"github.com/invopop/jsonschema"
)

// This file provides JSON-Schema inference for registered functions, the Go counterpart
// of the Rust SDK's #[derive(JsonSchema)] + RegisterFunction::new::<T>() and the schemas
// the engine advertises in request_format / response_format.
//
// Go has no compile-time derive macros, so inference is reflection-based (via
// github.com/invopop/jsonschema, the analog of Rust's schemars): given the request and
// response types as generic parameters, the schema is reflected at registration time and
// sent on the registerfunction frame. Add `json` and `jsonschema` struct tags to control
// the generated schema, e.g.:
//
//	type CreateOrderRequest struct {
//	    Item     string `json:"item" jsonschema:"required"`
//	    Quantity int    `json:"quantity" jsonschema:"minimum=1"`
//	}

// TypedHandler is a function handler with a typed request and response. The SDK
// unmarshals the invocation payload into Req before calling it and marshals the returned
// Resp into the result, so handlers work with concrete types instead of json.RawMessage.
type TypedHandler[Req any, Resp any] func(ctx context.Context, req Req) (Resp, error)

// RegisterFunctionTyped registers a function whose request and response schemas are
// inferred from the Req and Resp type parameters and advertised to the engine. It is the
// schema-aware counterpart of [Client.RegisterFunction]; reach for it when you want the
// engine (and its dashboard / typed callers) to know the function's contract.
//
//	iii.RegisterFunctionTyped[CreateOrderRequest, OrderResult](client, "orders::create",
//	    func(ctx context.Context, req CreateOrderRequest) (OrderResult, error) { ... })
//
// Use [Client.RegisterFunction] directly for schemaless functions or when you need to
// hand the engine a hand-written schema. See [InferSchema] to obtain a type's schema on
// its own.
func RegisterFunctionTyped[Req any, Resp any](c *Client, id string, handler TypedHandler[Req, Resp]) error {
	if handler == nil {
		return fmt.Errorf("iii: RegisterFunctionTyped(%q): handler is nil", id)
	}

	reqSchema, err := reflectSchema[Req]()
	if err != nil {
		return fmt.Errorf("iii: RegisterFunctionTyped(%q): request schema: %w", id, err)
	}
	respSchema, err := reflectSchema[Resp]()
	if err != nil {
		return fmt.Errorf("iii: RegisterFunctionTyped(%q): response schema: %w", id, err)
	}

	msg := &RegisterFunctionMessage{
		ID:             id,
		RequestFormat:  reqSchema,
		ResponseFormat: respSchema,
	}

	// Adapt the typed handler to the raw Handler the dispatcher calls. The typed API
	// works in terms of the decoded request only, so the raw per-invocation metadata is
	// accepted (to satisfy the Handler signature) but not surfaced here.
	raw := func(ctx context.Context, data json.RawMessage, _ json.RawMessage) (any, error) {
		var req Req
		if len(data) > 0 {
			if err := json.Unmarshal(data, &req); err != nil {
				return nil, &InvocationError{Code: "invalid_request", Message: err.Error()}
			}
		}
		return handler(ctx, req)
	}

	c.mu.Lock()
	c.functions[id] = registeredFunction{message: msg, handler: raw}
	c.mu.Unlock()
	c.sendRegistration(msg)
	return nil
}

// InferSchema reflects a JSON Schema for T, exposed for callers that want to build a
// registration manually or inspect the schema the typed API would send.
func InferSchema[T any]() (json.RawMessage, error) {
	return reflectSchema[T]()
}

// reflectSchema produces the JSON Schema for T as raw JSON. A reflector configured to
// inline definitions keeps the schema self-contained (no external $ref), matching what
// the engine expects on the wire.
func reflectSchema[T any]() (json.RawMessage, error) {
	reflector := &jsonschema.Reflector{
		// Inline rather than emitting a $defs/$ref graph, so the engine receives a single
		// self-contained schema object.
		DoNotReference: true,
	}
	var zero T
	schema := reflector.Reflect(zero)
	return json.Marshal(schema)
}

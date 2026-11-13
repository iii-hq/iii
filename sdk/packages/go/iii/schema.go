package iii

import (
	"context"
	"encoding/json"
	"fmt"
	"reflect"
	"strconv"
	"strings"

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
//
// Per-invocation metadata, when the call carries any, rides on ctx rather than the
// signature (so adding it is non-breaking): read it with [MetadataFromContext].
type TypedHandler[Req any, Resp any] func(ctx context.Context, req Req) (Resp, error)

// RegisterFunctionTyped registers a function whose request and response schemas are
// inferred from the Req and Resp type parameters and advertised to the engine. It is the
// schema-aware counterpart of [Client.RegisterFunction]; reach for it when you want the
// engine (and its dashboard / typed callers) to know the function's contract.
//
//	iii.RegisterFunctionTyped[CreateOrderRequest, OrderResult](client, "orders::create",
//	    func(ctx context.Context, req CreateOrderRequest) (OrderResult, error) { ... })
//
// It returns the function's handle, as [Client.RegisterFunction] does.
//
// Use [Client.RegisterFunction] directly for schemaless functions. See [InferSchema] to
// obtain a type's schema on its own. Pass a single [RegisterFunctionOptions] value to
// attach a description, registration metadata, args_format, private or observable;
// its RequestFormat and ResponseFormat, when set, replace the inferred schemas
// (that type is then not reflected). Its handle sets the function's
// hooks, as [Client.RegisterFunction]'s does.
func RegisterFunctionTyped[Req any, Resp any](c *Client, id string, handler TypedHandler[Req, Resp], opts ...RegisterFunctionOptions) (*FunctionRef, error) {
	if handler == nil {
		return nil, fmt.Errorf("iii: RegisterFunctionTyped(%q): handler is nil", id)
	}
	cfg, err := resolveRegisterFunctionOptions("RegisterFunctionTyped", id, opts)
	if err != nil {
		return nil, err
	}

	msg, err := registerFunctionMessage("RegisterFunctionTyped", id, cfg)
	if err != nil {
		return nil, err
	}
	// Explicit formats win over the inferred ones.
	if len(msg.RequestFormat) == 0 {
		if msg.RequestFormat, err = reflectSchema[Req](); err != nil {
			return nil, fmt.Errorf("iii: RegisterFunctionTyped(%q): request schema: %w", id, err)
		}
	}
	if len(msg.ResponseFormat) == 0 {
		if msg.ResponseFormat, err = reflectSchema[Resp](); err != nil {
			return nil, fmt.Errorf("iii: RegisterFunctionTyped(%q): response schema: %w", id, err)
		}
	}

	// Adapt the typed handler to the raw Handler the dispatcher calls.
	raw := func(ctx context.Context, data json.RawMessage) (any, error) {
		var req Req
		if len(data) > 0 {
			if err := json.Unmarshal(data, &req); err != nil {
				return nil, &InvocationError{Code: "invalid_request", Message: err.Error()}
			}
		}
		return handler(ctx, req)
	}

	return c.storeFunction("RegisterFunctionTyped", id, msg, raw, cfg)
}

// InferSchema reflects a JSON Schema for T, exposed for callers that want to build a
// registration manually or inspect the schema the typed API would send.
//
// Nested types are written out where they appear. A type that contains itself (a tree
// node with Children []Node, a struct with a pointer to its own type, two structs that
// point at each other) is described once under "$defs" and referenced with
// "$ref": "#/$defs/<Name>". An interface type, such as any, accepts any JSON value. A
// type with no JSON Schema, such as a chan or func field, is an error.
func InferSchema[T any]() (json.RawMessage, error) {
	schema, err := reflectSchema[T]()
	if err != nil {
		return nil, fmt.Errorf("iii: InferSchema: %w", err)
	}
	return schema, nil
}

// reflectSchema produces the JSON Schema for T as raw JSON: one self-contained document
// whose only references are local ("#/$defs/..."), which the engine resolves.
func reflectSchema[T any]() (json.RawMessage, error) {
	// The type comes from a nil *T, not from a zero T: a zero interface value (T = any)
	// has no dynamic type, and reflect.TypeOf would return nil.
	return schemaOf(reflect.TypeOf((*T)(nil)).Elem())
}

// schemaOf is reflectSchema for a reflect.Type.
func schemaOf(t reflect.Type) (schema json.RawMessage, err error) {
	defer func() {
		// The reflector panics on a type it cannot describe (a chan, a func, a complex
		// number); report it rather than crash the worker at registration.
		if r := recover(); r != nil {
			schema, err = nil, fmt.Errorf("no JSON Schema for %s: %v", t, r)
		}
	}()
	return json.Marshal(buildSchema(t))
}

// buildSchema reflects t into one schema document. Every type is inlined where it
// appears, as the reflector does with DoNotReference, except the recursive types:
// inlining one would never end, so each occurrence becomes a "$ref" to the type's entry
// in the root's "$defs". Without recursive types the result is exactly the reflector's
// DoNotReference output. When t itself is recursive, its body is inlined at the root and
// also listed in "$defs" for the inner references, as the Rust SDK's schemars does.
func buildSchema(t reflect.Type) *jsonschema.Schema {
	b := &schemaBuilder{
		recursive: recursiveTypes(t),
		names:     map[reflect.Type]string{},
		taken:     map[string]bool{},
	}
	root := b.reflect(t)
	// Each body can reference recursive types not seen before, which append to pending.
	for i := 0; i < len(b.pending); i++ {
		def := b.pending[i]
		body := b.reflect(def)
		body.Version, body.ID = "", jsonschema.EmptyID
		if root.Definitions == nil {
			root.Definitions = jsonschema.Definitions{}
		}
		root.Definitions[b.names[def]] = body
	}
	return root
}

// schemaBuilder holds the state of one buildSchema call.
type schemaBuilder struct {
	recursive map[reflect.Type]bool
	names     map[reflect.Type]string // the $defs name of each referenced recursive type
	taken     map[string]bool
	pending   []reflect.Type // referenced recursive types, in order of first reference
}

// reflect inlines t, and references every recursive type nested in it.
func (b *schemaBuilder) reflect(t reflect.Type) *jsonschema.Schema {
	outer := true
	reflector := &jsonschema.Reflector{
		// Inline every type, as this SDK always has, so the schema of a type without
		// recursion does not change.
		DoNotReference: true,
		// The reflector asks the Mapper about every type it is about to describe, the
		// outer type first (after pointers and JSONSchemaAlias are followed), then each
		// type nested in it.
		Mapper: func(t reflect.Type) *jsonschema.Schema {
			first := outer
			outer = false
			if first || !b.recursive[t] {
				return nil
			}
			return &jsonschema.Schema{Ref: "#/$defs/" + b.name(t)}
		},
	}
	return reflector.ReflectFromType(t)
}

// name returns the $defs name of the recursive type t: its Go name, with characters
// outside [A-Za-z0-9_.-] (from type arguments) replaced, and a number appended when
// another type already has it.
func (b *schemaBuilder) name(t reflect.Type) string {
	if name, ok := b.names[t]; ok {
		return name
	}
	base := strings.Map(func(r rune) rune {
		if r == '_' || r == '.' || r == '-' || ('0' <= r && r <= '9') || ('a' <= r && r <= 'z') || ('A' <= r && r <= 'Z') {
			return r
		}
		return '_'
	}, t.Name())
	name := base
	for n := 2; b.taken[name]; n++ {
		name = base + strconv.Itoa(n)
	}
	b.names[t] = name
	b.taken[name] = true
	b.pending = append(b.pending, t)
	return name
}

// recursiveTypes returns the types that contain themselves within t. It reflects t once
// with a definition and a reference for every named type (the reflector's default mode,
// which ends on cycles), and a type is recursive when its definition reaches its own
// reference. Using the reflector for this walks the type graph exactly as the inlining
// pass does: the same fields, tags, aliases and custom schemas.
func recursiveTypes(t reflect.Type) map[reflect.Type]bool {
	keys := map[reflect.Type]string{}
	types := map[string]reflect.Type{}
	reflector := &jsonschema.Reflector{
		// A key per type: by default a definition is named by the bare type name, which
		// two packages can share.
		Namer: func(t reflect.Type) string {
			if t.Name() == "" {
				return "" // an unnamed type (a slice, a map, struct{...}) gets no definition
			}
			key, ok := keys[t]
			if !ok {
				key = "t" + strconv.Itoa(len(keys))
				keys[t] = key
				types[key] = t
			}
			return key
		},
	}
	defs := reflector.ReflectFromType(t).Definitions
	refs := make(map[string][]string, len(defs))
	for key, def := range defs {
		refs[key] = localRefs(def)
	}
	recursive := map[reflect.Type]bool{}
	for key := range defs {
		if reaches(refs, key) {
			recursive[types[key]] = true
		}
	}
	return recursive
}

// localRefs lists the definitions a schema references ("$ref": "#/$defs/<key>").
func localRefs(schema *jsonschema.Schema) []string {
	var doc any
	raw, err := json.Marshal(schema)
	if err == nil {
		err = json.Unmarshal(raw, &doc)
	}
	if err != nil {
		// A schema that does not marshal would fail the final json.Marshal too; fail here
		// (schemaOf reports it) rather than miss a reference and recurse without end.
		panic(err)
	}
	var keys []string
	var walk func(v any)
	walk = func(v any) {
		switch v := v.(type) {
		case map[string]any:
			for k, child := range v {
				if ref, ok := child.(string); k == "$ref" && ok {
					if key, ok := strings.CutPrefix(ref, "#/$defs/"); ok {
						keys = append(keys, key)
					}
					continue
				}
				walk(child)
			}
		case []any:
			for _, child := range v {
				walk(child)
			}
		}
	}
	walk(doc)
	return keys
}

// reaches reports whether the definition key leads back to itself through refs.
func reaches(refs map[string][]string, key string) bool {
	seen := map[string]bool{}
	stack := append([]string(nil), refs[key]...)
	for len(stack) > 0 {
		next := stack[len(stack)-1]
		stack = stack[:len(stack)-1]
		if next == key {
			return true
		}
		if !seen[next] {
			seen[next] = true
			stack = append(stack, refs[next]...)
		}
	}
	return false
}

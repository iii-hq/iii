package iii

import (
	"context"
	"encoding/json"
	"fmt"
	"reflect"
	"slices"
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

// The interfaces the reflector checks a type against, for recursiveTypes.
type (
	schemaAliaser         interface{ JSONSchemaAlias() any }
	schemaPropertyAliaser interface{ JSONSchemaProperty(prop string) any }
	schemaProvider        interface{ JSONSchema() *jsonschema.Schema }
	protoEnumer           interface{ EnumDescriptor() ([]byte, []int) }
)

var (
	schemaAliaserType         = reflect.TypeFor[schemaAliaser]()
	schemaPropertyAliaserType = reflect.TypeFor[schemaPropertyAliaser]()
	schemaProviderType        = reflect.TypeFor[schemaProvider]()
	protoEnumerType           = reflect.TypeFor[protoEnumer]()
	rawMessageType            = reflect.TypeFor[json.RawMessage]()
	byteType                  = reflect.TypeFor[byte]()
)

// recursiveTypes returns the types that contain themselves within t. It walks the types
// the reflector (invopop/jsonschema v0.14.0) walks, by the same rules: exported fields,
// json "-" and jsonschema "-" skipped, embedded and `inline` structs flattened into
// their parent, JSONSchemaAlias and JSONSchemaProperty followed, and a type with its own
// JSONSchema method, json.RawMessage and []byte as leaves. It does not reflect, so the
// types' JSONSchemaExtend hooks only run in the inline pass, on the schemas they always
// got. The nodes are the named types the reflector hands to the Mapper (a cycle always
// goes through one); a node is recursive when it reaches itself.
func recursiveTypes(t reflect.Type) map[reflect.Type]bool {
	g := typeGraph{edges: map[reflect.Type][]reflect.Type{}}
	var roots []reflect.Type
	g.collect(t, &roots)
	for _, root := range roots {
		g.visit(root)
	}
	recursive := map[reflect.Type]bool{}
	for node := range g.edges {
		if g.reaches(node) {
			recursive[node] = true
		}
	}
	return recursive
}

// typeGraph maps each named type to the named types its schema contains directly (not
// through another named type).
type typeGraph struct {
	edges map[reflect.Type][]reflect.Type
}

func (g *typeGraph) visit(node reflect.Type) {
	if _, seen := g.edges[node]; seen {
		return
	}
	g.edges[node] = nil // seen: ends cycles
	var out []reflect.Type
	g.collectBody(node, &out)
	g.edges[node] = out
	for _, next := range out {
		g.visit(next)
	}
}

// collect adds the node an occurrence of t stands for: pointers and JSONSchemaAlias are
// followed, a named type is a node, and an unnamed one (a slice, a map, struct{...}) is
// part of the schema that contains it. An interface type is a leaf: it has no fields,
// and its JSONSchemaAlias method, if any, cannot be called without a value.
func (g *typeGraph) collect(t reflect.Type, out *[]reflect.Type) {
	switch t.Kind() {
	case reflect.Pointer:
		g.collect(t.Elem(), out)
		return
	case reflect.Interface:
		return
	}
	if t.Implements(schemaAliaserType) {
		g.collect(reflect.TypeOf(reflect.New(t).Interface().(schemaAliaser).JSONSchemaAlias()), out)
		return
	}
	if t.Name() != "" {
		*out = append(*out, t)
		return
	}
	g.collectBody(t, out)
}

// collectBody adds the nodes in the schema of t itself.
func (g *typeGraph) collectBody(t reflect.Type, out *[]reflect.Type) {
	if t.Implements(schemaProviderType) || t.Implements(protoEnumerType) {
		return
	}
	switch t.Kind() {
	case reflect.Struct:
		g.collectFields(t, out, map[reflect.Type]bool{})
	case reflect.Slice, reflect.Array:
		if t == rawMessageType || (t.Kind() == reflect.Slice && t.Elem() == byteType) {
			return
		}
		g.collect(t.Elem(), out)
	case reflect.Map:
		g.collect(t.Elem(), out)
	}
}

// collectFields adds the nodes of a struct's properties, embedded structs included.
func (g *typeGraph) collectFields(t reflect.Type, out *[]reflect.Type, embedded map[reflect.Type]bool) {
	if t.Kind() == reflect.Pointer {
		t = t.Elem()
	}
	if t.Kind() != reflect.Struct || embedded[t] {
		return
	}
	embedded[t] = true
	var propertyAlias func(string) any
	if t.Implements(schemaPropertyAliaserType) {
		propertyAlias = reflect.New(t).Interface().(schemaPropertyAliaser).JSONSchemaProperty
	}
	for i := range t.NumField() {
		f := t.Field(i)
		name, embed := schemaFieldName(f)
		if embed {
			g.collectFields(f.Type, out, embedded)
			continue
		}
		if name == "" {
			continue
		}
		field := f.Type
		if propertyAlias != nil {
			if alias := propertyAlias(name); alias != nil {
				field = reflect.TypeOf(alias)
			}
		}
		g.collect(field, out)
	}
}

// schemaFieldName is the reflector's reflectFieldName, reduced to what recursiveTypes
// needs: the property name of f ("" for none), and whether f's fields are flattened into
// its parent's.
func schemaFieldName(f reflect.StructField) (string, bool) {
	jsonTags := strings.Split(f.Tag.Get("json"), ",")
	if jsonTags[0] == "-" || strings.Split(f.Tag.Get("jsonschema"), ",")[0] == "-" {
		return "", false
	}
	if f.Anonymous && jsonTags[0] == "" {
		if k := f.Type.Kind(); k == reflect.Struct || (k == reflect.Pointer && f.Type.Elem().Kind() == reflect.Struct) {
			return "", true
		}
	}
	if slices.Contains(jsonTags[1:], "inline") {
		return "", true
	}
	if !f.Anonymous && !f.IsExported() {
		return "", false
	}
	if jsonTags[0] != "" {
		return jsonTags[0], false
	}
	return f.Name, false
}

// reaches reports whether node leads back to itself.
func (g *typeGraph) reaches(node reflect.Type) bool {
	seen := map[reflect.Type]bool{}
	stack := append([]reflect.Type(nil), g.edges[node]...)
	for len(stack) > 0 {
		next := stack[len(stack)-1]
		stack = stack[:len(stack)-1]
		if next == node {
			return true
		}
		if !seen[next] {
			seen[next] = true
			stack = append(stack, g.edges[next]...)
		}
	}
	return false
}

package iii

import (
	"context"
	"encoding/json"
	"io"
	"reflect"
	"sort"
	"strings"
	"testing"
	"time"

	"github.com/coder/websocket"
	"github.com/google/uuid"
)

type greetReq struct {
	Name  string `json:"name" jsonschema:"required"`
	Count int    `json:"count"`
}

type greetResp struct {
	Message string `json:"message"`
}

// TestInferSchema checks that a struct reflects into a JSON Schema describing its fields
// and types — the Go counterpart of Rust's JsonSchema derive.
func TestInferSchema(t *testing.T) {
	raw, err := InferSchema[greetReq]()
	if err != nil {
		t.Fatalf("InferSchema: %v", err)
	}
	var schema map[string]any
	if err := json.Unmarshal(raw, &schema); err != nil {
		t.Fatalf("schema is not valid JSON: %v\n%s", err, raw)
	}
	if schema["type"] != "object" {
		t.Errorf("schema type = %v, want object", schema["type"])
	}
	props, ok := schema["properties"].(map[string]any)
	if !ok {
		t.Fatalf("schema has no properties object: %s", raw)
	}
	if _, ok := props["name"]; !ok {
		t.Error("schema missing 'name' property")
	}
	if _, ok := props["count"]; !ok {
		t.Error("schema missing 'count' property")
	}
	// `required` from the jsonschema tag should surface name.
	if req, ok := schema["required"].([]any); ok {
		found := false
		for _, r := range req {
			if r == "name" {
				found = true
			}
		}
		if !found {
			t.Error("'name' should be required")
		}
	} else {
		t.Error("schema has no required list")
	}
}

// TestRegisterFunctionTypedSendsSchema verifies the typed registration sends a
// registerfunction frame carrying the inferred request/response schemas, and that the
// handler receives the unmarshaled typed request.
func TestRegisterFunctionTypedSendsSchema(t *testing.T) {
	m := newMockEngine(t)

	gotReq := make(chan greetReq, 1)
	m.onReceive = func(conn *websocket.Conn, msg map[string]json.RawMessage) {
		if messageType(msg) == string(MsgRegisterFunction) && messageID(msg) == "typed::greet" {
			// The frame must carry a non-null request_format describing the struct.
			if string(msg["request_format"]) == "null" || len(msg["request_format"]) == 0 {
				t.Error("registerfunction did not carry an inferred request_format")
			}
			jsonEqual(t, msg["metadata"], `{"owner":"typed"}`)
			id := mustUUID(t, "66666666-6666-6666-6666-666666666666")
			ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
			defer cancel()
			_ = m.send(ctx, conn, &InvokeFunctionMessage{
				InvocationID: &id,
				FunctionID:   "typed::greet",
				Data:         json.RawMessage(`{"name":"ada","count":2}`),
			})
		}
	}

	c := connectClient(t, m)
	_, err := RegisterFunctionTyped[greetReq, greetResp](c, "typed::greet",
		func(ctx context.Context, req greetReq) (greetResp, error) {
			gotReq <- req
			return greetResp{Message: "hi " + req.Name}, nil
		}, RegisterFunctionOptions{Metadata: json.RawMessage(`{"owner":"typed"}`)})
	if err != nil {
		t.Fatalf("RegisterFunctionTyped: %v", err)
	}

	select {
	case req := <-gotReq:
		if req.Name != "ada" || req.Count != 2 {
			t.Errorf("handler got %+v, want name=ada count=2", req)
		}
	case <-time.After(3 * time.Second):
		t.Fatal("typed handler was not invoked")
	}
}

// TestRegisterFunctionTypedDeliversMetadata verifies typed handlers can read the
// per-invocation metadata from ctx (see MetadataFromContext) while still receiving a
// decoded request and inferred schemas.
func TestRegisterFunctionTypedDeliversMetadata(t *testing.T) {
	m := newMockEngine(t)

	gotReq := make(chan greetReq, 1)
	gotMeta := make(chan json.RawMessage, 1)
	m.onReceive = func(conn *websocket.Conn, msg map[string]json.RawMessage) {
		if messageType(msg) == string(MsgRegisterFunction) && messageID(msg) == "typed::meta" {
			if string(msg["request_format"]) == "null" || len(msg["request_format"]) == 0 {
				t.Error("registerfunction did not carry an inferred request_format")
			}
			id := mustUUID(t, "77777777-7777-7777-7777-777777777777")
			ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
			defer cancel()
			_ = m.send(ctx, conn, &InvokeFunctionMessage{
				InvocationID: &id,
				FunctionID:   "typed::meta",
				Data:         json.RawMessage(`{"name":"grace","count":3}`),
				Metadata:     json.RawMessage(`{"tenant":"acme"}`),
			})
		}
	}

	c := connectClient(t, m)
	_, err := RegisterFunctionTyped[greetReq, greetResp](c, "typed::meta",
		func(ctx context.Context, req greetReq) (greetResp, error) {
			metadata, _ := MetadataFromContext(ctx)
			gotReq <- req
			gotMeta <- metadata
			return greetResp{Message: "hi " + req.Name}, nil
		})
	if err != nil {
		t.Fatalf("RegisterFunctionTyped: %v", err)
	}

	select {
	case req := <-gotReq:
		if req.Name != "grace" || req.Count != 3 {
			t.Errorf("handler got %+v, want name=grace count=3", req)
		}
	case <-time.After(3 * time.Second):
		t.Fatal("typed metadata handler was not invoked")
	}

	select {
	case metadata := <-gotMeta:
		if string(metadata) != `{"tenant":"acme"}` {
			t.Errorf("metadata = %s, want {\"tenant\":\"acme\"}", metadata)
		}
	case <-time.After(3 * time.Second):
		t.Fatal("typed metadata handler did not receive metadata")
	}
}

type pinnedStatus string

type pinnedAddress struct {
	Street string `json:"street" jsonschema:"required,minLength=1"`
	Zip    string `json:"zip,omitempty" jsonschema:"pattern=^[0-9]{5}$"`
}

type PinnedTrace struct {
	TraceID string `json:"trace_id,omitempty"`
}

// pinnedOrder covers what an existing schema is made of: scalars, tags, an embedded
// struct, slices, maps, arrays, a nested struct used three times (inlined each time,
// not moved to $defs), interface fields, json.RawMessage, []byte, time.Time and a
// nullable pointer.
type pinnedOrder struct {
	PinnedTrace
	ID        string            `json:"id" jsonschema:"required,description=Order id"`
	Status    pinnedStatus      `json:"status" jsonschema:"enum=open,enum=closed"`
	Quantity  int               `json:"quantity" jsonschema:"minimum=1"`
	Price     float64           `json:"price"`
	Paid      bool              `json:"paid"`
	Tags      []string          `json:"tags,omitempty"`
	Labels    map[string]string `json:"labels,omitempty"`
	Counts    map[int]int64     `json:"counts,omitempty"`
	Shipping  *pinnedAddress    `json:"shipping,omitempty"`
	Billing   pinnedAddress     `json:"billing"`
	History   []pinnedAddress   `json:"history,omitempty"`
	Extra     any               `json:"extra,omitempty"`
	Attrs     map[string]any    `json:"attrs,omitempty"`
	Raw       json.RawMessage   `json:"raw,omitempty"`
	Blob      []byte            `json:"blob,omitempty"`
	Grid      [2][2]int         `json:"grid"`
	CreatedAt time.Time         `json:"created_at"`
	Note      *string           `json:"note,omitempty" jsonschema:"nullable"`
	Skipped   string            `json:"-"`
}

// pinnedHiddenParent points at its own type only through a field JSON skips, so it is
// not recursive and stays inline.
type pinnedHiddenParent struct {
	Name   string              `json:"name"`
	Parent *pinnedHiddenParent `json:"-"`
}

// TestInferSchemaKeepsExistingSchemas pins the exact bytes of schemas for types without
// recursion: support for recursive and interface types must not change them.
func TestInferSchemaKeepsExistingSchemas(t *testing.T) {
	cases := []struct {
		name  string
		infer func() (json.RawMessage, error)
		want  string
	}{
		{"greetReq", InferSchema[greetReq], `{"$schema":"https://json-schema.org/draft/2020-12/schema","$id":"https://github.com/iii-hq/iii/sdk/packages/go/iii/greet-req","properties":{"name":{"type":"string"},"count":{"type":"integer"}},"additionalProperties":false,"type":"object","required":["name","count"]}`},
		{"pinnedOrder", InferSchema[pinnedOrder], `{"$schema":"https://json-schema.org/draft/2020-12/schema","$id":"https://github.com/iii-hq/iii/sdk/packages/go/iii/pinned-order","properties":{"trace_id":{"type":"string"},"id":{"type":"string","description":"Order id"},"status":{"type":"string","enum":["open","closed"]},"quantity":{"type":"integer","minimum":1},"price":{"type":"number"},"paid":{"type":"boolean"},"tags":{"items":{"type":"string"},"type":"array"},"labels":{"additionalProperties":{"type":"string"},"type":"object"},"counts":{"patternProperties":{"^[0-9]+$":{"type":"integer"}},"additionalProperties":false,"type":"object"},"shipping":{"properties":{"street":{"type":"string","minLength":1},"zip":{"type":"string","pattern":"^[0-9]{5}$"}},"additionalProperties":false,"type":"object","required":["street"]},"billing":{"properties":{"street":{"type":"string","minLength":1},"zip":{"type":"string","pattern":"^[0-9]{5}$"}},"additionalProperties":false,"type":"object","required":["street"]},"history":{"items":{"properties":{"street":{"type":"string","minLength":1},"zip":{"type":"string","pattern":"^[0-9]{5}$"}},"additionalProperties":false,"type":"object","required":["street"]},"type":"array"},"extra":true,"attrs":{"type":"object"},"raw":true,"blob":{"type":"string","contentEncoding":"base64"},"grid":{"items":{"items":{"type":"integer"},"type":"array","maxItems":2,"minItems":2},"type":"array","maxItems":2,"minItems":2},"created_at":{"type":"string","format":"date-time"},"note":{"oneOf":[{"type":"string"},{"type":"null"}]}},"additionalProperties":false,"type":"object","required":["id","status","quantity","price","paid","billing","grid","created_at"]}`},
		{"[]pinnedAddress", InferSchema[[]pinnedAddress], `{"$schema":"https://json-schema.org/draft/2020-12/schema","items":{"properties":{"street":{"type":"string","minLength":1},"zip":{"type":"string","pattern":"^[0-9]{5}$"}},"additionalProperties":false,"type":"object","required":["street"]},"type":"array"}`},
		{"*pinnedAddress", InferSchema[*pinnedAddress], `{"$schema":"https://json-schema.org/draft/2020-12/schema","$id":"https://github.com/iii-hq/iii/sdk/packages/go/iii/pinned-address","properties":{"street":{"type":"string","minLength":1},"zip":{"type":"string","pattern":"^[0-9]{5}$"}},"additionalProperties":false,"type":"object","required":["street"]}`},
		{"string", InferSchema[string], `{"$schema":"https://json-schema.org/draft/2020-12/schema","type":"string"}`},
		{"pinnedHiddenParent", InferSchema[pinnedHiddenParent], `{"$schema":"https://json-schema.org/draft/2020-12/schema","$id":"https://github.com/iii-hq/iii/sdk/packages/go/iii/pinned-hidden-parent","properties":{"name":{"type":"string"}},"additionalProperties":false,"type":"object","required":["name"]}`},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			raw, err := tc.infer()
			if err != nil {
				t.Fatalf("InferSchema: %v", err)
			}
			if string(raw) != tc.want {
				t.Errorf("schema changed:\n got: %s\nwant: %s", raw, tc.want)
			}
		})
	}
}

type schemaShape interface{ Area() float64 }

type schemaInterfaces struct {
	Any    any         `json:"any"`
	Err    error       `json:"err"`
	Reader io.Reader   `json:"reader"`
	Shape  schemaShape `json:"shape"`
	AnyPtr *any        `json:"any_ptr"`
}

// TestInferSchemaInterfaceTypes checks that an interface type (any, error, io.Reader, a
// user interface) reflects into a schema that accepts any value, at the top level (where
// a nil type used to reach the reflector and panic) and nested in a struct.
func TestInferSchemaInterfaceTypes(t *testing.T) {
	cases := []struct {
		name  string
		infer func() (json.RawMessage, error)
	}{
		{"any", InferSchema[any]},
		{"*any", InferSchema[*any]},
		{"error", InferSchema[error]},
		{"io.Reader", InferSchema[io.Reader]},
		{"schemaShape", InferSchema[schemaShape]},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			raw, err := tc.infer()
			if err != nil {
				t.Fatalf("InferSchema: %v", err)
			}
			schema := decodeSchema(t, raw)
			// Only annotations: no keyword restricts the value.
			for key := range schema {
				if key != "$schema" && key != "$id" {
					t.Errorf("schema has %q, want an empty schema: %s", key, raw)
				}
			}
		})
	}

	raw, err := InferSchema[schemaInterfaces]()
	if err != nil {
		t.Fatalf("InferSchema[schemaInterfaces]: %v", err)
	}
	props, _ := decodeSchema(t, raw)["properties"].(map[string]any)
	for _, name := range []string{"any", "err", "reader", "shape", "any_ptr"} {
		if props[name] != true {
			t.Errorf("property %q = %v, want true (any value): %s", name, props[name], raw)
		}
	}
}

type schemaSelf struct {
	Name string      `json:"name"`
	Next *schemaSelf `json:"next,omitempty"`
}

type schemaTree struct {
	Name     string       `json:"name"`
	Children []schemaTree `json:"children,omitempty"`
}

type schemaMapNode struct {
	Kids    map[string]schemaMapNode `json:"kids,omitempty"`
	ByIndex map[int]*schemaMapNode   `json:"by_index,omitempty"`
}

type schemaPing struct {
	Pong *schemaPong `json:"pong,omitempty"`
}

type schemaPong struct {
	Ping []schemaPing `json:"ping,omitempty"`
}

// schemaListNode recurses through a named slice type, which is a $defs entry of its own.
type schemaListNode struct {
	Items schemaList `json:"items"`
}

type schemaList []schemaListNode

// schemaJSONTree recurses through itself as a named map type.
type schemaJSONTree map[string]schemaJSONTree

// schemaForest is not recursive itself: it holds recursive types, and stays inline.
type schemaForest struct {
	Trees []schemaTree `json:"trees"`
	Self  schemaSelf   `json:"self"`
}

type schemaGenericList[T any] struct {
	Value T                     `json:"value"`
	Next  *schemaGenericList[T] `json:"next,omitempty"`
}

// TestInferSchemaRecursiveTypes checks that a type that contains itself (directly,
// through a pointer, a slice, a map, a named slice or map, or another type) reflects into
// a schema with a $defs entry per recursive type instead of overflowing the stack, and
// that every $ref resolves in the document.
func TestInferSchemaRecursiveTypes(t *testing.T) {
	cases := []struct {
		name  string
		infer func() (json.RawMessage, error)
		defs  []string
	}{
		{"Observed", InferSchema[Observed], []string{"Caller"}},
		{"*Caller", InferSchema[*Caller], []string{"Caller"}},
		{"schemaSelf", InferSchema[schemaSelf], []string{"schemaSelf"}},
		{"schemaTree", InferSchema[schemaTree], []string{"schemaTree"}},
		{"[]schemaTree", InferSchema[[]schemaTree], []string{"schemaTree"}},
		{"schemaMapNode", InferSchema[schemaMapNode], []string{"schemaMapNode"}},
		{"schemaPing", InferSchema[schemaPing], []string{"schemaPing", "schemaPong"}},
		{"schemaListNode", InferSchema[schemaListNode], []string{"schemaList", "schemaListNode"}},
		{"schemaJSONTree", InferSchema[schemaJSONTree], []string{"schemaJSONTree"}},
		{"schemaForest", InferSchema[schemaForest], []string{"schemaSelf", "schemaTree"}},
		{"schemaGenericList", InferSchema[schemaGenericList[int]], []string{"schemaGenericList_int_"}},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			raw, err := tc.infer()
			if err != nil {
				t.Fatalf("InferSchema: %v", err)
			}
			schema := decodeSchema(t, raw)
			if _, ok := schema["$ref"]; ok {
				t.Errorf("root is a $ref, want the type inlined: %s", raw)
			}
			if schema["type"] == nil {
				t.Errorf("root has no type: %s", raw)
			}
			defs, _ := schema["$defs"].(map[string]any)
			var names []string
			for name, def := range defs {
				names = append(names, name)
				body, _ := def.(map[string]any)
				if body["$id"] != nil || body["$schema"] != nil {
					t.Errorf("$defs[%q] carries $id or $schema: %v", name, body)
				}
			}
			sort.Strings(names)
			if strings.Join(names, ",") != strings.Join(tc.defs, ",") {
				t.Errorf("$defs = %v, want %v: %s", names, tc.defs, raw)
			}
			checkLocalRefs(t, schema, raw)
		})
	}
}

// TestInferSchemaRecursiveShape pins the schema of a recursive type and of Observed, the
// payload of an observer's target: the root is inlined, the recursive type is described
// once in $defs, and its occurrences are references.
func TestInferSchemaRecursiveShape(t *testing.T) {
	raw, err := InferSchema[schemaTree]()
	if err != nil {
		t.Fatalf("InferSchema[schemaTree]: %v", err)
	}
	const want = `{"$schema":"https://json-schema.org/draft/2020-12/schema","$id":"https://github.com/iii-hq/iii/sdk/packages/go/iii/schema-tree","$defs":{"schemaTree":{"properties":{"name":{"type":"string"},"children":{"items":{"$ref":"#/$defs/schemaTree"},"type":"array"}},"additionalProperties":false,"type":"object","required":["name"]}},"properties":{"name":{"type":"string"},"children":{"items":{"$ref":"#/$defs/schemaTree"},"type":"array"}},"additionalProperties":false,"type":"object","required":["name"]}`
	if string(raw) != want {
		t.Errorf("schema:\n got: %s\nwant: %s", raw, want)
	}

	raw, err = InferSchema[Observed]()
	if err != nil {
		t.Fatalf("InferSchema[Observed]: %v", err)
	}
	schema := decodeSchema(t, raw)
	props, _ := schema["properties"].(map[string]any)
	jsonEqual(t, mustJSON(t, props["caller"]), `{"$ref":"#/$defs/Caller"}`)
	// ErrorBody is not recursive: still inline.
	jsonEqual(t, mustJSON(t, props["error"]), `{"properties":{"code":{"type":"string"},"message":{"type":"string"},"stacktrace":{"type":"string"}},"additionalProperties":false,"type":"object","required":["code","message"]}`)
	defs, _ := schema["$defs"].(map[string]any)
	jsonEqual(t, mustJSON(t, defs["Caller"]), `{"properties":{"worker_id":{"type":"string"},"worker_name":{"type":"string"},"function_id":{"type":"string"},"invocation_id":{"type":"string"},"parent":{"$ref":"#/$defs/Caller"}},"additionalProperties":false,"type":"object"}`)
}

// TestInferSchemaRecursiveDefNames checks that two recursive types sharing a Go name get
// distinct $defs entries, each with its own body.
func TestInferSchemaRecursiveDefNames(t *testing.T) {
	var first, second reflect.Type
	{
		type node struct {
			N    int   `json:"n"`
			Next *node `json:"next,omitempty"`
		}
		first = reflect.TypeOf(node{})
	}
	{
		type node struct {
			S    string `json:"s"`
			Next *node  `json:"next,omitempty"`
		}
		second = reflect.TypeOf(node{})
	}
	both := reflect.StructOf([]reflect.StructField{
		{Name: "First", Type: first, Tag: `json:"first"`},
		{Name: "Second", Type: second, Tag: `json:"second"`},
	})
	raw, err := schemaOf(both)
	if err != nil {
		t.Fatalf("schemaOf: %v", err)
	}
	schema := decodeSchema(t, raw)
	checkLocalRefs(t, schema, raw)
	props, _ := schema["properties"].(map[string]any)
	jsonEqual(t, mustJSON(t, props["first"]), `{"$ref":"#/$defs/node"}`)
	jsonEqual(t, mustJSON(t, props["second"]), `{"$ref":"#/$defs/node2"}`)
	defs, _ := schema["$defs"].(map[string]any)
	jsonEqual(t, mustJSON(t, defs["node"]), `{"properties":{"n":{"type":"integer"},"next":{"$ref":"#/$defs/node"}},"additionalProperties":false,"type":"object","required":["n"]}`)
	jsonEqual(t, mustJSON(t, defs["node2"]), `{"properties":{"s":{"type":"string"},"next":{"$ref":"#/$defs/node2"}},"additionalProperties":false,"type":"object","required":["s"]}`)
}

type schemaUnsupported struct {
	Done chan struct{} `json:"done"`
}

// TestInferSchemaUnsupportedTypeIsAnError checks that a type with no JSON Schema (a chan
// or func field) fails the inference with an error instead of panicking.
func TestInferSchemaUnsupportedTypeIsAnError(t *testing.T) {
	if _, err := InferSchema[schemaUnsupported](); err == nil || !strings.Contains(err.Error(), "chan struct {}") {
		t.Errorf("InferSchema[schemaUnsupported] error = %v, want one naming chan struct {}", err)
	}
	c := New("ws://127.0.0.1:0")
	_, err := RegisterFunctionTyped(c, "typed::func-out", func(ctx context.Context, req greetReq) (func(), error) {
		return nil, nil
	})
	if err == nil || !strings.Contains(err.Error(), "response schema") {
		t.Errorf("RegisterFunctionTyped with a func response: error = %v, want a response schema error", err)
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	if len(c.functions) != 0 {
		t.Errorf("registered %d functions, want none", len(c.functions))
	}
}

// TestRegisterFunctionTypedInterfaceAndRecursiveTypes registers typed functions whose
// request or response is an interface type, and a typed observer target taking
// Observed (recursive through Caller.Parent); each registers with its schemas, and each
// handler runs and answers.
func TestRegisterFunctionTypedInterfaceAndRecursiveTypes(t *testing.T) {
	m := newMockEngine(t)
	invocations := map[string]uuid.UUID{
		"typed::any-out":     mustUUID(t, "a1a1a1a1-a1a1-a1a1-a1a1-a1a1a1a1a1a1"),
		"typed::any-in":      mustUUID(t, "b2b2b2b2-b2b2-b2b2-b2b2-b2b2b2b2b2b2"),
		"typed::on-observed": mustUUID(t, "c3c3c3c3-c3c3-c3c3-c3c3-c3c3c3c3c3c3"),
	}
	payloads := map[string]string{
		"typed::any-out":     `{"name":"ada","count":1}`,
		"typed::any-in":      `{"anything":[1,"two"]}`,
		"typed::on-observed": `{"observer_id":"o1","activation_id":"a1","function_id":"orders::create","namespace":"default","invocation_id":"i1","status":"ok","caller":{"function_id":"orders::api","parent":{"function_id":"http::on-request"}},"started_at_ms":1,"duration_ms":2}`,
	}
	m.onReceive = func(conn *websocket.Conn, msg map[string]json.RawMessage) {
		id := messageID(msg)
		invocationID, ok := invocations[id]
		if messageType(msg) != string(MsgRegisterFunction) || !ok {
			return
		}
		ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
		defer cancel()
		_ = m.send(ctx, conn, &InvokeFunctionMessage{
			InvocationID: &invocationID,
			FunctionID:   id,
			Data:         json.RawMessage(payloads[id]),
		})
	}

	c := connectClient(t, m)
	if _, err := RegisterFunctionTyped(c, "typed::any-out", func(ctx context.Context, req greetReq) (any, error) {
		return map[string]any{"hello": req.Name}, nil
	}); err != nil {
		t.Fatalf("RegisterFunctionTyped[greetReq, any]: %v", err)
	}
	if _, err := RegisterFunctionTyped(c, "typed::any-in", func(ctx context.Context, req any) (greetResp, error) {
		got, _ := json.Marshal(req)
		return greetResp{Message: string(got)}, nil
	}); err != nil {
		t.Fatalf("RegisterFunctionTyped[any, greetResp]: %v", err)
	}
	if _, err := RegisterFunctionTyped(c, "typed::on-observed", func(ctx context.Context, ev Observed) (any, error) {
		if ev.Caller == nil || ev.Caller.Parent == nil {
			return nil, &InvocationError{Code: "no_parent", Message: "caller or its parent missing"}
		}
		return map[string]string{"function": ev.FunctionID, "parent": ev.Caller.Parent.FunctionID}, nil
	}); err != nil {
		t.Fatalf("RegisterFunctionTyped[Observed, any]: %v", err)
	}

	got := m.waitFor(func(msgs []map[string]json.RawMessage) bool {
		return countType(msgs, string(MsgInvocationResult)) >= len(invocations)
	}, 3*time.Second)

	register := func(id string) map[string]json.RawMessage {
		return firstWhere(got, func(msg map[string]json.RawMessage) bool {
			return messageType(msg) == string(MsgRegisterFunction) && messageID(msg) == id
		})
	}
	const anySchema = `{"$schema":"https://json-schema.org/draft/2020-12/schema"}`
	jsonEqual(t, register("typed::any-out")["response_format"], anySchema)
	jsonEqual(t, register("typed::any-in")["request_format"], anySchema)
	jsonEqual(t, register("typed::on-observed")["response_format"], anySchema)
	observed := decodeSchema(t, register("typed::on-observed")["request_format"])
	if defs, _ := observed["$defs"].(map[string]any); defs["Caller"] == nil {
		t.Errorf("Observed request_format has no $defs.Caller: %v", observed)
	}

	results := map[string]string{
		"typed::any-out":     `{"hello":"ada"}`,
		"typed::any-in":      `{"message":"{\"anything\":[1,\"two\"]}"}`,
		"typed::on-observed": `{"function":"orders::create","parent":"http::on-request"}`,
	}
	for id, want := range results {
		res := firstWhere(got, func(msg map[string]json.RawMessage) bool {
			return messageType(msg) == string(MsgInvocationResult) && stringField(msg, "invocation_id") == invocations[id].String()
		})
		if res == nil {
			t.Errorf("%s: no invocation result", id)
			continue
		}
		if len(res["error"]) > 0 && string(res["error"]) != "null" {
			t.Errorf("%s: error %s", id, res["error"])
			continue
		}
		jsonEqual(t, res["result"], want)
	}
}

// decodeSchema decodes a schema document into a map.
func decodeSchema(t *testing.T, raw []byte) map[string]any {
	t.Helper()
	var schema map[string]any
	if err := json.Unmarshal(raw, &schema); err != nil {
		t.Fatalf("schema is not a JSON object: %v\n%s", err, raw)
	}
	return schema
}

func mustJSON(t *testing.T, v any) []byte {
	t.Helper()
	raw, err := json.Marshal(v)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	return raw
}

// checkLocalRefs fails the test unless every "$ref" in the schema points at an entry of
// its root "$defs", and every entry is referenced.
func checkLocalRefs(t *testing.T, schema map[string]any, raw []byte) {
	t.Helper()
	defs, _ := schema["$defs"].(map[string]any)
	used := map[string]bool{}
	var walk func(v any)
	walk = func(v any) {
		switch v := v.(type) {
		case map[string]any:
			if ref, ok := v["$ref"].(string); ok {
				name, local := strings.CutPrefix(ref, "#/$defs/")
				if !local || defs[name] == nil {
					t.Errorf("$ref %q does not resolve in the document: %s", ref, raw)
				}
				used[name] = true
			}
			for _, child := range v {
				walk(child)
			}
		case []any:
			for _, child := range v {
				walk(child)
			}
		}
	}
	walk(schema)
	for name := range defs {
		if !used[name] {
			t.Errorf("$defs[%q] is never referenced: %s", name, raw)
		}
	}
}

// TestRegisterWorkerConnects verifies RegisterWorker starts the lifecycle so the client
// reaches connected without a separate Connect call.
func TestRegisterWorkerConnects(t *testing.T) {
	m := newMockEngine(t)
	c := RegisterWorker(m.url)
	t.Cleanup(func() { _ = c.Close() })

	// Wait for connection by polling State (RegisterWorker connects in the background).
	deadline := time.Now().Add(3 * time.Second)
	for time.Now().Before(deadline) {
		if c.ConnectionState() == StateConnected {
			return
		}
		time.Sleep(20 * time.Millisecond)
	}
	t.Fatalf("RegisterWorker did not reach connected; state = %s", c.ConnectionState())
}

// TestConnectFailsFastWhenRetriesExhausted verifies Connect returns (rather than
// blocking forever) when the supervisor gives up after MaxRetries.
func TestConnectFailsFastWhenRetriesExhausted(t *testing.T) {
	// Point at a dead address with a tiny, finite retry budget.
	c := New("ws://127.0.0.1:1", WithReconnectConfig(ReconnectConfig{
		InitialDelay:      time.Millisecond,
		MaxDelay:          time.Millisecond,
		BackoffMultiplier: 1,
		JitterFactor:      0,
		MaxRetries:        2,
	}))
	t.Cleanup(func() { _ = c.Close() })

	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	err := c.Connect(ctx)
	if err == nil {
		t.Fatal("Connect should fail when the engine is unreachable and retries are exhausted")
	}
	if ctx.Err() != nil {
		t.Fatal("Connect blocked until ctx timeout instead of failing fast on exhausted retries")
	}
}

// TestConnectIdempotentSingleSupervisor verifies multiple Connect calls don't spawn
// multiple supervisors (no panic / race; the second call observes the same outcome).
func TestConnectIdempotentSingleSupervisor(t *testing.T) {
	m := newMockEngine(t)
	c := New(m.url)
	t.Cleanup(func() { _ = c.Close() })

	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	if err := c.Connect(ctx); err != nil {
		t.Fatalf("first Connect: %v", err)
	}
	// Second call must return promptly (already connected), not start a new supervisor.
	if err := c.Connect(ctx); err != nil {
		t.Fatalf("second Connect: %v", err)
	}
}

package iii

import (
	"encoding/json"
	"testing"
)

// TestMiddlewareFunctionInputDecodesEngineInput decodes the exact object the engine
// builds for the middleware (engine/src/engine/mod.rs, middleware_input): action is null
// for a synchronous call and the namespace is always present.
func TestMiddlewareFunctionInputDecodesEngineInput(t *testing.T) {
	raw := `{"function_id":"orders::create","payload":{"item":"book","qty":2},"action":null,"context":{"source":"browser"},"namespace":"default"}`
	var in MiddlewareFunctionInput
	if err := json.Unmarshal([]byte(raw), &in); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}
	if in.FunctionID != "orders::create" {
		t.Errorf("FunctionID = %q", in.FunctionID)
	}
	if string(in.Payload) != `{"item":"book","qty":2}` {
		t.Errorf("Payload = %s", in.Payload)
	}
	if in.Action != nil {
		t.Errorf("Action = %+v, want nil for a null action", in.Action)
	}
	if string(in.Context) != `{"source":"browser"}` {
		t.Errorf("Context = %s", in.Context)
	}
	if in.Namespace != "default" {
		t.Errorf("Namespace = %q", in.Namespace)
	}
}

func TestMiddlewareFunctionInputDecodesEnqueueAction(t *testing.T) {
	raw := `{"function_id":"jobs::run","payload":{},"action":{"type":"enqueue","queue":"jobs"},"context":{}}`
	var in MiddlewareFunctionInput
	if err := json.Unmarshal([]byte(raw), &in); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}
	if in.Action == nil || in.Action.Type != "enqueue" || in.Action.Queue != "jobs" {
		t.Fatalf("Action = %+v, want enqueue/jobs", in.Action)
	}
	if in.Namespace != "" {
		t.Errorf("Namespace = %q, want empty when absent", in.Namespace)
	}
}

// TestMiddlewareFunctionInputJSONNames pins the wire names against Node
// MiddlewareFunctionInput (iii-types.ts): optional fields are omitted when unset.
func TestMiddlewareFunctionInputJSONNames(t *testing.T) {
	full, err := json.Marshal(MiddlewareFunctionInput{
		FunctionID: "f",
		Payload:    json.RawMessage(`{"a":1}`),
		Action:     VoidAction(),
		Context:    json.RawMessage(`{"u":"x"}`),
		Namespace:  "orders",
	})
	if err != nil {
		t.Fatal(err)
	}
	if want := `{"function_id":"f","payload":{"a":1},"action":{"type":"void"},"context":{"u":"x"},"namespace":"orders"}`; string(full) != want {
		t.Errorf("full = %s\nwant  %s", full, want)
	}
	minimal, err := json.Marshal(MiddlewareFunctionInput{
		FunctionID: "f",
		Payload:    json.RawMessage(`{}`),
		Context:    json.RawMessage(`{}`),
	})
	if err != nil {
		t.Fatal(err)
	}
	if want := `{"function_id":"f","payload":{},"context":{}}`; string(minimal) != want {
		t.Errorf("minimal = %s\nwant     %s", minimal, want)
	}
}

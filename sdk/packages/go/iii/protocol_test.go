package iii

import (
	"encoding/json"
	"testing"

	"github.com/google/uuid"
)

// jsonEqual compares two JSON documents structurally (key order independent), which
// is the correct notion of "byte-for-byte" for JSON objects: the engine's serde and
// Go's encoding/json may emit fields in different orders but the engine parses by key.
func jsonEqual(t *testing.T, got []byte, want string) {
	t.Helper()
	var g, w any
	if err := json.Unmarshal(got, &g); err != nil {
		t.Fatalf("got is not valid JSON: %v\n%s", err, got)
	}
	if err := json.Unmarshal([]byte(want), &w); err != nil {
		t.Fatalf("want is not valid JSON: %v\n%s", err, want)
	}
	gb, _ := json.Marshal(g)
	wb, _ := json.Marshal(w)
	if string(gb) != string(wb) {
		t.Errorf("wire mismatch:\n got: %s\nwant: %s", gb, wb)
	}
}

// TestGoldenFrames pins our marshaled output against frames the engine itself
// accepts or produces. The "want" strings are lifted from the engine's own tests in
// engine/src/protocol.rs (cited per case) so this suite fails loudly if our encoding
// ever drifts from what the engine expects.
func TestGoldenFrames(t *testing.T) {
	// Fixed UUID so frames are deterministic.
	id := uuid.MustParse("11111111-1111-1111-1111-111111111111")

	tests := []struct {
		name string
		msg  any
		want string
	}{
		{
			// engine/src/protocol.rs:329-354 (deserialize_invoke_function_with_void_action):
			// void action is fire-and-forget, so no invocation_id is sent.
			name: "invoke void omits invocation_id",
			msg: &InvokeFunctionMessage{
				FunctionID: "audit.log",
				Data:       json.RawMessage(`{"event":"login"}`),
				Action:     VoidAction(),
			},
			want: `{"type":"invokefunction","function_id":"audit.log","data":{"event":"login"},"action":{"type":"void"}}`,
		},
		{
			// engine/src/protocol.rs:300-327 (deserialize_invoke_function_with_enqueue_action).
			name: "invoke enqueue carries queue",
			msg: &InvokeFunctionMessage{
				FunctionID: "payment.process",
				Data:       json.RawMessage(`{"amount":100}`),
				Action:     EnqueueAction("payment"),
			},
			want: `{"type":"invokefunction","function_id":"payment.process","data":{"amount":100},"action":{"type":"enqueue","queue":"payment"}}`,
		},
		{
			// engine/src/protocol.rs:379-403 (serialize_invoke_function_without_action_omits_field):
			// action, traceparent and baggage are all omitted when unset.
			name: "invoke without action/trace omits optionals",
			msg: &InvokeFunctionMessage{
				FunctionID: "test.fn",
				Data:       json.RawMessage(`{}`),
			},
			want: `{"type":"invokefunction","function_id":"test.fn","data":{}}`,
		},
		{
			// invocation_id present => the engine awaits an InvocationResult (engine/src/protocol.rs:86).
			name: "invoke with invocation_id",
			msg: &InvokeFunctionMessage{
				InvocationID: &id,
				FunctionID:   "sync.call",
				Data:         json.RawMessage(`{}`),
			},
			want: `{"type":"invokefunction","invocation_id":"11111111-1111-1111-1111-111111111111","function_id":"sync.call","data":{}}`,
		},
		{
			// Per-invocation metadata rides its own "metadata" field alongside data, not
			// folded into the payload (engine/src/protocol.rs:97-102). Present => emitted.
			name: "invoke carries metadata when set",
			msg: &InvokeFunctionMessage{
				FunctionID: "audit.log",
				Data:       json.RawMessage(`{"event":"login"}`),
				Metadata:   json.RawMessage(`{"tenant":"acme"}`),
			},
			want: `{"type":"invokefunction","function_id":"audit.log","data":{"event":"login"},"metadata":{"tenant":"acme"}}`,
		},
		{
			// metadata uses skip_serializing_if = Option::is_none on the engine
			// (engine/src/protocol.rs:101), so a nil Metadata must be OMITTED, never null.
			name: "invoke omits metadata when nil",
			msg: &InvokeFunctionMessage{
				FunctionID: "test.fn",
				Data:       json.RawMessage(`{}`),
			},
			want: `{"type":"invokefunction","function_id":"test.fn","data":{}}`,
		},
		{
			// RegisterTrigger uses the wire field "trigger_type", NOT "type"
			// (engine/src/protocol.rs:53). This is the rename we must replicate.
			name: "register trigger uses trigger_type field",
			msg: &RegisterTriggerMessage{
				ID:          "my-trigger-1",
				TriggerType: "http",
				FunctionID:  "my::handler",
				Config:      json.RawMessage(`{"api_path":"/webhooks/data","http_method":"POST"}`),
			},
			want: `{"type":"registertrigger","id":"my-trigger-1","trigger_type":"http","function_id":"my::handler","config":{"api_path":"/webhooks/data","http_method":"POST"}}`,
		},
		{
			// InvocationResult.invocation_id is required, not omitempty
			// (engine/src/protocol.rs:99): a result must be routable.
			name: "invocation result requires invocation_id",
			msg: &InvocationResultMessage{
				InvocationID: id,
				FunctionID:   "sync.call",
				Result:       json.RawMessage(`{"ok":true}`),
			},
			want: `{"type":"invocationresult","invocation_id":"11111111-1111-1111-1111-111111111111","function_id":"sync.call","result":{"ok":true}}`,
		},
		{
			// request_format/response_format lack skip_serializing_if in the engine
			// (engine/src/protocol.rs:75-76), so they serialize as null when unset.
			name: "register function emits null formats",
			msg: &RegisterFunctionMessage{
				ID: "hello::greet",
			},
			want: `{"type":"registerfunction","id":"hello::greet","request_format":null,"response_format":null}`,
		},
		{
			// Payloadless variant (engine/src/protocol.rs:121).
			name: "ping is just a type tag",
			msg:  &PingMessage{},
			want: `{"type":"ping"}`,
		},
		{
			// MarshalMessage accepts value forms too, not just pointers — the
			// documented "by pointer or value" contract.
			name: "value message marshals like its pointer",
			msg:  PingMessage{},
			want: `{"type":"ping"}`,
		},
		{
			name: "value invoke marshals like its pointer",
			msg:  InvokeFunctionMessage{FunctionID: "test.fn", Data: json.RawMessage(`{}`)},
			want: `{"type":"invokefunction","function_id":"test.fn","data":{}}`,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got, err := MarshalMessage(tt.msg)
			if err != nil {
				t.Fatalf("MarshalMessage: %v", err)
			}
			jsonEqual(t, got, tt.want)
		})
	}
}

// TestUnmarshalRoundtrip verifies the tag-dispatch decoder: each frame decodes into
// exactly the matching variant. The fire-and-forget case (invocation_id absent =>
// nil pointer) is the one with real semantic weight.
func TestUnmarshalRoundtrip(t *testing.T) {
	t.Run("invoke without invocation_id decodes to nil pointer", func(t *testing.T) {
		// engine/src/protocol.rs:356-377 (deserialize_invoke_function_without_action).
		frame := `{"type":"invokefunction","invocation_id":null,"function_id":"sync.call","data":{}}`
		dec, err := UnmarshalMessage([]byte(frame))
		if err != nil {
			t.Fatalf("UnmarshalMessage: %v", err)
		}
		if dec.Type != MsgInvokeFunction || dec.InvokeFunction == nil {
			t.Fatalf("expected invokefunction variant, got %q", dec.Type)
		}
		if dec.InvokeFunction.InvocationID != nil {
			t.Errorf("invocation_id null must decode to nil pointer (fire-and-forget), got %v", dec.InvokeFunction.InvocationID)
		}
	})

	t.Run("invoke without metadata decodes to nil (backward compatible)", func(t *testing.T) {
		// An older engine never sends the metadata key; serde's #[serde(default)]
		// (engine/src/protocol.rs:101) makes that "no metadata", which on our side must be
		// a nil RawMessage so handlers can test metadata == nil.
		frame := `{"type":"invokefunction","invocation_id":null,"function_id":"sync.call","data":{}}`
		dec, err := UnmarshalMessage([]byte(frame))
		if err != nil {
			t.Fatalf("UnmarshalMessage: %v", err)
		}
		if dec.InvokeFunction == nil {
			t.Fatalf("expected invokefunction variant, got %q", dec.Type)
		}
		if dec.InvokeFunction.Metadata != nil {
			t.Errorf("absent metadata must decode to nil, got %s", dec.InvokeFunction.Metadata)
		}
	})

	t.Run("invoke with metadata decodes to its raw JSON", func(t *testing.T) {
		frame := `{"type":"invokefunction","function_id":"audit.log","data":{"event":"login"},"metadata":{"tenant":"acme"}}`
		dec, err := UnmarshalMessage([]byte(frame))
		if err != nil {
			t.Fatalf("UnmarshalMessage: %v", err)
		}
		if dec.InvokeFunction == nil {
			t.Fatalf("expected invokefunction variant, got %q", dec.Type)
		}
		jsonEqual(t, dec.InvokeFunction.Metadata, `{"tenant":"acme"}`)
	})

	t.Run("unregister trigger without trigger_type", func(t *testing.T) {
		// engine/src/protocol.rs:221-233.
		frame := `{"type":"unregistertrigger","id":"abc"}`
		dec, err := UnmarshalMessage([]byte(frame))
		if err != nil {
			t.Fatalf("UnmarshalMessage: %v", err)
		}
		if dec.UnregisterTrigger == nil || dec.UnregisterTrigger.ID != "abc" {
			t.Fatalf("unexpected decode: %+v", dec)
		}
		if dec.UnregisterTrigger.TriggerType != nil {
			t.Errorf("trigger_type should be nil when absent, got %v", *dec.UnregisterTrigger.TriggerType)
		}
	})

	t.Run("worker registered is engine to worker", func(t *testing.T) {
		// engine/src/protocol.rs:123-125.
		frame := `{"type":"workerregistered","worker_id":"w-42"}`
		dec, err := UnmarshalMessage([]byte(frame))
		if err != nil {
			t.Fatalf("UnmarshalMessage: %v", err)
		}
		if dec.WorkerRegistered == nil || dec.WorkerRegistered.WorkerID != "w-42" {
			t.Fatalf("unexpected decode: %+v", dec)
		}
	})

	t.Run("unknown type errors", func(t *testing.T) {
		if _, err := UnmarshalMessage([]byte(`{"type":"nope"}`)); err == nil {
			t.Error("expected error for unknown message type")
		}
	})
}

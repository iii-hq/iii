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
			// args_format, private and observable are top-level; private: false and
			// observable: true are the defaults and are omitted.
			name: "register function with the observe v2 fields",
			msg: &RegisterFunctionMessage{
				ID:         "cron::on-tick",
				ArgsFormat: json.RawMessage(`{"type":"object"}`),
				Private:    true,
				Observable: func() *bool { b := false; return &b }(),
			},
			want: `{"type":"registerfunction","id":"cron::on-tick","args_format":{"type":"object"},"private":true,"observable":false}`,
		},
		{
			name: "register function omits the defaults",
			msg:  &RegisterFunctionMessage{ID: "orders::archive"},
			want: `{"type":"registerfunction","id":"orders::archive"}`,
		},
		{
			// matched rides a routed answer from an observe.v2 host whose function has
			// a match hook (decision 55)
			name: "invocation result lists matched ids",
			msg: &InvocationResultMessage{
				InvocationID: id,
				FunctionID:   "orders::create",
				Result:       json.RawMessage(`{"ok":true}`),
				Matched:      []string{"o1", "o2"},
			},
			want: `{"type":"invocationresult","invocation_id":"11111111-1111-1111-1111-111111111111","function_id":"orders::create","result":{"ok":true},"matched":["o1","o2"]}`,
		},
		{
			// a match hook that matched none sends [], which an absent matched is not
			name: "invocation result sends an empty matched",
			msg: &InvocationResultMessage{
				InvocationID: id,
				FunctionID:   "orders::create",
				Result:       json.RawMessage(`{"ok":true}`),
				Matched:      []string{},
			},
			want: `{"type":"invocationresult","invocation_id":"11111111-1111-1111-1111-111111111111","function_id":"orders::create","result":{"ok":true},"matched":[]}`,
		},
		{
			name: "invocation result says delivered",
			msg: &InvocationResultMessage{
				InvocationID: id,
				FunctionID:   "approvals::request",
				Result:       json.RawMessage(`{"ok":true}`),
				Delivered:    true,
			},
			want: `{"type":"invocationresult","invocation_id":"11111111-1111-1111-1111-111111111111","function_id":"approvals::request","result":{"ok":true},"delivered":true}`,
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
			// request_format/response_format are optional (skip_serializing_if in the
			// engine since 0.25; it reads absent and null alike), so unset ones are omitted.
			name: "register function omits absent formats",
			msg: &RegisterFunctionMessage{
				ID: "hello::greet",
			},
			want: `{"type":"registerfunction","id":"hello::greet"}`,
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

	t.Run("0.25 registration frames are not part of the protocol", func(t *testing.T) {
		// the read loop ignores a frame it cannot decode
		for _, msgType := range []string{"registertriggertype", "unregistertriggertype", "registertrigger", "unregistertrigger", "triggerregistrationresult"} {
			if _, err := UnmarshalMessage([]byte(`{"type":"` + msgType + `","id":"abc"}`)); err == nil {
				t.Errorf("%s decoded", msgType)
			}
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

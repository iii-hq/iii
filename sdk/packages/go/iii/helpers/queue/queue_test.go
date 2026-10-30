package queue

import (
	"encoding/json"
	"testing"
)

// TestEnqueueResultJSON pins the camelCase wire name used by Node EnqueueResult
// (helpers/src/queue/index.ts) and Rust #[serde(rename = "messageReceiptId")].
func TestEnqueueResultJSON(t *testing.T) {
	var got EnqueueResult
	if err := json.Unmarshal([]byte(`{"messageReceiptId":"r-1"}`), &got); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}
	if got.MessageReceiptID != "r-1" {
		t.Fatalf("MessageReceiptID = %q, want r-1", got.MessageReceiptID)
	}
	out, err := json.Marshal(EnqueueResult{MessageReceiptID: "r-2"})
	if err != nil {
		t.Fatal(err)
	}
	if want := `{"messageReceiptId":"r-2"}`; string(out) != want {
		t.Fatalf("marshal = %s, want %s", out, want)
	}
}

// TestEnqueueResultIgnoresSnakeCase guards against a snake_case drift: the engine
// receipt is camelCase, so a snake_case key must not populate the field.
func TestEnqueueResultIgnoresSnakeCase(t *testing.T) {
	var got EnqueueResult
	if err := json.Unmarshal([]byte(`{"message_receipt_id":"r-1"}`), &got); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}
	if got.MessageReceiptID != "" {
		t.Fatalf("MessageReceiptID = %q, want empty", got.MessageReceiptID)
	}
}

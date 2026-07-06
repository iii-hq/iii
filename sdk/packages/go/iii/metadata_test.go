package iii

import (
	"context"
	"encoding/json"
	"testing"
)

// TestMetadataFromContext locks the accessor contract independently of the dispatch path:
// a bare context reports absent, withMetadata attaches a non-empty sidecar, and an empty
// sidecar is treated as absent (so it is indistinguishable from an invocation that carried
// no metadata at all).
func TestMetadataFromContext(t *testing.T) {
	t.Run("absent on a bare context", func(t *testing.T) {
		md, ok := MetadataFromContext(context.Background())
		if ok {
			t.Errorf("ok = true, want false on a context without metadata")
		}
		if md != nil {
			t.Errorf("md = %s, want nil when absent", md)
		}
	})

	t.Run("present after withMetadata", func(t *testing.T) {
		ctx := withMetadata(context.Background(), json.RawMessage(`{"tenant":"acme"}`))
		md, ok := MetadataFromContext(ctx)
		if !ok {
			t.Fatal("ok = false, want true after attaching metadata")
		}
		if string(md) != `{"tenant":"acme"}` {
			t.Errorf("md = %s, want {\"tenant\":\"acme\"}", md)
		}
	})

	t.Run("empty sidecar is treated as absent", func(t *testing.T) {
		for _, empty := range []json.RawMessage{nil, {}} {
			ctx := withMetadata(context.Background(), empty)
			if _, ok := MetadataFromContext(ctx); ok {
				t.Errorf("ok = true for empty metadata %q, want false (unchanged ctx)", empty)
			}
		}
	})
}

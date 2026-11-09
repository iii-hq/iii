package iii

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
)

// This file carries per-invocation metadata (the InvokeFunction.metadata sidecar,
// protocol.go) to a handler out-of-band on its context rather than as a positional
// argument, so adding metadata support does not break the [Handler] signature. The
// attach happens in handleInvoke; handlers read it back with MetadataFromContext.

// metadataKey is the unexported context key under which per-invocation metadata is
// stored. A dedicated unexported type makes the key un-collidable with keys from any
// other package (the standard context-key idiom).
type metadataKey struct{}

// errObserverCallMetadata is why [Client.Call] refuses a call through an observer that
// carries metadata (v2 §4.6, decision 53); the same message in every SDK.
var errObserverCallMetadata = errors.New("iii: a call through an observer takes no metadata; its target receives the observer's metadata")

// presentMetadata is the metadata a call or an invocation carries: nil when it carries
// none or JSON null, which means none (decision 54), so a handler sees the same thing
// on every path, local or routed.
func presentMetadata(metadata json.RawMessage) json.RawMessage {
	trimmed := bytes.TrimSpace(metadata)
	if len(trimmed) == 0 || bytes.Equal(trimmed, []byte("null")) {
		return nil
	}
	return metadata
}

// withMetadata attaches per-invocation metadata to ctx, but only when there is something
// to attach: a nil/empty sidecar leaves ctx untouched so MetadataFromContext reports
// "absent", matching an invocation that carried no metadata at all.
func withMetadata(ctx context.Context, metadata json.RawMessage) context.Context {
	if len(metadata) == 0 {
		return ctx
	}
	return context.WithValue(ctx, metadataKey{}, metadata)
}

// MetadataFromContext returns the per-invocation metadata attached to a handler's ctx.
// The bool is false (and the value nil) when the invocation carried no metadata, letting
// a handler distinguish "no metadata" from a present-but-empty value. Metadata is any
// JSON value; JSON null is no metadata.
//
//	func handler(ctx context.Context, data json.RawMessage) (any, error) {
//	    if md, ok := iii.MetadataFromContext(ctx); ok {
//	        // ... use md (raw JSON) ...
//	    }
//	    ...
//	}
func MetadataFromContext(ctx context.Context) (json.RawMessage, bool) {
	md, ok := ctx.Value(metadataKey{}).(json.RawMessage)
	return md, ok
}

package iii

import (
	"context"
	"encoding/json"
)

// This file carries per-invocation metadata — the InvokeFunction.metadata sidecar
// (protocol.go) — to a handler out-of-band on its context rather than as a positional
// argument, so adding metadata support does not break the [Handler] signature. The
// attach happens in handleInvoke; handlers read it back with MetadataFromContext.

// metadataKey is the unexported context key under which per-invocation metadata is
// stored. A dedicated unexported type makes the key un-collidable with keys from any
// other package (the standard context-key idiom).
type metadataKey struct{}

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
// a handler distinguish "no metadata" from a present-but-empty value.
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

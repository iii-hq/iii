package iii

import "context"

// This file is the v1 no-op OpenTelemetry seam. The wire protocol carries W3C trace
// context as the optional traceparent / baggage fields on InvokeFunction and
// InvocationResult (protocol.go); the engine and the other SDKs propagate it so a
// trace spans the engine->worker->engine hop.
//
// v1 carries those fields on the wire faithfully but does NOT wire them to an
// OpenTelemetry SDK: extracting trace context from a real otel context, and starting
// real spans around invocations, is deferred to a follow-up (open question #3 in
// iii-hq/iii#1719). Keeping the seam here — rather than scattering nils through
// client.go — means the follow-up changes only this file.

// traceContext is the pair of W3C headers carried on the wire. A zero value means "no
// trace context", which marshals to omitted fields (the pointers stay nil).
type traceContext struct {
	traceparent *string
	baggage     *string
}

// extractTraceContext pulls W3C trace context out of ctx for an outbound invocation.
// v1 is a no-op: it returns an empty traceContext, so outbound frames omit traceparent
// and baggage. The follow-up will read these from the active otel span.
func extractTraceContext(ctx context.Context) traceContext {
	_ = ctx
	return traceContext{}
}

// injectTraceContext is the inbound counterpart: given the trace context received on an
// InvokeFunction, it would attach it to ctx so a handler's own spans are children of
// the caller's. v1 is a no-op and returns ctx unchanged; the incoming traceparent /
// baggage are still echoed back on the InvocationResult by the client regardless.
func injectTraceContext(ctx context.Context, tc traceContext) context.Context {
	_ = tc
	return ctx
}

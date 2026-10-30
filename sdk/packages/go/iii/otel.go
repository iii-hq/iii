package iii

import (
	"context"

	"github.com/iii-hq/iii/sdk/packages/go/iii/internal/telemetry"
)

// This file is the trace-context seam between the wire protocol and OpenTelemetry. The
// InvokeFunction and InvocationResult frames carry W3C trace context as the optional
// traceparent / baggage fields (protocol.go); the engine and the other SDKs propagate it
// so a trace spans the engine->worker->engine hop.
//
// Propagation uses the W3C Trace Context and Baggage propagators directly, so it works
// whether or not telemetry export is enabled (WithOtel) and whatever global propagator
// the program installed: a worker that exports nothing still keeps its callers' traces
// connected.

// traceContext is the pair of W3C headers carried on the wire. A zero value means "no
// trace context", which marshals to omitted fields (the pointers stay nil).
type traceContext struct {
	traceparent *string
	baggage     *string
}

// extractTraceContext pulls W3C trace context out of ctx for an outbound invocation: the
// traceparent of the span in ctx (a handler's "execute" span, a span the caller
// started, or the remote context a handler received) and the baggage in ctx. Fields
// with nothing to carry stay nil, so the frame omits them.
func extractTraceContext(ctx context.Context) traceContext {
	if ctx == nil {
		return traceContext{}
	}
	tp, bag := telemetry.Inject(ctx)
	var tc traceContext
	if tp != "" {
		tc.traceparent = &tp
	}
	if bag != "" {
		tc.baggage = &bag
	}
	return tc
}

// injectTraceContext is the inbound counterpart: it attaches the trace context received
// on an InvokeFunction to ctx, as the remote parent span context and the baggage, so
// the handler's spans and the calls it makes join the caller's trace. The incoming
// traceparent / baggage are also echoed back on the InvocationResult by the client.
func injectTraceContext(ctx context.Context, tc traceContext) context.Context {
	var tp, bag string
	if tc.traceparent != nil {
		tp = *tc.traceparent
	}
	if tc.baggage != nil {
		bag = *tc.baggage
	}
	return telemetry.Extract(ctx, tp, bag)
}

package iii

import (
	"context"
	"encoding/json"
)

// This file is the current-invocation context (sdk.md §4.3): a context.Context value,
// set around every handler, remote (an inbound InvokeFunction) and local (in-process
// dispatch). Outgoing calls read it to send caller and observe_depth, and obs.Emit /
// obs.Call read it for the caller rules of an event. Go handlers already receive a
// ctx, so propagation is explicit: pass the handler's ctx to Trigger, Emit and Call.

type invocationKey struct{}

// invocation is the context value. Its address identifies the invocation: emits of one
// event to several observers from the same invocation coalesce into one report item.
type invocation struct {
	functionID   string
	invocationID string
	caller       *Caller
	activation   *Activation
	observeDepth uint32
	// metadata is the invocation's metadata sidecar: what an emitted event's filters
	// see as metadata.
	metadata json.RawMessage
	// trace is the W3C trace context the invocation arrived with; obs.Emit report
	// items carry it, so the target's activation joins the observed trace.
	trace traceContext
	// localDepth counts the local calls nested above this one (0 for a remote one).
	localDepth int
}

func withInvocation(ctx context.Context, inv *invocation) context.Context {
	return context.WithValue(ctx, invocationKey{}, inv)
}

func invocationFrom(ctx context.Context) *invocation {
	if ctx == nil {
		return nil
	}
	inv, _ := ctx.Value(invocationKey{}).(*invocation)
	return inv
}

// InvocationFromContext returns the invocation a handler's ctx belongs to, remote or
// local. ok is false outside any handler.
//
//	func handler(ctx context.Context, data json.RawMessage) (any, error) {
//	    if inv, ok := iii.InvocationFromContext(ctx); ok {
//	        log.Printf("%s called by %+v", inv.InvocationID, inv.Caller)
//	    }
//	    ...
//	}
func InvocationFromContext(ctx context.Context) (Invocation, bool) {
	inv := invocationFrom(ctx)
	if inv == nil {
		return Invocation{}, false
	}
	out := Invocation{
		FunctionID:   inv.functionID,
		InvocationID: inv.invocationID,
		Caller:       cloneCaller(inv.caller),
		ObserveDepth: inv.observeDepth,
	}
	if inv.activation != nil {
		a := *inv.activation
		out.Activation = &a
	}
	return out, true
}

func cloneCaller(c *Caller) *Caller {
	if c == nil {
		return nil
	}
	out := *c
	out.Parent = cloneCaller(c.Parent)
	return &out
}

// stripCaller drops parent (one level only); nil when nothing is left.
func stripCaller(c *Caller) *Caller {
	if c == nil {
		return nil
	}
	out := Caller{WorkerID: c.WorkerID, WorkerName: c.WorkerName, FunctionID: c.FunctionID, InvocationID: c.InvocationID}
	if out == (Caller{}) {
		return nil
	}
	return &out
}

// callerFor is the caller an outbound call asserts from inside a handler: that
// handler, and its own caller one level up.
func callerFor(inv *invocation) *Caller {
	return &Caller{FunctionID: inv.functionID, InvocationID: inv.invocationID, Parent: stripCaller(inv.caller)}
}

// callerDoc is a caller as a filter reads it (filters.md §1): only the fields it has.
func callerDoc(c *Caller) map[string]any {
	m := make(map[string]any, 5)
	if c.WorkerID != "" {
		m["worker_id"] = c.WorkerID
	}
	if c.WorkerName != "" {
		m["worker_name"] = c.WorkerName
	}
	if c.FunctionID != "" {
		m["function_id"] = c.FunctionID
	}
	if c.InvocationID != "" {
		m["invocation_id"] = c.InvocationID
	}
	if c.Parent != nil {
		m["parent"] = callerDoc(c.Parent)
	}
	return m
}

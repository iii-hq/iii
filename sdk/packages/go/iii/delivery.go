package iii

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"slices"
	"time"
)

// This file is the delivery of a call: every call of a function this worker hosts,
// local or routed, runs its handler and is then delivered here to the observers active
// when it started. The function's match hook narrows them. Its deliver hook, when set,
// delivers the call itself, and what the hook returns is the caller's answer. Otherwise
// iii delivers the call to the matched observers whose on and filters accept it. For a
// local call, which the engine never sees, this worker evaluates them (filter_index.go)
// and reports the observers delivered in one report item. For a routed call the engine
// evaluates them (decision 55): it holds the input, the caller, the metadata and the
// answer, and the answer says which observers it evaluates (routedAnswer).

// callState is what a call of a function reads when it starts, swapped as one pointer:
// whether Call runs the function in-process, the active observers and the hooks that
// deliver to them. A call loads it once.
type callState struct {
	// local: Call may run the function in-process (the engine confirmed it with
	// local_dispatch, and RegisterFunctionOptions.LocalDispatch allows it), in
	// namespace, the one the engine registered it in.
	local     bool
	namespace string
	observers []*Observer
	deliver   DeliverHook
	match     *MatchHook
	// index is the active observers by key, for a match hook by keys; nil otherwise.
	index *keyIndex
	// filters is the active observers by the equality conditions of their filters.
	filters *filterIndex
}

// delivering reports whether a local call is delivered at all: it has observers, or a
// deliver hook. A call that is not reads nothing else.
func (st *callState) delivering() bool { return st.deliver != nil || len(st.observers) > 0 }

// hooked reports whether the function has a match hook or a deliver hook. Its hooks see
// its calls, and to an engine with observe.v2 this worker delivers its routed calls; the
// engine delivers every other routed call to every active observer whose on and filters
// accept it.
func (st *callState) hooked() bool { return st.deliver != nil || st.match != nil }

// callDelivery is one call to deliver: the state it started with, the handler's call
// and outcome, and what a report item of it carries.
type callDelivery struct {
	state   *callState
	call    *Call
	handler outcome
	// copyRaw: the caller gets its own copy of a json.RawMessage the deliver hook
	// returns (a local call to a function that is not private).
	copyRaw      bool
	startedAt    time.Time
	duration     time.Duration
	observeDepth uint32
	trace        traceContext
}

// deliverLocal delivers a local call: the deliver hook delivers it, or iii delivers it to
// each matched observer whose on and filters accept it, in one report item. It returns
// the caller's outcome: the deliver hook's answer, or the handler's own.
func (f *functionObservers) deliverLocal(ctx context.Context, d *callDelivery) outcome {
	st, call := d.state, d.call
	if hook := st.deliver; hook != nil {
		value, err := f.runDeliverHook(ctx, hook, call, f.matching(st, call))
		return outcomeOf(value, err, d.copyRaw)
	}
	// selected starts in a buffer on the stack: the appends in the closure below would
	// make its first allocation on the heap
	var held [4]*Observer
	selected := held[:0]
	if st.match == nil {
		ok, doc := call.succeeded(), call.doc()
		st.filters.selecting(doc, st.observers, func(obs *Observer) {
			if onAccepts(obs.on, ok) && obs.filter.Matches(doc) {
				selected = append(selected, obs)
			}
		})
	} else {
		for _, obs := range f.matching(st, call) {
			if obs.Matches(call) {
				selected = append(selected, obs)
			}
		}
	}
	if len(selected) > 0 {
		f.client.reporter.push(f.reportInit(d, selected))
	}
	return d.handler
}

// routedAnswer is what an observe.v2 host's answer to a routed call says about its
// delivery (decision 55): with neither field, the engine evaluates every active observer
// of the function; matched lists the observers the match hook matched, before their on
// and filters, which the engine evaluates ([] when it matched none); delivered says the
// deliver hook delivered the call, and the engine activates nothing.
type routedAnswer struct {
	matched   []string
	delivered bool
}

// deliverRouted delivers a routed call of a function with a match hook or a deliver hook
// (hooked). The deliver hook delivers it, and its answer is the caller's; otherwise
// the answer is the handler's and lists the observers the match hook matched.
func (f *functionObservers) deliverRouted(ctx context.Context, d *callDelivery) (outcome, routedAnswer) {
	st, call := d.state, d.call
	if st.deliver == nil && len(st.observers) == 0 {
		// The match hook has no active observer to match: [] activates none, where an
		// absent matched would have the engine evaluate every observer it holds active,
		// without the hook.
		return d.handler, routedAnswer{matched: []string{}}
	}
	matched := f.matching(st, call)
	if hook := st.deliver; hook != nil {
		value, err := f.runDeliverHook(ctx, hook, call, matched)
		return outcomeOf(value, err, d.copyRaw), routedAnswer{delivered: true}
	}
	ids := make([]string, len(matched))
	for i, obs := range matched {
		ids[i] = obs.id
	}
	return d.handler, routedAnswer{matched: ids}
}

// runDeliverHook runs the function's deliver hook; a panic is the caller's error.
func (f *functionObservers) runDeliverHook(ctx context.Context, hook DeliverHook, call *Call, observers []*Observer) (value any, err error) {
	defer func() {
		if r := recover(); r != nil {
			value, err = nil, fmt.Errorf("iii: deliver hook of %q panicked: %v", f.functionID, r)
		}
	}()
	return hook(ctx, call, slices.Clone(observers))
}

// reportInit is the report item of a local call iii delivered to selected.
func (f *functionObservers) reportInit(d *callDelivery, selected []*Observer) (reportInit, []Capture) {
	call := d.call
	values := reportValues{input: call.Input, caller: reportCaller(call.Caller), metadata: call.Metadata}
	status := "success"
	if call.succeeded() {
		values.output = call.Output
	} else {
		status = "error"
		values.errBody, _ = json.Marshal(call.Error)
	}
	matched := make([]string, len(selected))
	var captures []Capture
	for i, obs := range selected {
		matched[i] = obs.id
		for _, c := range obs.capture {
			if !slices.Contains(captures, c) {
				captures = append(captures, c)
			}
		}
	}
	return reportInit{
		functionID:   f.functionID,
		namespace:    f.currentNamespace(),
		invocationID: call.InvocationID,
		matched:      matched,
		status:       status,
		values:       values,
		startedAtMs:  d.startedAt.UnixMilli(),
		durationMs:   float64(d.duration) / float64(time.Millisecond),
		observeDepth: d.observeDepth,
		trace:        d.trace,
	}, captures
}

// reportCaller is a caller as a report item carries it: the function, the invocation
// and the parent of the call's context. The engine stamps the worker itself.
func reportCaller(c *Caller) *Caller {
	if c == nil {
		return nil
	}
	out := &Caller{FunctionID: c.FunctionID, InvocationID: c.InvocationID, Parent: stripCaller(c.Parent)}
	if *out == (Caller{}) {
		return nil
	}
	return out
}

// encodeResult encodes what a handler or a deliver hook returned: a json.RawMessage is
// used as given once it is checked (copied when copyRaw is set), any other value is
// marshaled.
func encodeResult(v any, copyRaw bool) (json.RawMessage, error) {
	raw, ok := v.(json.RawMessage)
	if !ok {
		return json.Marshal(v)
	}
	if raw == nil {
		return json.RawMessage("null"), nil
	}
	if !json.Valid(raw) {
		_, err := json.Marshal(raw)
		return nil, err
	}
	if copyRaw {
		return bytes.Clone(raw), nil
	}
	return raw, nil
}

// handlerCall is the call a handler made, with the outcome its caller would get, as
// the delivery of a call that started with st hands it to the function's hooks: with a
// view for Observer.Matches when the function has a hook and observers.
func handlerCall(st *callState, functionID, invocationID string, input json.RawMessage, handler outcome, caller *Caller, metadata json.RawMessage) *Call {
	var call *Call
	if st.hooked() && len(st.observers) > 0 {
		call = newHookCall(st.filters)
	} else {
		call = new(Call)
	}
	call.FunctionID, call.InvocationID, call.Input, call.Caller, call.Metadata = functionID, invocationID, input, caller, metadata
	if handler.errBody != nil {
		call.Status = "error"
		e := *handler.errBody
		call.Error = &e
	} else {
		call.Status = "success"
		call.Output = handler.result
	}
	return call
}

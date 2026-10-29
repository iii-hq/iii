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
// iii delivers the call to the matched observers whose on and filters accept it: a
// local call in one report item, a routed call in its answer's observed list.

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
}

// delivering reports whether a call is delivered at all: it has observers, or a deliver
// hook. A call that is not reads nothing else.
func (st *callState) delivering() bool { return st.deliver != nil || len(st.observers) > 0 }

// callDelivery is one call to deliver: the state it started with, the handler's call
// and outcome, and what a report item of it carries.
type callDelivery struct {
	state   *callState
	call    *Call
	handler outcome
	// routed: the engine routed the call here, and what iii delivers rides its answer.
	routed bool
	// copyRaw: the caller gets its own copy of a json.RawMessage the deliver hook
	// returns (a local call to a function that is not private).
	copyRaw      bool
	startedAt    time.Time
	duration     time.Duration
	observeDepth uint32
	trace        traceContext
}

// deliver delivers d.call. It returns the caller's outcome: the deliver hook's answer,
// or the handler's own. For a routed call iii delivered, it also returns the ids of the
// observers the answer lists in observed.
func (f *functionObservers) deliver(ctx context.Context, d *callDelivery) (answer outcome, observed []string) {
	matched := f.matching(d.state, d.call)
	if hook := d.state.deliver; hook != nil {
		value, err := f.runDeliverHook(ctx, hook, d.call, matched)
		return outcomeOf(value, err, d.copyRaw), nil
	}
	ok := d.call.succeeded()
	doc := d.call.doc()
	var selected []*Observer
	for _, obs := range matched {
		if onAccepts(obs.on, ok) && obs.filter.Matches(doc) {
			selected = append(selected, obs)
		}
	}
	if len(selected) == 0 {
		return d.handler, nil
	}
	if d.routed {
		observed = make([]string, len(selected))
		for i, obs := range selected {
			observed[i] = obs.id
		}
		return d.handler, observed
	}
	f.client.reporter.push(f.reportInit(d, selected))
	return d.handler, nil
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

// handlerCall is the call a handler made, with the outcome its caller would get.
func handlerCall(functionID, invocationID string, input json.RawMessage, handler outcome, caller *Caller, metadata json.RawMessage) *Call {
	call := &Call{FunctionID: functionID, InvocationID: invocationID, Input: input, Caller: caller, Metadata: metadata}
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

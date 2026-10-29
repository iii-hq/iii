package iii

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"slices"
	"sync"
	"time"
)

// This file is iii's delivery: every call of a function this worker hosts, local or
// routed, runs its handler and then delivers here. The function's deliver hook, or
// next alone, chooses the observers; next applies their on and filters, delivers each
// observer at most once per call, and records what it delivered. Once the hook
// settles, the deliveries are reported: a routed call's answer lists them in observed
// when they were made with the handler's call and the answer is the handler's own
// outcome; anything else goes in invocationsobserved items, one per call next was
// given.

// deliveryKey is the context key of the delivery a deliver hook runs in.
type deliveryKey struct{}

func withDelivery(ctx context.Context, run *deliveryRun) context.Context {
	return context.WithValue(ctx, deliveryKey{}, run)
}

func deliveryFrom(ctx context.Context) *deliveryRun {
	run, _ := ctx.Value(deliveryKey{}).(*deliveryRun)
	return run
}

// delivered is what one next call delivered: the call it was given, the ids it
// matched and the parts their observers capture.
type delivered struct {
	call     *Call
	matched  []string
	captures []Capture
}

// deliveryRun is the delivery of one call.
type deliveryRun struct {
	f *functionObservers
	// call is the handler's call, handed to the deliver hook. The orig fields are its
	// values when the hook started: a call next is given is unchanged when it has them
	// all, compared as bytes.
	call         *Call
	origInput    json.RawMessage
	origOutput   json.RawMessage
	origMetadata json.RawMessage
	origStatus   string
	origError    []byte
	origCaller   []byte
	// hooked: a deliver hook runs, so next may be given another call.
	hooked bool
	// routed: the engine routed the call here and holds it until the answer.
	routed bool

	namespace    string
	startedAt    time.Time
	duration     time.Duration
	observeDepth uint32
	trace        traceContext

	mu        sync.Mutex
	settled   bool
	sent      map[string]bool
	unchanged delivered
	changed   []delivered
}

func newDeliveryRun(f *functionObservers, call *Call, hooked, routed bool) *deliveryRun {
	d := &deliveryRun{
		f:         f,
		call:      call,
		hooked:    hooked,
		routed:    routed,
		namespace: f.currentNamespace(),
		sent:      map[string]bool{},
		unchanged: delivered{call: call},
	}
	if hooked {
		d.origInput, d.origOutput, d.origMetadata, d.origStatus = call.Input, call.Output, call.Metadata, call.Status
		d.origError, d.origCaller = jsonOf(call.Error), jsonOf(call.Caller)
	}
	return d
}

// jsonOf encodes a wire struct, nil for a nil pointer.
func jsonOf[T any](v *T) []byte {
	if v == nil {
		return nil
	}
	data, _ := json.Marshal(v)
	return data
}

// isUnchanged reports whether call has the handler call's input, status, output,
// error, caller and metadata.
func (d *deliveryRun) isUnchanged(call *Call) bool {
	if !d.hooked {
		return true
	}
	return call.Status == d.origStatus &&
		bytes.Equal(call.Input, d.origInput) &&
		bytes.Equal(call.Output, d.origOutput) &&
		bytes.Equal(call.Metadata, d.origMetadata) &&
		bytes.Equal(jsonOf(call.Error), d.origError) &&
		bytes.Equal(jsonOf(call.Caller), d.origCaller)
}

// isOwnOutcome reports whether an answer is the handler's own outcome: its output, or
// the error it returned.
func (d *deliveryRun) isOwnOutcome(result json.RawMessage, errBody *ErrorBody) bool {
	if !d.hooked {
		return true
	}
	if d.call.succeeded() {
		return errBody == nil && bytes.Equal(result, d.origOutput)
	}
	return errBody != nil && bytes.Equal(jsonOf(errBody), d.origError)
}

// next is the Next a deliver hook receives.
func (d *deliveryRun) next(_ context.Context, call *Call, observers []*Observer) (any, error) {
	if call == nil {
		return nil, errors.New("iii: next: call is nil")
	}
	unchanged := d.isUnchanged(call)
	given := *call
	ok := given.succeeded()
	doc := given.doc()
	out := delivered{call: &given}

	d.mu.Lock()
	late := d.settled && d.routed
	if !late {
		for _, obs := range observers {
			if obs == nil || obs.owner != d.f || d.sent[obs.id] {
				continue
			}
			if !onAccepts(obs.on, ok) || !obs.filter.Matches(doc) {
				continue
			}
			d.sent[obs.id] = true
			out.matched = append(out.matched, obs.id)
			out.captures = addCaptures(out.captures, obs.capture)
		}
	}
	settled := d.settled
	if len(out.matched) > 0 && !settled {
		if unchanged {
			d.unchanged.matched = append(d.unchanged.matched, out.matched...)
			d.unchanged.captures = addCaptures(d.unchanged.captures, out.captures)
		} else {
			d.changed = append(d.changed, out)
		}
	}
	d.mu.Unlock()

	switch {
	case late:
		// the engine no longer holds the call: nothing can be delivered for it
		d.f.client.reporter.countDrop(d.f.functionID, dropLateDelivery)
	case settled && len(out.matched) > 0:
		// a local call's next after its hook settled is reported on its own
		if unchanged {
			out.call = d.call
		}
		d.f.client.reporter.push(d.reportInit(out))
	}
	return given.outcome()
}

func addCaptures(into, captures []Capture) []Capture {
	for _, c := range captures {
		if !slices.Contains(into, c) {
			into = append(into, c)
		}
	}
	return into
}

// deliver runs the function's delivery for the handler's call: the deliver hook, or
// next with every observer of the snapshot. It returns what the caller gets back; a
// panic in the hook is the caller's error.
func (d *deliveryRun) deliver(ctx context.Context, hook DeliverHook, observers []*Observer) (value any, err error) {
	if hook == nil {
		return d.next(ctx, d.call, observers)
	}
	defer func() {
		if r := recover(); r != nil {
			value, err = nil, fmt.Errorf("iii: deliver hook of %q panicked: %v", d.f.functionID, r)
		}
	}()
	return hook(withDelivery(ctx, d), d.call, slices.Clone(observers), d.next)
}

// settle closes the delivery: a routed call's later next delivers nothing.
func (d *deliveryRun) settle() (unchanged delivered, changed []delivered) {
	d.mu.Lock()
	defer d.mu.Unlock()
	d.settled = true
	return d.unchanged, d.changed
}

// settleLocal closes the delivery of a local call and reports what it delivered.
func (d *deliveryRun) settleLocal() {
	unchanged, changed := d.settle()
	if len(unchanged.matched) > 0 {
		d.f.client.reporter.push(d.reportInit(unchanged))
	}
	for _, dc := range changed {
		d.f.client.reporter.push(d.reportInit(dc))
	}
}

// settleRouted closes the delivery of a routed call answered with result or errBody.
// It returns the ids for the answer's observed list and the report items to send
// before the answer.
func (d *deliveryRun) settleRouted(result json.RawMessage, errBody *ErrorBody) (observed []string, items []*reportItem) {
	unchanged, changed := d.settle()
	if len(unchanged.matched) > 0 {
		if d.isOwnOutcome(result, errBody) {
			observed = unchanged.matched
		} else {
			changed = append([]delivered{unchanged}, changed...)
		}
	}
	for _, dc := range changed {
		if item := d.f.client.reporter.build(d.reportInit(dc)); item != nil {
			items = append(items, item)
		}
	}
	return observed, items
}

// reportInit is the report item of one next call.
func (d *deliveryRun) reportInit(dc delivered) (reportInit, []Capture) {
	call := dc.call
	values := reportValues{input: call.Input, caller: reportCaller(call.Caller), metadata: call.Metadata}
	status := "success"
	if call.succeeded() {
		values.output = call.Output
	} else {
		status = "error"
		body := call.Error
		if body == nil {
			body = &ErrorBody{Code: "invocation_failed", Message: "the call failed"}
		}
		values.errBody, _ = json.Marshal(body)
	}
	return reportInit{
		functionID:   d.f.functionID,
		namespace:    d.namespace,
		invocationID: d.call.InvocationID,
		matched:      dc.matched,
		status:       status,
		values:       values,
		startedAtMs:  d.startedAt.UnixMilli(),
		durationMs:   float64(d.duration) / float64(time.Millisecond),
		observeDepth: d.observeDepth,
		trace:        d.trace,
	}, dc.captures
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
// used as given once it is checked (copied when clone is set), any other value is
// marshaled.
func encodeResult(v any, clone bool) (json.RawMessage, error) {
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
	if clone {
		return bytes.Clone(raw), nil
	}
	return raw, nil
}

// handlerCall is the call a handler made: its outcome as the caller would get it.
func handlerCall(functionID, invocationID string, input json.RawMessage, result json.RawMessage, errBody *ErrorBody, caller *Caller, metadata json.RawMessage) *Call {
	call := &Call{FunctionID: functionID, InvocationID: invocationID, Input: input, Caller: caller, Metadata: metadata}
	if errBody != nil {
		call.Status = "error"
		e := *errBody
		call.Error = &e
	} else {
		call.Status = "success"
		call.Output = result
	}
	return call
}

package iii

import (
	"bytes"
	"context"
	"encoding/json"
	"log"
	"time"

	"github.com/google/uuid"
)

// This file is the local path of Call: a function this worker hosts, confirmed by the
// engine for in-process calls, runs here without touching the wire, and is delivered
// here as a routed call is. Nobody observing it means zero frames; a matched observer
// means one report item.

// localTarget returns the function Call may run in-process: registered here, the
// engine confirmed it with local_dispatch, the local-dispatch option is on, and the
// resolved namespace is the one the engine registered it in (empty: this worker's).
func (c *Client) localTarget(functionID, namespace string) (registeredFunction, bool) {
	c.mu.Lock()
	fn, ok := c.functions[functionID]
	c.mu.Unlock()
	if !ok || fn.observe == nil {
		return registeredFunction{}, false
	}
	st := fn.observe.dispatch.Load()
	if !st.enabled || (namespace != "" && namespace != st.namespace) {
		return registeredFunction{}, false
	}
	return fn, true
}

// invokeLocal runs the handler in-process, inside a new invocation context, then
// delivers the call. The handler gets a copy of the payload and the caller its own
// encoding of the result (value semantics, as over the wire), except for a private
// function, whose payload and result pass by reference. Timeouts and errors have the
// remote path's shape: ErrTimeout, and an *InvocationError carrying the error body.
func (c *Client) invokeLocal(ctx context.Context, fn registeredFunction, req CallRequest) (json.RawMessage, error) {
	data := req.payload()
	if data == nil {
		data = json.RawMessage("{}")
	}
	if !json.Valid(data) {
		_, err := json.Marshal(data) // the error the remote path gives
		return nil, err
	}
	if len(req.Metadata) > 0 && !json.Valid(req.Metadata) {
		_, err := json.Marshal(req.Metadata)
		return nil, err
	}
	f := fn.observe
	functionID := fn.message.ID
	private := f.private
	copyOf := func(raw json.RawMessage) json.RawMessage {
		if private {
			return raw
		}
		return bytes.Clone(raw)
	}

	parent := invocationFrom(ctx)
	workerID, workerName := c.identity()
	caller := &Caller{WorkerID: workerID, WorkerName: workerName}
	inv := &invocation{functionID: functionID, invocationID: uuid.NewString(), caller: caller}
	if parent != nil {
		caller.FunctionID = parent.functionID
		caller.InvocationID = parent.invocationID
		caller.Parent = stripCaller(parent.caller)
		inv.observeDepth = parent.observeDepth
		inv.trace = parent.trace
		inv.localDepth = parent.localDepth + 1
	}
	if len(req.Metadata) > 0 {
		inv.metadata = copyOf(req.Metadata)
	}

	// Delivered to the observers active when the call started: an unobserved call
	// without a deliver hook is never reported, and its input is not copied for it.
	set := f.compiledSet()
	deliver := f.deliverHook()
	delivering := deliver != nil || len(set) > 0
	input := copyOf(data)
	// what observers see is the payload as the caller passed it
	observedInput := input
	if delivering {
		observedInput = copyOf(data)
	}
	// The handler runs on the client's lifetime context, as a remote one runs on the
	// connection's: the caller's cancellation and values do not reach it.
	// The handler gets its own copy of the metadata: what it does with it cannot
	// change the report (made from inv.metadata, the snapshot taken at call start).
	// The caller's trace context does reach it (otel.go), so its span joins the trace.
	hctx := withInvocation(withMetadata(injectTraceContext(c.lifeCtx, extractTraceContext(ctx)), copyOf(inv.metadata)), inv)

	run := func() outcome {
		started := time.Now()
		var delivery *deliveryRun
		out := c.runInvocation(hctx, fn.handler, input, !private, func(ctx context.Context, handler outcome) outcome {
			duration := time.Since(started)
			c.reporter.recordLocal(functionID, handler.errBody == nil, float64(duration)/float64(time.Millisecond))
			if !delivering {
				return handler
			}
			delivery = newDeliveryRun(f, handlerCall(functionID, inv.invocationID, observedInput, handler, caller, inv.metadata), deliver != nil, false)
			delivery.startedAt, delivery.duration, delivery.observeDepth, delivery.trace = started, duration, inv.observeDepth, inv.trace
			answer := outcomeOf(delivery.deliver(ctx, deliver, set))
			if !private && deliver != nil && answer.result != nil {
				// the hook's value is the caller's own copy, as the handler's is
				answer.result = bytes.Clone(answer.result)
			}
			return answer
		})
		if delivery != nil {
			delivery.settleLocal()
		}
		return out
	}

	if req.Action != nil && req.Action.Type == "void" {
		go func() {
			if out := run(); out.errBody != nil {
				log.Printf("iii: error invoking function %q: %s: %s", functionID, out.errBody.Code, out.errBody.Message)
			}
		}()
		return nil, nil
	}

	done := make(chan outcome, 1)
	go func() { done <- run() }()
	timer := time.NewTimer(c.callTimeout(req.Timeout))
	defer timer.Stop()
	select {
	case out := <-done:
		if out.errBody != nil {
			return nil, newInvocationError(out.errBody, functionID)
		}
		return out.result, nil
	case <-timer.C:
		return nil, ErrTimeout
	case <-ctx.Done():
		return nil, ctx.Err()
	case <-c.shutdown:
		return nil, ErrNotConnected
	}
}

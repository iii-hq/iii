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
// here: this worker evaluates its observers' capture and filters, which the engine never
// sees. Nobody observing it means zero frames; matched observers mean one report item.

// localTarget returns the function Call may run in-process, with the call state it
// starts with: registered here, the engine confirmed it with local_dispatch, the
// local-dispatch option is on, and the resolved namespace is the one the engine
// registered it in (empty: this worker's).
func (c *Client) localTarget(functionID, namespace string) (registeredFunction, *callState, bool) {
	c.mu.Lock()
	fn, ok := c.functions[functionID]
	c.mu.Unlock()
	if !ok || fn.observe == nil {
		return registeredFunction{}, nil, false
	}
	st := fn.observe.state.Load()
	if !st.local || (namespace != "" && namespace != st.namespace) {
		return registeredFunction{}, nil, false
	}
	return fn, st, true
}

// invokeLocal runs the handler in-process, inside a new invocation context, then
// delivers the call with st, the call state it started with. The handler gets a copy of
// the payload and the caller its own encoding of the result (value semantics, as over
// the wire), except for a private function, whose payload and result pass by
// reference. Timeouts and errors have the remote path's shape: ErrTimeout, and an
// *InvocationError carrying the error body.
//
// Streams pass as they are: the handler reads the caller's source, or writes into the
// caller's sink, with no pipe. The call returns once the sinks it passed ended, by
// deadline (R4).
func (c *Client) invokeLocal(ctx context.Context, fn registeredFunction, st *callState, req CallRequest, deadline time.Time) (json.RawMessage, error) {
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

	// Delivered to the observers active when the call started, by the hooks set then:
	// an unobserved call without a deliver hook is never reported, and its input is not
	// copied for it.
	delivering := st.delivering()
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

	void := req.Action != nil && req.Action.Type == "void"
	run := func() outcome {
		started := time.Now()
		return c.runInvocation(hctx, fn.handler, input, !private, streamCall{answered: !void}, func(ctx context.Context, handler outcome) outcome {
			duration := time.Since(started)
			c.reporter.recordLocal(functionID, handler.errBody == nil, float64(duration)/float64(time.Millisecond))
			if !delivering {
				return handler
			}
			return f.deliverLocal(ctx, &callDelivery{
				state:        st,
				call:         handlerCall(st, functionID, inv.invocationID, observedInput, handler, caller, inv.metadata),
				handler:      handler,
				copyRaw:      !private,
				startedAt:    started,
				duration:     duration,
				observeDepth: inv.observeDepth,
				trace:        inv.trace,
			})
		})
	}

	if void {
		go func() {
			if out := run(); out.errBody != nil {
				log.Printf("iii: error invoking function %q: %s: %s", functionID, out.errBody.Code, out.errBody.Message)
			}
		}()
		return nil, nil
	}

	// The sinks the caller passes: the call returns once they ended.
	sinks := localSinks(data)
	done := make(chan outcome, 1)
	go func() { done <- run() }()
	timer := time.NewTimer(c.callTimeout(req.Timeout))
	defer timer.Stop()
	var result json.RawMessage
	var err error
	select {
	case out := <-done:
		if out.errBody != nil {
			err = newInvocationError(out.errBody, functionID)
		} else {
			result = out.result
		}
	case <-timer.C:
		err = ErrTimeout
	case <-ctx.Done():
		err = ctx.Err()
	case <-c.shutdown:
		err = ErrNotConnected
	}
	if result, err = settleLocalSinks(ctx, sinks, result, err, deadline); err != nil {
		return nil, err
	}
	c.trackResult(result)
	return result, nil
}

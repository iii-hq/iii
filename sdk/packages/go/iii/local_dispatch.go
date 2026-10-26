package iii

import (
	"bytes"
	"context"
	"encoding/json"
	"log"
	"time"

	"github.com/google/uuid"
)

// This file is the local path of Trigger (sdk.md §3, §4.3): a function this worker
// hosts, confirmed by the engine for in-process calls, runs here without touching the
// wire. Nobody observing it means zero frames; a matched observer means one report item.

// localTarget returns the function Trigger may run in-process: registered here, the
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

type localOutcome struct {
	result json.RawMessage
	err    *ErrorBody
}

// invokeLocal runs the handler in-process, inside a new invocation context, with a
// copy of the payload (value semantics, as over the wire); the caller gets a fresh
// encoding of the result. Timeouts and errors have the remote path's shape: ErrTimeout,
// and an *InvocationError carrying the handler's error body.
func (c *Client) invokeLocal(ctx context.Context, fn registeredFunction, req TriggerRequest) (json.RawMessage, error) {
	data := req.Data
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
	obs := fn.observe
	functionID := fn.message.ID

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
		inv.metadata = bytes.Clone(req.Metadata)
	}

	// Evaluated against the observers active when the call started: an unobserved
	// call is never reported, and its input is not copied for it.
	set := obs.compiledSet()
	var observedInput json.RawMessage
	if obs.delivery == DeliveryAuto && len(set) > 0 {
		// what observers see is the payload as the caller passed it
		observedInput = bytes.Clone(data)
	}
	input := bytes.Clone(data)
	// The handler runs on the client's lifetime context, as a remote one runs on the
	// connection's: the caller's cancellation and values do not reach it.
	// The handler gets its own copy of the metadata: what it does with it cannot
	// change the report (made from inv.metadata, the snapshot taken at call start).
	hctx := withInvocation(withMetadata(c.lifeCtx, bytes.Clone(inv.metadata)), inv)

	run := func() localOutcome {
		started := time.Now()
		var out localOutcome
		result, herr := c.runHandler(hctx, fn.handler, input)
		if herr != nil {
			out.err = errorBodyFromHandlerError(herr)
		} else if encoded, err := json.Marshal(result); err != nil {
			out.err = &ErrorBody{Code: "invocation_failed", Message: err.Error()}
		} else {
			out.result = encoded
		}
		duration := time.Since(started)
		c.reporter.recordLocal(functionID, out.err == nil, float64(duration)/float64(time.Millisecond))
		if len(set) > 0 {
			obs.reportLocal(set, localCall{
				invocationID: inv.invocationID,
				ok:           out.err == nil,
				input:        observedInput,
				output:       out.result,
				err:          out.err,
				caller:       caller,
				metadata:     inv.metadata,
				startedAt:    started,
				duration:     duration,
				observeDepth: inv.observeDepth,
				trace:        inv.trace,
			})
		}
		return out
	}

	if req.Action != nil && req.Action.Type == "void" {
		go func() {
			if out := run(); out.err != nil {
				log.Printf("iii: error invoking function %q: %s: %s", functionID, out.err.Code, out.err.Message)
			}
		}()
		return nil, nil
	}

	done := make(chan localOutcome, 1)
	go func() { done <- run() }()
	timer := time.NewTimer(c.triggerTimeout(req.Timeout))
	defer timer.Stop()
	select {
	case out := <-done:
		if out.err != nil {
			return nil, newInvocationError(out.err, functionID)
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

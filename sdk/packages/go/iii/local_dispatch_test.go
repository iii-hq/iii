package iii

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/google/uuid"
)

// registerLocal registers fn and waits until the engine confirmed it for local dispatch.
func registerLocal(t *testing.T, c *Client, id string, h Handler, opts ...RegisterFunctionOptions) {
	t.Helper()
	if err := c.RegisterFunction(id, h, opts...); err != nil {
		t.Fatal(err)
	}
	waitUntil(t, "local dispatch for "+id, func() bool { return c.LocalDispatchActive(id) })
}

func trigger(t *testing.T, c *Client, req TriggerRequest) json.RawMessage {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	out, err := c.Trigger(ctx, req)
	if err != nil {
		t.Fatalf("Trigger(%s): %v", req.FunctionID, err)
	}
	return out
}

func invokeFrames(fe *fakeEngine, fn string) int {
	n := 0
	for _, d := range fe.frames(MsgInvokeFunction) {
		if d.InvokeFunction.FunctionID == fn {
			n++
		}
	}
	return n
}

// sdk.md §3: a confirmed function this worker hosts runs in-process and puts nothing
// on the wire; enqueue, Dispatch remote, LocalDispatch false and another namespace go
// through the engine.
func TestLocalDispatchRunsInProcess(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	var calls atomic.Int32
	registerLocal(t, c, "math::inc", func(_ context.Context, data json.RawMessage) (any, error) {
		calls.Add(1)
		var n int
		_ = json.Unmarshal(data, &n)
		return n + 1, nil
	})
	mark := fe.mark()
	if out := trigger(t, c, TriggerRequest{FunctionID: "math::inc", Data: json.RawMessage(`41`)}); string(out) != "42" {
		t.Fatalf("result %s", out)
	}
	if out := trigger(t, c, TriggerRequest{FunctionID: "math::inc", Data: json.RawMessage(`1`), Namespace: "default"}); string(out) != "2" {
		t.Fatalf("result %s (the namespace it registered in stays local)", out)
	}
	time.Sleep(20 * time.Millisecond)
	if n := fe.countFrames(mark); n != 0 || calls.Load() != 2 {
		t.Fatalf("%d frames, %d calls", n, calls.Load())
	}

	short := func(req TriggerRequest) {
		ctx, cancel := context.WithTimeout(context.Background(), 30*time.Millisecond)
		defer cancel()
		_, _ = c.Trigger(ctx, req) // the fake engine does not answer
	}
	short(TriggerRequest{FunctionID: "math::inc", Dispatch: DispatchRemote})
	short(TriggerRequest{FunctionID: "math::inc", Action: EnqueueAction("q")})
	short(TriggerRequest{FunctionID: "math::inc", Namespace: "elsewhere"})
	off := false
	registerLocal(t, c, "math::remote-only", echoHandler)
	_ = c.RegisterFunction("math::remote-only", echoHandler, RegisterFunctionOptions{LocalDispatch: &off})
	waitUntil(t, "confirmed", func() bool { return c.functionObservers("math::remote-only").syncedOn(c.currentEpoch()) })
	short(TriggerRequest{FunctionID: "math::remote-only"})
	waitUntil(t, "remote frames", func() bool { return invokeFrames(fe, "math::inc") == 3 && invokeFrames(fe, "math::remote-only") == 1 })
	if calls.Load() != 2 {
		t.Errorf("handler ran %d times; the remote calls must not run it here", calls.Load())
	}
	if _, err := c.Trigger(context.Background(), TriggerRequest{FunctionID: "math::inc", Dispatch: "sideways"}); err == nil {
		t.Error("unknown Dispatch accepted")
	}
}

func TestLocalDispatchNeedsTheEngineConfirmation(t *testing.T) {
	fe := newFakeEngine(t)
	fe.mu.Lock()
	fe.localDispatch = false
	fe.mu.Unlock()
	c := fe.client()
	_ = c.RegisterFunction("math::inc", echoHandler)
	waitUntil(t, "confirmed", func() bool { return c.functionObservers("math::inc").syncedOn(c.currentEpoch()) })
	if c.LocalDispatchActive("math::inc") {
		t.Fatal("local dispatch with functionregistered{local_dispatch: false}")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Millisecond)
	defer cancel()
	_, _ = c.Trigger(ctx, TriggerRequest{FunctionID: "math::inc"})
	waitUntil(t, "remote frame", func() bool { return invokeFrames(fe, "math::inc") == 1 })
}

func TestLocalVoidIsFireAndForget(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	ran := make(chan struct{})
	release := make(chan struct{})
	registerLocal(t, c, "jobs::run", func(context.Context, json.RawMessage) (any, error) {
		close(ran)
		<-release
		return nil, nil
	})
	out, err := c.Trigger(context.Background(), TriggerRequest{FunctionID: "jobs::run", Action: VoidAction()})
	if err != nil || out != nil {
		t.Fatalf("void: %s %v", out, err)
	}
	<-ran
	close(release)
	if n := invokeFrames(fe, "jobs::run"); n != 0 {
		t.Errorf("%d frames", n)
	}
}

// sdk.md §4.5: once confirmed, local dispatch keeps serving while disconnected.
func TestLocalDispatchSurvivesADisconnect(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	registerLocal(t, c, "math::inc", echoHandler)
	gate := make(chan struct{})
	fe.mu.Lock()
	fe.gate = gate
	fe.mu.Unlock()
	fe.m.closeActiveConnection()
	waitUntil(t, "offline", func() bool { return !c.registeredNow() })
	if out := trigger(t, c, TriggerRequest{FunctionID: "math::inc", Data: json.RawMessage(`7`)}); string(out) != "7" {
		t.Errorf("result %s", out)
	}
	close(gate)
}

// README §11.8: a handler that mutates its input does not affect the caller's value,
// and the reverse.
func TestLocalValueSemantics(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	var mu sync.Mutex
	var kept json.RawMessage
	var keptMap map[string]any
	registerLocal(t, c, "vals::mutate", func(_ context.Context, data json.RawMessage) (any, error) {
		mu.Lock()
		defer mu.Unlock()
		var m map[string]any
		_ = json.Unmarshal(data, &m)
		kept, keptMap = data, m
		data[2] = 'X' // the handler mutates its input
		return m, nil
	})
	in := json.RawMessage(`{"a":[1,2]}`)
	out := trigger(t, c, TriggerRequest{FunctionID: "vals::mutate", Data: in})
	if string(in) != `{"a":[1,2]}` {
		t.Errorf("the handler's mutation reached the caller: %s", in)
	}
	in[2] = 'Z'  // the caller mutates its payload after the call
	out[2] = 'Q' // and the result
	mu.Lock()
	defer mu.Unlock()
	if string(kept) != `{"X":[1,2]}` {
		t.Errorf("the caller's mutation reached the handler's input: %s", kept)
	}
	if _, ok := keptMap["a"]; !ok || len(keptMap) != 1 {
		t.Errorf("the caller's mutation reached the handler's result: %v", keptMap)
	}
	if string(out) != `{"Q":[1,2]}` {
		t.Errorf("result %s", out)
	}
}

// Errors and timeouts on the local path look as on the remote path.
func TestLocalErrorsAndTimeoutsMatchTheRemotePath(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	registerLocal(t, c, "err::typed", func(context.Context, json.RawMessage) (any, error) {
		return nil, &InvocationError{Code: "CARD_DECLINED", Message: "declined", Stacktrace: "at charge()"}
	})
	registerLocal(t, c, "err::plain", func(context.Context, json.RawMessage) (any, error) { return nil, errors.New("boom") })
	registerLocal(t, c, "err::panic", func(context.Context, json.RawMessage) (any, error) { panic("kaput") })
	registerLocal(t, c, "err::slow", func(ctx context.Context, _ json.RawMessage) (any, error) {
		<-ctx.Done()
		return nil, nil
	})
	var ie *InvocationError
	_, err := c.Trigger(context.Background(), TriggerRequest{FunctionID: "err::typed"})
	if !errors.As(err, &ie) || ie.Code != "CARD_DECLINED" || ie.Message != "declined" || ie.Stacktrace != "at charge()" || ie.FunctionID != "err::typed" {
		t.Errorf("typed: %#v", err)
	}
	_, err = c.Trigger(context.Background(), TriggerRequest{FunctionID: "err::plain"})
	if !errors.As(err, &ie) || ie.Code != "invocation_failed" || ie.Message != "boom" {
		t.Errorf("plain: %#v", err)
	}
	_, err = c.Trigger(context.Background(), TriggerRequest{FunctionID: "err::panic"})
	if !errors.As(err, &ie) || ie.Code != "invocation_failed" {
		t.Errorf("panic: %#v", err)
	}
	if _, err = c.Trigger(context.Background(), TriggerRequest{FunctionID: "err::slow", Timeout: 20 * time.Millisecond}); !errors.Is(err, ErrTimeout) {
		t.Errorf("timeout: %v", err)
	}
	if _, err = c.Trigger(context.Background(), TriggerRequest{FunctionID: "err::plain", Data: json.RawMessage(`{`)}); err == nil {
		t.Error("invalid JSON payload accepted")
	}
}

// sdk.md §4.3: the invocation context in local handlers and the caller chain of nested
// local calls.
func TestInvocationContextOfLocalCalls(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	if _, ok := InvocationFromContext(context.Background()); ok {
		t.Fatal("an invocation outside any handler")
	}
	got := make(chan Invocation, 2)
	registerLocal(t, c, "chain::inner", func(ctx context.Context, _ json.RawMessage) (any, error) {
		inv, _ := InvocationFromContext(ctx)
		got <- inv
		return nil, nil
	})
	registerLocal(t, c, "chain::outer", func(ctx context.Context, _ json.RawMessage) (any, error) {
		inv, _ := InvocationFromContext(ctx)
		got <- inv
		return c.Trigger(ctx, TriggerRequest{FunctionID: "chain::inner"})
	})
	trigger(t, c, TriggerRequest{FunctionID: "chain::outer"})
	outer, inner := <-got, <-got
	if outer.FunctionID != "chain::outer" || outer.Caller == nil || outer.Caller.FunctionID != "" || outer.Caller.WorkerName != "observe-test" || outer.Caller.WorkerID != "worker-1" {
		t.Errorf("outer %+v caller %+v", outer, outer.Caller)
	}
	if _, err := uuid.Parse(outer.InvocationID); err != nil {
		t.Errorf("invocation id %q", outer.InvocationID)
	}
	if inner.Caller.FunctionID != "chain::outer" || inner.Caller.InvocationID != outer.InvocationID || inner.Caller.Parent == nil ||
		inner.Caller.Parent.WorkerName != "observe-test" || inner.Caller.Parent.Parent != nil {
		t.Errorf("inner caller %+v parent %+v", inner.Caller, inner.Caller.Parent)
	}
}

// SDK-PARITY decision 20: a local call nested more than 64 local calls deep goes
// through the engine.
func TestLocalReentrancyGuard(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	var depth atomic.Int32
	registerLocal(t, c, "rec::down", func(ctx context.Context, data json.RawMessage) (any, error) {
		var n int
		_ = json.Unmarshal(data, &n)
		depth.Store(int32(n))
		if n == 70 {
			return n, nil
		}
		cctx, cancel := context.WithTimeout(ctx, 100*time.Millisecond)
		defer cancel()
		return c.Trigger(cctx, TriggerRequest{FunctionID: "rec::down", Data: json.RawMessage(fmt.Sprint(n + 1))})
	})
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	_, _ = c.Trigger(ctx, TriggerRequest{FunctionID: "rec::down", Data: json.RawMessage(`0`)})
	if depth.Load() != maxLocalDepth {
		t.Errorf("ran %d levels locally, want %d", depth.Load(), maxLocalDepth)
	}
	waitUntil(t, "the next call went remote", func() bool { return invokeFrames(fe, "rec::down") == 1 })
}

// sdk.md §4.3, CONTRACT D16: an inbound activation sets the context, is answered, and
// the calls made from it carry caller and observe_depth; depth 0 is omitted, and
// nothing is sent outside a handler or to an engine without observe.v1.
func TestRemoteHandlerContextAndCallerPropagation(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	got := make(chan Invocation, 1)
	_ = c.RegisterFunction("alerts::notify", func(ctx context.Context, _ json.RawMessage) (any, error) {
		inv, _ := InvocationFromContext(ctx)
		got <- inv
		cctx, cancel := context.WithTimeout(ctx, 30*time.Millisecond)
		defer cancel()
		_, _ = c.Trigger(cctx, TriggerRequest{FunctionID: "pager::page", Dispatch: DispatchRemote})
		return "done", nil
	})
	var activation map[string]json.RawMessage
	_ = json.Unmarshal(observeFixture(t, "invokefunction-activation.json"), &activation)
	activation["function_id"] = json.RawMessage(`"alerts::notify"`)
	frame, _ := json.Marshal(activation)
	fe.sendRaw(string(frame))
	inv := <-got
	if inv.Activation == nil || inv.Activation.ObserverID != "c81e2f4a-5b6c-4d7e-9f0a-1b2c3d4e5f60" || inv.ObserveDepth != 1 ||
		inv.Caller.WorkerName != "iii-engine" || inv.InvocationID != "f1a3c5e7-9b1d-4f3a-8c5e-7a9b1d3f5a7c" {
		t.Errorf("activation context %+v", inv)
	}
	res := fe.waitFrames(MsgInvocationResult, 1, nil)[0].InvocationResult
	if res.InvocationID.String() != "f1a3c5e7-9b1d-4f3a-8c5e-7a9b1d3f5a7c" || string(res.Result) != `"done"` {
		t.Errorf("activation answered with %+v", res)
	}
	out := fe.waitFrames(MsgInvokeFunction, 1, func(d *DecodedMessage) bool { return d.InvokeFunction.FunctionID == "pager::page" })[0].InvokeFunction
	if out.ObserveDepth != 1 || out.Caller == nil || out.Caller.FunctionID != "alerts::notify" ||
		out.Caller.InvocationID != inv.InvocationID || out.Caller.Parent == nil || out.Caller.Parent.WorkerName != "iii-engine" {
		t.Errorf("outgoing caller %+v depth %d", out.Caller, out.ObserveDepth)
	}

	// outside any handler: no caller, no depth
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Millisecond)
	defer cancel()
	_, _ = c.Trigger(ctx, TriggerRequest{FunctionID: "pager::other"})
	plain := fe.waitFrames(MsgInvokeFunction, 1, func(d *DecodedMessage) bool { return d.InvokeFunction.FunctionID == "pager::other" })[0]
	raw, _ := MarshalMessage(plain.InvokeFunction)
	var fields map[string]json.RawMessage
	_ = json.Unmarshal(raw, &fields)
	if _, ok := fields["caller"]; ok {
		t.Error("caller sent outside a handler")
	}
	if _, ok := fields["observe_depth"]; ok {
		t.Error("observe_depth 0 sent")
	}
}

func TestNoCallerToAnEngineWithoutObserve(t *testing.T) {
	fe := newFakeEngine(t).oldEngine()
	c := fe.client()
	_ = c.RegisterFunction("old::fn", func(ctx context.Context, _ json.RawMessage) (any, error) {
		cctx, cancel := context.WithTimeout(ctx, 30*time.Millisecond)
		defer cancel()
		_, _ = c.Trigger(cctx, TriggerRequest{FunctionID: "old::other"})
		return nil, nil
	})
	id := uuid.New()
	fe.send(&InvokeFunctionMessage{InvocationID: &id, FunctionID: "old::fn", Data: json.RawMessage(`{}`)})
	out := fe.waitFrames(MsgInvokeFunction, 1, func(d *DecodedMessage) bool { return d.InvokeFunction.FunctionID == "old::other" })[0].InvokeFunction
	if out.Caller != nil || out.ObserveDepth != 0 {
		t.Errorf("caller %+v depth %d sent to an old engine", out.Caller, out.ObserveDepth)
	}
}

// autoObserver adds an observer of an auto function and waits for its ack.
func autoObserver(t *testing.T, fe *fakeEngine, fn string, info ObserverInfo) {
	t.Helper()
	rev := fe.add(fn, info)
	if a := fe.waitAck(fn, info.ID, rev); a.Error != nil {
		t.Fatalf("%s rejected: %+v", info.ID, a.Error)
	}
}

// sdk.md §4.3: a matched local call of an auto function is reported once, with the
// union of the matched observers' captures; on: error sees only failures.
func TestLocalCallsOfAutoFunctionsAreReported(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	registerLocal(t, c, "pay::charge", func(_ context.Context, data json.RawMessage) (any, error) {
		var in struct {
			Amount int  `json:"amount"`
			Fail   bool `json:"fail"`
		}
		_ = json.Unmarshal(data, &in)
		if in.Fail {
			return nil, &InvocationError{Code: "CARD_DECLINED", Message: "declined"}
		}
		return map[string]any{"charged": in.Amount}, nil
	})
	big := observerInfo("big", "x::y", "", `{"$and":[{"input.amount":{"$gte":500}}]}`, CaptureInput)
	caller := observerInfo("by-worker", "x::y", "", `{"$and":[{"caller.worker_name":{"$eq":"observe-test"}},{"output.charged":{"$gt":0}}]}`, CaptureOutput, CaptureMetadata)
	failures := observerInfo("failures", "x::y", "", "", CaptureInput, CaptureError, CaptureCaller)
	failures.On = OnError
	for _, o := range []ObserverInfo{big, caller, failures} {
		autoObserver(t, fe, "pay::charge", o)
	}

	trigger(t, c, TriggerRequest{FunctionID: "pay::charge", Data: json.RawMessage(`{"amount":750}`), Metadata: json.RawMessage(`{"channel":"web"}`)})
	items := fe.waitItems(1)
	it := items[0]
	if len(it.Matched) != 2 || it.Matched[0] != "big" || it.Matched[1] != "by-worker" || it.Status != "success" {
		t.Errorf("matched %v status %s", it.Matched, it.Status)
	}
	if string(it.Input) != `{"amount":750}` || string(it.Output) != `{"charged":750}` || string(it.Metadata) != `{"channel":"web"}` || it.Caller != nil || it.Error != nil {
		t.Errorf("parts: input %s output %s metadata %s caller %+v", it.Input, it.Output, it.Metadata, it.Caller)
	}
	if it.FunctionID != "pay::charge" || it.Namespace != "default" || it.ObserveDepth != 0 || it.StartedAtMs == 0 {
		t.Errorf("item %+v", it)
	}

	_, err := c.Trigger(context.Background(), TriggerRequest{FunctionID: "pay::charge", Data: json.RawMessage(`{"amount":10,"fail":true}`)})
	if err == nil {
		t.Fatal("no error")
	}
	items = fe.waitItems(2)
	fail := items[1]
	if len(fail.Matched) != 1 || fail.Matched[0] != "failures" || fail.Status != "error" || fail.Output != nil {
		t.Errorf("failure item %+v", fail)
	}
	if string(fail.Error) != `{"code":"CARD_DECLINED","message":"declined"}` || fail.Caller == nil || fail.Caller.WorkerName != "observe-test" {
		t.Errorf("failure parts: error %s caller %+v", fail.Error, fail.Caller)
	}

	// no filter matches: nothing reported
	trigger(t, c, TriggerRequest{FunctionID: "pay::charge", Data: json.RawMessage(`{"amount":0}`)})
	time.Sleep(30 * time.Millisecond)
	if n := len(fe.reportItems()); n != 2 {
		t.Errorf("%d items, want 2", n)
	}
}

// SDK-PARITY decision 14: a call is evaluated against the observers active when it
// started.
func TestLocalCallUsesTheSetCapturedAtCallStart(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	started := make(chan struct{})
	release := make(chan struct{})
	registerLocal(t, c, "slow::fn", func(context.Context, json.RawMessage) (any, error) {
		close(started)
		<-release
		return 1, nil
	})
	done := make(chan struct{})
	go func() {
		defer close(done)
		trigger(t, c, TriggerRequest{FunctionID: "slow::fn"})
	}()
	<-started
	autoObserver(t, fe, "slow::fn", observerInfo("late", "x::y", "", ""))
	close(release)
	<-done
	time.Sleep(30 * time.Millisecond)
	if n := len(fe.reportItems()); n != 0 {
		t.Errorf("%d items for a call that started unobserved", n)
	}
}

// sdk.md §7 Zero-wire: local calls of an unobserved function send zero frames
// (localinvocationstats, the periodic counter, aside).
func TestZeroWireUnobservedLocalCalls(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	registerLocal(t, c, "zw::inc", func(context.Context, json.RawMessage) (any, error) { return 1, nil })
	calls := 1_000_000
	if raceEnabled || testing.Short() {
		calls = 50_000
	}
	mark := fe.mark()
	ctx := context.Background()
	req := TriggerRequest{FunctionID: "zw::inc", Data: json.RawMessage(`1`)}
	start := time.Now()
	for i := 0; i < calls; i++ {
		if _, err := c.Trigger(ctx, req); err != nil {
			t.Fatal(err)
		}
	}
	elapsed := time.Since(start)
	time.Sleep(20 * time.Millisecond)
	if n := fe.countFrames(mark, MsgLocalInvocationStats); n != 0 {
		t.Fatalf("%d frames for %d unobserved local calls", n, calls)
	}
	t.Logf("%d local calls in %s (%.2f µs/call)", calls, elapsed, float64(elapsed.Microseconds())/float64(calls))

	// positive control: the same harness sees the frames of an observed function
	registerLocal(t, c, "zw::observed", func(context.Context, json.RawMessage) (any, error) { return 1, nil })
	autoObserver(t, fe, "zw::observed", observerInfo("o1", "x::y", "", "", CaptureInput))
	mark = fe.mark()
	for i := 0; i < 10; i++ {
		if _, err := c.Trigger(ctx, TriggerRequest{FunctionID: "zw::observed", Data: json.RawMessage(`1`)}); err != nil {
			t.Fatal(err)
		}
	}
	waitUntil(t, "the observed calls' reports", func() bool { return fe.countFrames(mark, MsgLocalInvocationStats) > 0 })
	if n := len(fe.reportItems()); n != 10 {
		t.Errorf("%d report items for 10 observed calls", n)
	}
}

// SDK-PARITY decision 22: reports held for a function whose replayed registration is
// refused (FUNCTION_NAMESPACE_CONFLICT) are discarded and counted as INACTIVE.
func TestNamespaceConflictOnReplayDiscardsHeldReports(t *testing.T) {
	fe := newFakeEngine(t)
	fe.setObservers("held::fn", 1, observerInfo("o1", "x::y", "", "", CaptureInput))
	c := fe.client()
	registerLocal(t, c, "held::fn", echoHandler)
	fe.waitAck("held::fn", "o1", 1)

	gate := make(chan struct{})
	fe.mu.Lock()
	fe.gate, fe.confirm = gate, false
	fe.mu.Unlock()
	fe.setOnFrame(func(msg map[string]json.RawMessage) {
		if messageType(msg) == string(MsgRegisterFunction) && messageID(msg) == "held::fn" {
			fe.send(&RegistrationRejectedMessage{Code: "FUNCTION_NAMESPACE_CONFLICT", Namespace: "default", FunctionID: "held::fn", OwnerWorkerID: "other"})
		}
	})
	fe.m.closeActiveConnection()
	waitUntil(t, "offline", func() bool { return !c.registeredNow() })
	trigger(t, c, TriggerRequest{FunctionID: "held::fn", Data: json.RawMessage(`"held"`)}) // still local
	waitUntil(t, "held", func() bool { return c.reporter.size() == 1 })
	close(gate)
	waitUntil(t, "discarded", func() bool { return c.reporter.size() == 0 && !c.LocalDispatchActive("held::fn") })
	if !c.reporter.sendStats(time.Now()) {
		t.Fatal("no stats")
	}
	stats := fe.waitFrames(MsgLocalInvocationStats, 1, nil)[0].LocalInvocationStats
	if len(stats.DroppedReports) != 1 || stats.DroppedReports[0].Reason != "INACTIVE" || stats.DroppedReports[0].Count != 1 {
		t.Errorf("drops %+v", stats.DroppedReports)
	}
	if n := len(fe.reportItems()); n != 0 {
		t.Errorf("%d items reported for a refused function", n)
	}
}

// SDK-PARITY decision 15: localinvocationstats counts every local call, every
// interval, only when non-zero.
func TestLocalInvocationStatsCountEveryLocalCall(t *testing.T) {
	fe := newFakeEngine(t)
	c := New(fe.m.url, WithName("observe-test"))
	c.reporter.limits.statsInterval = 40 * time.Millisecond
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	if err := c.Connect(ctx); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = c.Close() })
	registerLocal(t, c, "st::ok", echoHandler)
	registerLocal(t, c, "st::err", func(context.Context, json.RawMessage) (any, error) { return nil, errors.New("x") })
	time.Sleep(100 * time.Millisecond)
	if n := len(fe.frames(MsgLocalInvocationStats)); n != 0 {
		t.Fatalf("%d stats frames with nothing to report", n)
	}
	for i := 0; i < 3; i++ {
		trigger(t, c, TriggerRequest{FunctionID: "st::ok"})
	}
	_, _ = c.Trigger(ctx, TriggerRequest{FunctionID: "st::err"})
	stats := fe.waitFrames(MsgLocalInvocationStats, 1, nil)[0].LocalInvocationStats
	byFn := map[string]LocalFunctionStats{}
	for _, f := range stats.Functions {
		byFn[f.FunctionID] = f
	}
	if byFn["st::ok"].Count != 3 || byFn["st::ok"].Errors != 0 || byFn["st::err"].Count != 1 || byFn["st::err"].Errors != 1 || stats.WindowMs == 0 {
		t.Errorf("stats %+v", stats)
	}
	time.Sleep(120 * time.Millisecond)
	if n := len(fe.frames(MsgLocalInvocationStats)); n != 1 {
		t.Errorf("%d stats frames, want 1 (only when non-zero)", n)
	}
}

// sdk.md §7 Reconnect: reports buffered while disconnected flush after the reconnect,
// once the engine confirmed the function on the new connection.
func TestReportsHeldUntilTheFunctionIsConfirmedAgain(t *testing.T) {
	fe := newFakeEngine(t)
	fe.setObservers("rc::fn", 1, observerInfo("o1", "x::y", "", "", CaptureInput))
	c := fe.client()
	registerLocal(t, c, "rc::fn", echoHandler)
	fe.waitAck("rc::fn", "o1", 1)
	gate := make(chan struct{})
	fe.mu.Lock()
	fe.gate = gate
	fe.mu.Unlock()
	fe.m.closeActiveConnection()
	waitUntil(t, "offline", func() bool { return !c.registeredNow() })
	trigger(t, c, TriggerRequest{FunctionID: "rc::fn", Data: json.RawMessage(`"while away"`)})
	time.Sleep(20 * time.Millisecond)
	if c.reporter.size() != 1 {
		t.Fatalf("buffered %d", c.reporter.size())
	}
	close(gate)
	items := fe.waitItems(1)
	if string(items[0].Input) != `"while away"` {
		t.Errorf("item %+v", items[0])
	}
	// it went after the replayed observer was acked again on the new connection
	msgs := fe.m.receivedMessages()
	var acks []int
	for i, m := range msgs {
		if messageType(m) == string(MsgObserverAck) {
			acks = append(acks, i)
		}
	}
	if len(acks) < 2 || indexOfType(msgs, string(MsgInvocationsObserved)) < acks[1] {
		t.Errorf("report at %d, acks at %v: the report must follow the re-ack", indexOfType(msgs, string(MsgInvocationsObserved)), acks)
	}
}

// concurrent local calls, emits and engine pushes share state safely (go test -race).
func TestConcurrentLocalCallsAndObserverChurn(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	registerLocal(t, c, "cc::fn", echoHandler)
	var wg sync.WaitGroup
	stop := make(chan struct{})
	for g := 0; g < 4; g++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			for {
				select {
				case <-stop:
					return
				default:
					_, _ = c.Trigger(context.Background(), TriggerRequest{FunctionID: "cc::fn", Data: json.RawMessage(`{"k":1}`)})
					_ = c.IsObserved("cc::fn")
					_ = c.Observers("cc::fn")
				}
			}
		}()
	}
	for i := 0; i < 20; i++ {
		id := fmt.Sprintf("o%d", i)
		fe.add("cc::fn", observerInfo(id, "x::y", "", `{"$and":[{"input.k":{"$eq":1}}]}`))
		if i%2 == 1 {
			fe.remove("cc::fn", fmt.Sprintf("o%d", i-1))
		}
	}
	// the odd ones stay: once they are active, calls are reported
	waitUntil(t, "o1, o3 ... o19 active", func() bool { return len(c.Observers("cc::fn")) == 10 })
	time.Sleep(20 * time.Millisecond)
	close(stop)
	wg.Wait()
	trigger(t, c, TriggerRequest{FunctionID: "cc::fn", Data: json.RawMessage(`{"k":1}`)})
	deadline := time.Now().Add(5 * time.Second)
	for len(fe.frames(MsgInvocationsObserved)) == 0 {
		if time.Now().After(deadline) {
			epoch, online := c.reportOnline()
			t.Fatalf("no report frames: buffered %d, queued %d, deferred %v, online %v, synced %v, frames %v",
				c.reporter.size(), c.reportQueued(), c.reporter.deferred.Load(), online,
				c.functionObservers("cc::fn").syncedOn(epoch), frameTypes(fe))
		}
		time.Sleep(5 * time.Millisecond)
	}
}

func frameTypes(fe *fakeEngine) map[string]int {
	out := map[string]int{}
	for _, m := range fe.m.receivedMessages() {
		out[messageType(m)]++
	}
	return out
}

// SDK-PARITY decisions 22, 26: a report pushed after FUNCTION_NAMESPACE_CONFLICT by a
// call already running is counted INACTIVE, never held.
func TestReportOfACallRunningAcrossAConflictIsInactive(t *testing.T) {
	fe := newFakeEngine(t)
	fe.setObservers("race::fn", 1, observerInfo("o1", "x::y", "", "", CaptureInput))
	c := fe.client()
	started, release := make(chan struct{}), make(chan struct{})
	registerLocal(t, c, "race::fn", func(context.Context, json.RawMessage) (any, error) {
		close(started)
		<-release
		return 1, nil
	})
	fe.waitAck("race::fn", "o1", 1)
	done := make(chan struct{})
	go func() {
		defer close(done)
		trigger(t, c, TriggerRequest{FunctionID: "race::fn"})
	}()
	<-started
	fe.send(&RegistrationRejectedMessage{Code: "FUNCTION_NAMESPACE_CONFLICT", Namespace: "default", FunctionID: "race::fn", OwnerWorkerID: "other"})
	waitUntil(t, "released", func() bool { return !c.LocalDispatchActive("race::fn") })
	close(release)
	<-done
	waitUntil(t, "the report dropped", func() bool { return c.reporter.size() == 0 })
	c.reporter.sendStats(time.Now())
	stats := fe.waitFrames(MsgLocalInvocationStats, 1, nil)[0].LocalInvocationStats
	if len(stats.DroppedReports) != 1 || stats.DroppedReports[0].Reason != "INACTIVE" || len(fe.reportItems()) != 0 {
		t.Errorf("drops %+v, items %d", stats.DroppedReports, len(fe.reportItems()))
	}
}

// A call running while its function is registered again under the same id is
// reported under the new registration.
func TestReportOfACallRunningAcrossAReRegistration(t *testing.T) {
	fe := newFakeEngine(t)
	fe.setObservers("rr::fn", 1, observerInfo("o1", "x::y", "", "", CaptureInput))
	c := fe.client()
	started, release := make(chan struct{}), make(chan struct{})
	registerLocal(t, c, "rr::fn", func(context.Context, json.RawMessage) (any, error) {
		close(started)
		<-release
		return 1, nil
	})
	fe.waitAck("rr::fn", "o1", 1)
	done := make(chan struct{})
	go func() {
		defer close(done)
		trigger(t, c, TriggerRequest{FunctionID: "rr::fn", Data: json.RawMessage(`"across"`)})
	}()
	<-started
	registerLocal(t, c, "rr::fn", echoHandler)
	waitUntil(t, "the new registration confirmed", func() bool {
		return c.functionObservers("rr::fn").syncedOn(c.currentEpoch())
	})
	close(release)
	<-done
	items := fe.waitItems(1)
	if string(items[0].Input) != `"across"` {
		t.Errorf("item %+v", items[0])
	}
}

// SDK-PARITY decision 30: a local call honors the caller's ctx deadline and Timeout
// even when the handler blocks and ignores its ctx; the handler runs to completion
// and its call is still counted.
func TestLocalCallReturnsOnTheCallersDeadlineWhenTheHandlerBlocks(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	release, finished := make(chan struct{}), make(chan struct{})
	registerLocal(t, c, "block::fn", func(context.Context, json.RawMessage) (any, error) {
		<-release // ignores ctx
		close(finished)
		return "late", nil
	})
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Millisecond)
	defer cancel()
	start := time.Now()
	if _, err := c.Trigger(ctx, TriggerRequest{FunctionID: "block::fn"}); !errors.Is(err, context.DeadlineExceeded) {
		t.Errorf("ctx deadline: %v", err)
	}
	if _, err := c.Trigger(context.Background(), TriggerRequest{FunctionID: "block::fn", Timeout: 30 * time.Millisecond}); !errors.Is(err, ErrTimeout) {
		t.Errorf("Timeout: %v", err)
	}
	if d := time.Since(start); d > time.Second {
		t.Errorf("the caller waited %s for a blocked handler", d)
	}
	close(release)
	<-finished
	waitUntil(t, "both calls counted", func() bool {
		c.reporter.mu.Lock()
		defer c.reporter.mu.Unlock()
		s := c.reporter.stats["block::fn"]
		return s != nil && s.count == 2
	})
}

// SDK-PARITY decision 29: the report of a local call carries the input and metadata as
// they were when the call started, whatever the caller does with its buffers later.
func TestLocalReportUsesSnapshotsTakenAtCallStart(t *testing.T) {
	fe := newFakeEngine(t)
	fe.setObservers("snap::fn", 1, observerInfo("o1", "x::y", "", "", CaptureInput, CaptureMetadata))
	c := fe.client()
	started, release := make(chan struct{}), make(chan struct{})
	registerLocal(t, c, "snap::fn", func(context.Context, json.RawMessage) (any, error) {
		close(started)
		<-release
		return nil, nil
	})
	fe.waitAck("snap::fn", "o1", 1)
	payload := json.RawMessage(`{"v":"original"}`)
	metadata := json.RawMessage(`{"m":"original"}`)
	done := make(chan struct{})
	go func() {
		defer close(done)
		_, _ = c.Trigger(context.Background(), TriggerRequest{FunctionID: "snap::fn", Data: payload, Metadata: metadata, Action: VoidAction()})
	}()
	<-done // void: returned at once
	<-started
	copy(payload[6:], "CHANGED!")
	copy(metadata[6:], "CHANGED!")
	close(release)
	it := fe.waitItems(1)[0]
	if string(it.Input) != `{"v":"original"}` || string(it.Metadata) != `{"m":"original"}` {
		t.Errorf("reported input %s metadata %s", it.Input, it.Metadata)
	}
}

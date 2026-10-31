//go:build integration

package iii_test

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
	"sync"
	"testing"
	"time"

	iii "github.com/iii-hq/iii/sdk/packages/go/iii"
)

// Client.Observe against the real engine, on the routed path: the observed function
// lives on another worker and every call goes through the engine, which evaluates the
// observers' on and filters with the input it forwarded and the host's answer
// (decision 55).

var allCaptures = []iii.Capture{iii.CaptureInput, iii.CaptureOutput, iii.CaptureError, iii.CaptureCaller, iii.CaptureMetadata}

func TestObserveRoutedFiltersOnInputOutputAndCaller(t *testing.T) {
	owner := observeWorker(t, "routed-owner", nil)
	watcher := observeWorker(t, "routed-watcher", nil)
	charge, checkout, target := fnID(t, "go-e2e::charge"), fnID(t, "go-e2e::checkout"), fnID(t, "go-e2e::on-big-charge")
	_, _ = owner.RegisterFunction(charge, func(_ context.Context, data json.RawMessage) (any, error) {
		var in struct {
			Amount int `json:"amount"`
		}
		_ = json.Unmarshal(data, &in)
		return map[string]int{"charged": in.Amount}, nil
	})
	got := collector(t, watcher, target)
	_, _ = watcher.RegisterFunction(checkout, func(ctx context.Context, data json.RawMessage) (any, error) {
		var amount int
		_ = json.Unmarshal(data, &amount)
		return watcher.Call(ctx, iii.CallRequest{FunctionID: charge, Payload: raw(map[string]int{"amount": amount})})
	})
	h, err := watcher.Observe(iii.Source{
		FunctionID: charge,
		Filters: iii.Filters{
			"input.amount":       iii.Op{"$gte": 500},
			"output.charged":     iii.Op{"$exists": true},
			"caller.function_id": iii.Op{"$eq": checkout},
		},
		Capture: allCaptures,
	}, target, iii.ObserverOptions{Metadata: raw(map[string]string{"team": "payments"})})
	if err != nil {
		t.Fatal(err)
	}
	ready(t, h)
	if h.State() != iii.ObserverActive {
		t.Fatalf("state %s", h.State())
	}

	call(t, watcher, iii.CallRequest{FunctionID: checkout, Payload: raw(750)})
	call(t, watcher, iii.CallRequest{FunctionID: checkout, Payload: raw(10)})                          // filtered out by input
	call(t, watcher, iii.CallRequest{FunctionID: charge, Payload: raw(map[string]int{"amount": 900})}) // no caller.function_id
	acts := exactly(t, got, 1, "one activation", 10*time.Second)
	a := acts[0]
	if a.Data["observer_id"] != h.ID() || a.Data["function_id"] != charge || a.Data["namespace"] != "default" || a.Data["status"] != "success" {
		t.Errorf("envelope %v", a.Data)
	}
	if field(a.Data, "input.amount") != float64(750) || field(a.Data, "output.charged") != float64(750) || field(a.Data, "caller.function_id") != checkout {
		t.Errorf("envelope parts %v", a.Data)
	}
	if name, _ := field(a.Data, "caller.worker_name").(string); !strings.Contains(name, "routed-watcher") {
		t.Errorf("caller.worker_name %v", field(a.Data, "caller.worker_name"))
	}
	for _, k := range []string{"activation_id", "invocation_id"} {
		if _, ok := a.Data[k].(string); !ok {
			t.Errorf("%s missing: %v", k, a.Data)
		}
	}
	if string(a.Metadata) != `{"team":"payments"}` {
		t.Errorf("metadata sidecar %s", a.Metadata)
	}
	if a.Inv.FunctionID != target || a.Inv.ObserveDepth != 1 || a.Inv.Activation == nil || a.Inv.Activation.ObserverID != h.ID() ||
		a.Inv.Caller == nil || a.Inv.Caller.WorkerName != "iii-engine" || a.Inv.Caller.FunctionID != charge || a.Inv.Caller.InvocationID != a.Data["invocation_id"] {
		t.Errorf("activation context %+v caller %+v", a.Inv, a.Inv.Caller)
	}
}

func TestObserveRoutedOnErrorCaptureAndDeliver(t *testing.T) {
	owner := observeWorker(t, "routed-owner", nil)
	watcher := observeWorker(t, "routed-watcher", nil)
	flaky := fnID(t, "go-e2e::flaky")
	_, _ = owner.RegisterFunction(flaky, func(_ context.Context, data json.RawMessage) (any, error) {
		var in struct {
			Fail bool `json:"fail"`
			N    int  `json:"n"`
		}
		_ = json.Unmarshal(data, &in)
		if in.Fail {
			return nil, &iii.InvocationError{Code: "invocation_failed", Message: "flaky failed"}
		}
		return in.N, nil
	})
	errs := collector(t, watcher, fnID(t, "go-e2e::on-error"))
	inputs := collector(t, watcher, fnID(t, "go-e2e::on-input"))
	a, _ := watcher.Observe(iii.Source{FunctionID: flaky, On: iii.OnError, Capture: []iii.Capture{iii.CaptureError}}, fnID(t, "go-e2e::on-error"))
	b, _ := watcher.Observe(iii.Source{FunctionID: flaky, On: iii.OnComplete, Deliver: iii.DeliverInput}, fnID(t, "go-e2e::on-input"))
	ready(t, a)
	ready(t, b)

	call(t, watcher, iii.CallRequest{FunctionID: flaky, Payload: raw(map[string]any{"fail": false, "n": 1})})
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	if _, err := watcher.Call(ctx, iii.CallRequest{FunctionID: flaky, Payload: raw(map[string]any{"fail": true, "n": 2})}); err == nil {
		t.Fatal("the failing call succeeded")
	}
	e := exactly(t, errs, 1, "the error activation", 10*time.Second)[0]
	if e.Data["status"] != "error" || field(e.Data, "error.code") != "invocation_failed" || field(e.Data, "error.message") != "flaky failed" {
		t.Errorf("error envelope %v", e.Data)
	}
	if _, has := e.Data["input"]; has {
		t.Errorf("capture [error] shipped the input: %v", e.Data)
	}
	ins := exactly(t, inputs, 2, "the input activations", 10*time.Second)
	seen := map[float64]bool{}
	for _, in := range ins {
		if _, isEnvelope := in.Data["observer_id"]; isEnvelope {
			t.Errorf("deliver: input sent an envelope: %v", in.Data)
		}
		n, _ := in.Data["n"].(float64)
		seen[n] = true
	}
	if !seen[1] || !seen[2] {
		t.Errorf("inputs %v", ins)
	}
}

func TestObserveRoutedPendingUntilTheTargetIsRegistered(t *testing.T) {
	owner := observeWorker(t, "routed-owner", nil)
	watcher := observeWorker(t, "routed-watcher", nil)
	tick, target := fnID(t, "go-e2e::tick"), fnID(t, "go-e2e::late-target")
	_, _ = owner.RegisterFunction(tick, func(context.Context, json.RawMessage) (any, error) { return "ok", nil })
	h, _ := watcher.Observe(iii.Source{FunctionID: tick}, target)
	time.Sleep(300 * time.Millisecond)
	if h.State() != iii.ObserverPending {
		t.Fatalf("state %s, want pending", h.State())
	}
	got := collector(t, watcher, target)
	ready(t, h)
	call(t, watcher, iii.CallRequest{FunctionID: tick})
	exactly(t, got, 1, "the activation", 10*time.Second)
}

func TestObserveRoutedErrors(t *testing.T) {
	self, other := fnID(t, "go-e2e::self"), fnID(t, "go-e2e::other")
	cases := []struct {
		code   string
		source iii.Source
		target string
	}{
		{"SELF_TARGET", iii.Source{FunctionID: self}, self},
		{"INVALID_ARGS", iii.Source{FunctionID: self, Args: iii.Args{"expression": "* * *"}}, other},
		{"INVALID_FILTER", iii.Source{FunctionID: self, Filters: iii.Filters{"input.op": "insert"}}, other},
	}
	for _, tc := range cases {
		t.Run(tc.code, func(t *testing.T) {
			owner := observeWorker(t, "routed-owner", nil)
			_, _ = owner.RegisterFunction(self, func(context.Context, json.RawMessage) (any, error) { return nil, nil })
			_, _ = owner.RegisterFunction(other, func(context.Context, json.RawMessage) (any, error) { return nil, nil })
			h, err := owner.Observe(tc.source, tc.target)
			if err != nil {
				t.Fatal(err)
			}
			if rej := readyFails(t, h); rej.Code != tc.code {
				t.Errorf("code %s (%s), want %s", rej.Code, rej.Message, tc.code)
			}
			if h.State() != iii.ObserverError || h.Reason() == nil || h.Reason().Code != tc.code {
				t.Errorf("state %s reason %+v", h.State(), h.Reason())
			}
		})
	}
}

// The engine's codes for function options and calls through an observer, as a Go
// worker gets them: an observer of a function registered Observable: &false fails
// Ready with NOT_OBSERVABLE; a call to a private function from a worker that does not
// host it fails with FORBIDDEN, while the host's own call through the engine runs it;
// a private function as a target fails Ready with FORBIDDEN; a call through an observer
// from a worker that does not host the observed function fails with
// NOT_FUNCTION_OWNER, while the host's call reaches the target.
func TestObserveRoutedEngineCodes(t *testing.T) {
	host := observeWorker(t, "codes-host", nil)
	other := observeWorker(t, "codes-other", nil)
	private, closed, target := fnID(t, "go-codes::private"), fnID(t, "go-codes::closed"), fnID(t, "go-codes::target")
	held := make(chan *iii.Observer, 1)
	host.OnObserverAddedFor(private, func(_ context.Context, obs *iii.Observer) error {
		held <- obs
		return nil
	})
	off := false
	if _, err := host.RegisterFunction(private, iii.Identity, iii.FunctionOptions{Private: true}); err != nil {
		t.Fatal(err)
	}
	if _, err := host.RegisterFunction(closed, iii.Identity, iii.FunctionOptions{Observable: &off}); err != nil {
		t.Fatal(err)
	}
	got := collector(t, other, target)
	localDispatchReady(t, host, private)
	localDispatchReady(t, host, closed)
	codeOf := func(err error) string {
		var ie *iii.InvocationError
		if !errors.As(err, &ie) {
			return ""
		}
		return ie.Code
	}

	t.Run("NOT_OBSERVABLE", func(t *testing.T) {
		h, err := other.ObserveFunction(closed, target)
		if err != nil {
			t.Fatal(err)
		}
		if rej := readyFails(t, h); rej.Code != "NOT_OBSERVABLE" {
			t.Errorf("code %s (%s), want NOT_OBSERVABLE", rej.Code, rej.Message)
		}
	})
	t.Run("FORBIDDEN call to a private function", func(t *testing.T) {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		if _, err := other.Call(ctx, iii.CallRequest{FunctionID: private, Payload: raw(map[string]int{"n": 1})}); codeOf(err) != "FORBIDDEN" {
			t.Errorf("call from another worker: %v, want FORBIDDEN", err)
		}
		out, err := host.Call(ctx, iii.CallRequest{FunctionID: private, Payload: raw(map[string]int{"n": 2}), Dispatch: iii.DispatchRemote})
		var echoed struct {
			N int `json:"n"`
		}
		if err != nil || json.Unmarshal(out, &echoed) != nil || echoed.N != 2 {
			t.Errorf("the host's call through the engine: %s %v", out, err)
		}
	})
	t.Run("FORBIDDEN private target", func(t *testing.T) {
		h, err := other.ObserveFunction(target, private)
		if err != nil {
			t.Fatal(err)
		}
		if rej := readyFails(t, h); rej.Code != "FORBIDDEN" {
			t.Errorf("code %s (%s), want FORBIDDEN", rej.Code, rej.Message)
		}
	})
	t.Run("NOT_FUNCTION_OWNER", func(t *testing.T) {
		h, err := other.ObserveFunction(private, target)
		if err != nil {
			t.Fatal(err)
		}
		ready(t, h)
		var obs *iii.Observer
		select {
		case obs = <-held:
		case <-time.After(10 * time.Second):
			t.Fatal("the host's added hook did not run")
		}
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		if _, err := other.Call(ctx, iii.CallRequest{Observer: obs, Payload: raw("from another worker")}); codeOf(err) != "NOT_FUNCTION_OWNER" {
			t.Errorf("call through the observer from another worker: %v, want NOT_FUNCTION_OWNER", err)
		}
		if _, err := host.Call(ctx, iii.CallRequest{Observer: obs, Payload: raw("from the host")}); err != nil {
			t.Fatalf("the host's call through the observer: %v", err)
		}
		a := exactly(t, got, 1, "the host's call through the observer", 10*time.Second)[0]
		if string(a.Raw) != `"from the host"` {
			t.Errorf("target got %s", a.Raw)
		}
	})
}

// deliver: output sends the target the handler's return value alone, for routed and
// local calls of the observed function.
func TestObserveRoutedDeliverOutput(t *testing.T) {
	owner := observeWorker(t, "deliver-owner", nil)
	watcher := observeWorker(t, "deliver-watcher", nil)
	double, target := fnID(t, "go-deliver::double"), fnID(t, "go-deliver::on-output")
	_, _ = owner.RegisterFunction(double, func(_ context.Context, data json.RawMessage) (any, error) {
		var in struct {
			N int `json:"n"`
		}
		_ = json.Unmarshal(data, &in)
		return map[string]int{"doubled": in.N * 2}, nil
	})
	got := collector(t, watcher, target)
	h, err := watcher.Observe(iii.Source{FunctionID: double, Deliver: iii.DeliverOutput}, target)
	if err != nil {
		t.Fatal(err)
	}
	ready(t, h)
	localDispatchReady(t, owner, double)
	call(t, watcher, iii.CallRequest{FunctionID: double, Payload: raw(map[string]int{"n": 2})}) // routed
	call(t, owner, iii.CallRequest{FunctionID: double, Payload: raw(map[string]int{"n": 3})})   // local, reported
	outs := exactly(t, got, 2, "the output activations", 10*time.Second)
	seen := map[string]bool{}
	for _, a := range outs {
		seen[string(a.Raw)] = true
	}
	if !seen[`{"doubled":4}`] || !seen[`{"doubled":6}`] {
		t.Errorf("targets got %v, want the bare outputs {\"doubled\":4} and {\"doubled\":6}", outs)
	}
}

func TestObserveRoutedDisposeAndMaxFires(t *testing.T) {
	owner := observeWorker(t, "routed-owner", nil)
	watcher := observeWorker(t, "routed-watcher", nil)
	ping := fnID(t, "go-e2e::ping")
	_, _ = owner.RegisterFunction(ping, func(context.Context, json.RawMessage) (any, error) { return "pong", nil })
	disposed := collector(t, watcher, fnID(t, "go-e2e::on-ping-a"))
	once := collector(t, watcher, fnID(t, "go-e2e::on-ping-b"))
	a, _ := watcher.Observe(iii.Source{FunctionID: ping}, fnID(t, "go-e2e::on-ping-a"))
	b, _ := watcher.Observe(iii.Source{FunctionID: ping}, fnID(t, "go-e2e::on-ping-b"), iii.ObserverOptions{MaxFires: 1})
	ready(t, a)
	ready(t, b)
	if err := a.Dispose(context.Background()); err != nil || a.State() != iii.ObserverDisposed {
		t.Fatalf("dispose: %v %s", err, a.State())
	}
	call(t, watcher, iii.CallRequest{FunctionID: ping})
	call(t, watcher, iii.CallRequest{FunctionID: ping})
	waitFor(t, "max_fires to dispose the observer", 10*time.Second, func() bool { return b.State() == iii.ObserverDisposed })
	time.Sleep(300 * time.Millisecond)
	if disposed.len() != 0 || once.len() != 1 {
		t.Errorf("disposed observer got %d, max_fires 1 got %d", disposed.len(), once.len())
	}
	if b.Reason() == nil || b.Reason().Code != "MAX_FIRES" {
		t.Errorf("reason %+v", b.Reason())
	}
}

func TestObserveRoutedSurvivesAReconnectOfItsWorker(t *testing.T) {
	proxy := newFrameProxy(t)
	owner := observeWorker(t, "routed-owner", nil)
	watcher := observeWorker(t, "routed-watcher", proxy)
	work, target := fnID(t, "go-e2e::work"), fnID(t, "go-e2e::on-work")
	_, _ = owner.RegisterFunction(work, func(context.Context, json.RawMessage) (any, error) { return "done", nil })
	got := collector(t, watcher, target)
	h, _ := watcher.Observe(iii.Source{FunctionID: work}, target)
	ready(t, h)
	mark := proxy.mark()
	proxy.drop()
	waitFor(t, "the observe replay", 10*time.Second, func() bool { return countOf(proxy.since(mark), "observe") >= 1 })
	time.Sleep(500 * time.Millisecond)
	call(t, owner, iii.CallRequest{FunctionID: work})
	a := exactly(t, got, 1, "an activation after the reconnect", 10*time.Second)[0]
	if a.Data["observer_id"] != h.ID() || h.State() != iii.ObserverActive {
		t.Errorf("observer %v state %s", a.Data["observer_id"], h.State())
	}
}

func TestObserveRoutedExpiresIn(t *testing.T) {
	owner := observeWorker(t, "routed-owner", nil)
	watcher := observeWorker(t, "routed-watcher", nil)
	lease, target := fnID(t, "go-e2e::lease"), fnID(t, "go-e2e::on-lease")
	_, _ = owner.RegisterFunction(lease, func(context.Context, json.RawMessage) (any, error) { return "ok", nil })
	got := collector(t, watcher, target)
	h, _ := watcher.Observe(iii.Source{FunctionID: lease}, target, iii.ObserverOptions{ExpiresIn: 300 * time.Millisecond})
	ready(t, h)
	time.Sleep(500 * time.Millisecond)
	call(t, watcher, iii.CallRequest{FunctionID: lease})
	waitFor(t, "the expiry", 10*time.Second, func() bool { return h.State() == iii.ObserverDisposed })
	time.Sleep(200 * time.Millisecond)
	if got.len() != 0 || h.Reason() == nil || h.Reason().Code != "EXPIRED" {
		t.Errorf("%d activations, reason %+v", got.len(), h.Reason())
	}
}

// observe_depth travels with the calls a target makes (through the handler's ctx), and
// the engine's loop guard stops an observe loop.
func TestObserveRoutedLoopGuard(t *testing.T) {
	owner := observeWorker(t, "routed-owner", nil)
	watcher := observeWorker(t, "routed-watcher", nil)
	loop, onLoop := fnID(t, "go-e2e::loop"), fnID(t, "go-e2e::on-loop")
	_, _ = owner.RegisterFunction(loop, func(_ context.Context, data json.RawMessage) (any, error) { return data, nil })
	var mu sync.Mutex
	var depths []uint32
	_, _ = watcher.RegisterFunction(onLoop, func(ctx context.Context, data json.RawMessage) (any, error) {
		inv, _ := iii.InvocationFromContext(ctx)
		mu.Lock()
		depths = append(depths, inv.ObserveDepth)
		mu.Unlock()
		var env struct {
			Input int `json:"input"`
		}
		_ = json.Unmarshal(data, &env)
		_, err := watcher.Call(ctx, iii.CallRequest{FunctionID: loop, Payload: raw(env.Input + 1)})
		return nil, err
	})
	h, _ := watcher.Observe(iii.Source{FunctionID: loop, Capture: []iii.Capture{iii.CaptureInput}}, onLoop)
	ready(t, h)
	call(t, watcher, iii.CallRequest{FunctionID: loop, Payload: raw(0)})
	time.Sleep(2 * time.Second)
	mu.Lock()
	defer mu.Unlock()
	if len(depths) == 0 || len(depths) > 8 {
		t.Fatalf("%d hops (depths %v): the loop guard stops at max_observe_depth 8", len(depths), depths)
	}
	for i, d := range depths {
		if d != uint32(i+1) {
			t.Errorf("hop %d at depth %d, want %d (depths %v)", i, d, i+1, depths)
			break
		}
	}
}

// sdk.md §7 Reconnect: with a valid Reattach the engine re-parents the observer and the
// replay is a no-op that keeps its fires counter (max_fires counts across it).
func TestObserveRoutedReattachKeepsFires(t *testing.T) {
	proxy := newFrameProxy(t)
	owner := observeWorker(t, "routed-owner", nil)
	watcher := observeWorker(t, "routed-watcher", proxy)
	work, target := fnID(t, "go-e2e::fires"), fnID(t, "go-e2e::on-fires")
	_, _ = owner.RegisterFunction(work, func(context.Context, json.RawMessage) (any, error) { return "done", nil })
	got := collector(t, watcher, target)
	h, _ := watcher.Observe(iii.Source{FunctionID: work}, target, iii.ObserverOptions{MaxFires: 3})
	ready(t, h)
	call(t, owner, iii.CallRequest{FunctionID: work})
	waitFor(t, "the first activation", 10*time.Second, func() bool { return got.len() == 1 })

	mark := proxy.mark()
	proxy.drop()
	waitFor(t, "the reattach", 10*time.Second, func() bool {
		sent := proxy.since(mark)
		return countOf(sent, "reattach") == 1 && countOf(sent, "observe") >= 1
	})
	time.Sleep(500 * time.Millisecond)
	call(t, owner, iii.CallRequest{FunctionID: work})
	call(t, owner, iii.CallRequest{FunctionID: work})
	waitFor(t, "max_fires reached across the reattach", 10*time.Second, func() bool { return h.State() == iii.ObserverDisposed })
	if got.len() != 3 || h.Reason() == nil || h.Reason().Code != "MAX_FIRES" {
		t.Errorf("%d activations, reason %+v: fires must survive the reattach", got.len(), h.Reason())
	}
}

// A routed call is delivered by the engine, from the input it forwarded and the host's
// invocationresult: an observed routed call costs no frame of its own.
func TestObserveRoutedMatchesRideTheAnswer(t *testing.T) {
	proxy := newFrameProxy(t)
	host := observeWorker(t, "routed-host", proxy)
	caller := observeWorker(t, "routed-caller", nil)
	fn, target := fnID(t, "go-host::double"), fnID(t, "go-host::on-double")
	_, _ = host.RegisterFunction(fn, func(_ context.Context, data json.RawMessage) (any, error) {
		var n int
		_ = json.Unmarshal(data, &n)
		return n * 2, nil
	})
	got := collector(t, caller, target)
	h, err := caller.Observe(iii.Source{FunctionID: fn, Filters: iii.Filters{"output": iii.Op{"$gte": 10}}}, target)
	if err != nil {
		t.Fatal(err)
	}
	ready(t, h)
	mark := proxy.mark()
	if out := call(t, caller, iii.CallRequest{FunctionID: fn, Payload: raw(21)}); string(out) != "42" {
		t.Errorf("result %s", out)
	}
	call(t, caller, iii.CallRequest{FunctionID: fn, Payload: raw(1)}) // filtered out by output
	a := exactly(t, got, 1, "the routed activation", 10*time.Second)[0]
	if a.Data["input"] != float64(21) || a.Data["output"] != float64(42) {
		t.Errorf("envelope %v", a.Data)
	}
	if sent := without(proxy.since(mark), "localinvocationstats"); countOf(sent, "invocationsobserved") != 0 || countOf(sent, "invocationresult") != 2 {
		t.Errorf("host frames %v: the answer is the only frame", sent)
	}
}

//go:build integration

package iii_test

import (
	"context"
	"encoding/json"
	"strings"
	"sync"
	"testing"
	"time"

	iii "github.com/iii-hq/iii/sdk/packages/go/iii"
)

// Client.Observe against the real engine, on the routed path (README §7 phase 1):
// the observed function lives on another worker, every call goes through the engine,
// and the engine evaluates the filters.

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
		return watcher.Trigger(ctx, iii.TriggerRequest{FunctionID: charge, Payload: raw(map[string]int{"amount": amount})})
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

	call(t, watcher, iii.TriggerRequest{FunctionID: checkout, Payload: raw(750)})
	call(t, watcher, iii.TriggerRequest{FunctionID: checkout, Payload: raw(10)})                          // filtered out by input
	call(t, watcher, iii.TriggerRequest{FunctionID: charge, Payload: raw(map[string]int{"amount": 900})}) // no caller.function_id
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

	call(t, watcher, iii.TriggerRequest{FunctionID: flaky, Payload: raw(map[string]any{"fail": false, "n": 1})})
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	if _, err := watcher.Trigger(ctx, iii.TriggerRequest{FunctionID: flaky, Payload: raw(map[string]any{"fail": true, "n": 2})}); err == nil {
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
	call(t, watcher, iii.TriggerRequest{FunctionID: tick})
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
	call(t, watcher, iii.TriggerRequest{FunctionID: ping})
	call(t, watcher, iii.TriggerRequest{FunctionID: ping})
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
	call(t, owner, iii.TriggerRequest{FunctionID: work})
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
	call(t, watcher, iii.TriggerRequest{FunctionID: lease})
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
		_, err := watcher.Trigger(ctx, iii.TriggerRequest{FunctionID: loop, Payload: raw(env.Input + 1)})
		return nil, err
	})
	h, _ := watcher.Observe(iii.Source{FunctionID: loop, Capture: []iii.Capture{iii.CaptureInput}}, onLoop)
	ready(t, h)
	call(t, watcher, iii.TriggerRequest{FunctionID: loop, Payload: raw(0)})
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
	call(t, owner, iii.TriggerRequest{FunctionID: work})
	waitFor(t, "the first activation", 10*time.Second, func() bool { return got.len() == 1 })

	mark := proxy.mark()
	proxy.drop()
	waitFor(t, "the reattach", 10*time.Second, func() bool {
		sent := proxy.since(mark)
		return countOf(sent, "reattach") == 1 && countOf(sent, "observe") >= 1
	})
	time.Sleep(500 * time.Millisecond)
	call(t, owner, iii.TriggerRequest{FunctionID: work})
	call(t, owner, iii.TriggerRequest{FunctionID: work})
	waitFor(t, "max_fires reached across the reattach", 10*time.Second, func() bool { return h.State() == iii.ObserverDisposed })
	if got.len() != 3 || h.Reason() == nil || h.Reason().Code != "MAX_FIRES" {
		t.Errorf("%d activations, reason %+v: fires must survive the reattach", got.len(), h.Reason())
	}
}

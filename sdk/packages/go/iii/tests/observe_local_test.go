//go:build integration

package iii_test

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"reflect"
	"strings"
	"sync"
	"testing"
	"time"

	iii "github.com/iii-hq/iii/sdk/packages/go/iii"
)

// Observable functions against the real engine: local dispatch, the reporter, the
// observer hooks, matching by keys and calls through an observer.

// stable drops what differs between two runs of the same invocation: ids and timings.
func stable(env map[string]any) map[string]any {
	out := map[string]any{}
	for k, v := range env {
		switch k {
		case "activation_id", "invocation_id", "started_at_ms", "duration_ms":
		case "error":
			if m, ok := v.(map[string]any); ok {
				body := map[string]any{}
				for kk, vv := range m {
					if kk != "stacktrace" {
						body[kk] = vv
					}
				}
				out[k] = body
			}
		case "caller":
			if m, ok := v.(map[string]any); ok {
				c := map[string]any{}
				for kk, vv := range m {
					if kk != "invocation_id" {
						c[kk] = vv
					}
				}
				out[k] = c
			}
		default:
			out[k] = v
		}
	}
	return out
}

// sdk.md §7 Equivalence: dispatch auto (local) and remote give identical envelopes,
// apart from ids and timings.
func TestObserveEquivalenceLocalAndRemote(t *testing.T) {
	proxy := newFrameProxy(t)
	owner := observeWorker(t, "eq-owner", proxy)
	watcher := observeWorker(t, "eq-watcher", nil)
	price, checkout, target := fnID(t, "go-eq::price"), fnID(t, "go-eq::checkout"), fnID(t, "go-eq::on-price")
	_, _ = owner.RegisterFunction(price, func(_ context.Context, data json.RawMessage) (any, error) {
		var in struct {
			SKU  string `json:"sku"`
			Qty  int    `json:"qty"`
			Fail bool   `json:"fail"`
		}
		_ = json.Unmarshal(data, &in)
		if in.Fail {
			return nil, errors.New("out of stock")
		}
		return map[string]any{"total": in.Qty * 10, "sku": in.SKU}, nil
	})
	_, _ = owner.RegisterFunction(checkout, func(ctx context.Context, data json.RawMessage) (any, error) {
		var in struct {
			Dispatch iii.Dispatch `json:"dispatch"`
			Fail     bool         `json:"fail"`
		}
		_ = json.Unmarshal(data, &in)
		return owner.Call(ctx, iii.CallRequest{
			FunctionID: price,
			Payload:    raw(map[string]any{"sku": "A-1", "qty": 3, "fail": in.Fail}),
			Metadata:   raw(map[string]string{"channel": "web"}),
			Dispatch:   in.Dispatch,
		})
	})
	got := collector(t, watcher, target)
	h, _ := watcher.Observe(iii.Source{
		FunctionID: price, On: iii.OnComplete, Capture: allCaptures,
		Filters: iii.Filters{"input.sku": iii.Op{"$prefix": "A-"}},
	}, target, iii.ObserverOptions{Metadata: raw(map[string]string{"observer": "eq"})})
	ready(t, h)
	localDispatchReady(t, owner, price)
	localDispatchReady(t, owner, checkout)

	run := func(dispatch iii.Dispatch, fail bool) activation {
		before := got.len()
		ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cancel()
		_, _ = owner.Call(ctx, iii.CallRequest{FunctionID: checkout, Payload: raw(map[string]any{"dispatch": dispatch, "fail": fail}), Dispatch: dispatch})
		waitFor(t, fmt.Sprintf("the %s activation", dispatch), 10*time.Second, func() bool { return got.len() > before })
		all := got.all()
		return all[len(all)-1]
	}
	remoteOK := run(iii.DispatchRemote, false)
	mark := proxy.mark()
	localOK := run(iii.DispatchAuto, false)
	if n := countOf(proxy.since(mark), "invokefunction"); n != 0 {
		t.Errorf("the local run sent %d invokefunction frames", n)
	}
	remoteErr := run(iii.DispatchRemote, true)
	localErr := run(iii.DispatchAuto, true)

	if a, b := stable(localOK.Data), stable(remoteOK.Data); !reflect.DeepEqual(a, b) {
		t.Errorf("success envelopes differ:\n local  %v\n remote %v", a, b)
	}
	if a, b := stable(localErr.Data), stable(remoteErr.Data); !reflect.DeepEqual(a, b) {
		t.Errorf("error envelopes differ:\n local  %v\n remote %v", a, b)
	}
	if string(localOK.Metadata) != string(remoteOK.Metadata) || localOK.Inv.ObserveDepth != remoteOK.Inv.ObserveDepth {
		t.Errorf("metadata %s / %s, depth %d / %d", localOK.Metadata, remoteOK.Metadata, localOK.Inv.ObserveDepth, remoteOK.Inv.ObserveDepth)
	}
	if field(remoteOK.Data, "input.sku") != "A-1" || field(remoteOK.Data, "output.total") != float64(30) ||
		field(remoteOK.Data, "metadata.channel") != "web" || field(remoteOK.Data, "caller.function_id") != checkout {
		t.Errorf("remote envelope %v", remoteOK.Data)
	}
}

// sdk.md §7 Zero-wire: 1M local calls of an unobserved function send zero frames
// (localinvocationstats, the periodic counter, aside).
func TestObserveZeroWire(t *testing.T) {
	// It measures what reaches the wire, not telemetry: with telemetry on, its million
	// local calls each run in a traced span, about 7s more for the whole loop. The
	// telemetry socket is separate and its frames are not counted either way.
	t.Setenv("OTEL_ENABLED", "false")
	proxy := newFrameProxy(t)
	owner := observeWorker(t, "zero-wire", proxy)
	inc := fnID(t, "go-zw::inc")
	_, _ = owner.RegisterFunction(inc, func(_ context.Context, data json.RawMessage) (any, error) { return data, nil })
	localDispatchReady(t, owner, inc)
	const calls = 1_000_000
	mark := proxy.mark()
	ctx := context.Background()
	req := iii.CallRequest{FunctionID: inc, Payload: json.RawMessage(`1`)}
	start := time.Now()
	for i := 0; i < calls; i++ {
		if _, err := owner.Call(ctx, req); err != nil {
			t.Fatal(err)
		}
	}
	elapsed := time.Since(start)
	time.Sleep(100 * time.Millisecond)
	if sent := without(proxy.since(mark), "localinvocationstats"); len(sent) != 0 {
		t.Fatalf("%d frames for unobserved local calls: %v", len(sent), sent[:min(len(sent), 10)])
	}
	t.Logf("%d local calls in %s", calls, elapsed)

	// positive control: the same counter sees the frames of an observed function
	watcher := observeWorker(t, "zero-wire-watcher", nil)
	observed, target := fnID(t, "go-zw::observed"), fnID(t, "go-zw::target")
	_, _ = owner.RegisterFunction(observed, func(_ context.Context, data json.RawMessage) (any, error) { return data, nil })
	localDispatchReady(t, owner, observed)
	got := collector(t, watcher, target)
	h, _ := watcher.Observe(iii.Source{FunctionID: observed, Capture: []iii.Capture{iii.CaptureInput}}, target)
	ready(t, h)
	mark = proxy.mark()
	for i := 0; i < 10; i++ {
		call(t, owner, iii.CallRequest{FunctionID: observed, Payload: raw(i)})
	}
	exactly(t, got, 10, "the observed calls' activations", 10*time.Second)
	if n := countOf(proxy.since(mark), "invocationsobserved"); n == 0 {
		t.Error("the frame counter saw no invocationsobserved frame for observed calls")
	}
}

// sdk.md §7 Activation race: after ready(), N local calls give exactly N activations.
func TestObserveActivationRace(t *testing.T) {
	const n = 200
	proxy := newFrameProxy(t)
	owner := observeWorker(t, "race-owner", proxy)
	watcher := observeWorker(t, "race-watcher", nil)
	fn, target := fnID(t, "go-race::fn"), fnID(t, "go-race::target")
	_, _ = owner.RegisterFunction(fn, func(_ context.Context, data json.RawMessage) (any, error) { return data, nil })
	localDispatchReady(t, owner, fn)
	got := collector(t, watcher, target)
	h, _ := watcher.Observe(iii.Source{FunctionID: fn, Capture: []iii.Capture{iii.CaptureInput}}, target, iii.ObserverOptions{MaxInFlight: n})
	ready(t, h)
	mark := proxy.mark()
	for i := 0; i < n; i++ {
		call(t, owner, iii.CallRequest{FunctionID: fn, Payload: raw(i)})
	}
	acts := exactly(t, got, n, fmt.Sprintf("%d activations", n), 20*time.Second)
	seen := map[float64]bool{}
	for _, a := range acts {
		v, _ := a.Data["input"].(float64)
		seen[v] = true
	}
	sent := proxy.since(mark)
	if len(seen) != n || countOf(sent, "invokefunction") != 0 || countOf(sent, "invocationsobserved") == 0 {
		t.Errorf("%d distinct inputs; frames %d invokefunction, %d invocationsobserved", len(seen), countOf(sent, "invokefunction"), countOf(sent, "invocationsobserved"))
	}
}

// cronSource is an event source like the cron worker's: a private function that
// returns the tick it is called with, observed with args { expression }. Its hooks are
// set by id before it is registered, and its match hook gives each tick to the
// observers of its expression, by keys.
type cronSource struct {
	fn     string
	client *iii.Client
	proxy  *frameProxy

	mu       sync.Mutex
	added    []*iii.Observer
	removed  []string
	reject   string
	addDelay time.Duration
}

func newCronSource(t *testing.T) *cronSource {
	t.Helper()
	o := &cronSource{fn: fnID(t, "go-cron::on-tick")}
	o.proxy = newFrameProxy(t)
	o.client = observeWorker(t, "cron-source", o.proxy)
	expression := func(obs *iii.Observer) string {
		expr, _ := obs.Args["expression"].(string)
		return expr
	}
	o.client.OnObserverAddedFor(o.fn, func(ctx context.Context, obs *iii.Observer) error {
		o.mu.Lock()
		o.added = append(o.added, obs)
		delay, reject := o.addDelay, o.reject
		o.mu.Unlock()
		time.Sleep(delay)
		if reject != "" && expression(obs) == reject {
			return iii.ObserveRejection("INVALID_CRON_EXPRESSION", "bad expression "+reject)
		}
		return nil
	})
	o.client.OnObserverRemovedFor(o.fn, func(_ context.Context, obs *iii.Observer) {
		o.mu.Lock()
		o.removed = append(o.removed, obs.ID)
		o.mu.Unlock()
	})
	o.client.OnObserverMatchFor(o.fn, iii.MatchKeys(
		func(obs *iii.Observer) []string { return []string{expression(obs)} },
		func(call *iii.Call) []string {
			var tick struct {
				Expression string `json:"expression"`
			}
			_ = json.Unmarshal(call.Input, &tick)
			return []string{tick.Expression}
		},
	))
	_, err := o.client.RegisterFunction(o.fn, iii.Identity, iii.FunctionOptions{
		ArgsFormat: json.RawMessage(`{"type":"object","additionalProperties":false,"required":["expression"],"properties":{"expression":{"type":"string"}}}`),
		Private:    true,
	})
	if err != nil {
		t.Fatal(err)
	}
	localDispatchReady(t, o.client, o.fn)
	return o
}

func (o *cronSource) addedArgs() []map[string]any {
	o.mu.Lock()
	defer o.mu.Unlock()
	var out []map[string]any
	for _, a := range o.added {
		out = append(out, a.Args)
	}
	return out
}

func (o *cronSource) wasRemoved(id string) bool {
	o.mu.Lock()
	defer o.mu.Unlock()
	for _, r := range o.removed {
		if r == id {
			return true
		}
	}
	return false
}

func (o *cronSource) tick(t *testing.T, expr string, n int) {
	t.Helper()
	call(t, o.client, iii.CallRequest{FunctionID: o.fn, Payload: raw(map[string]any{"expression": expr, "n": n})})
}

// Args: obs.Args is what the observer sent; a missing required argument, and args on
// a function without args_format, are INVALID_ARGS before any hook runs.
func TestObserveSourceArgs(t *testing.T) {
	cron := newCronSource(t)
	watcher := observeWorker(t, "args-watcher", nil)
	target, plain := fnID(t, "go-args::target"), fnID(t, "go-args::plain")
	_, _ = watcher.RegisterFunction(target, func(context.Context, json.RawMessage) (any, error) { return nil, nil })
	_, _ = watcher.RegisterFunction(plain, func(context.Context, json.RawMessage) (any, error) { return nil, nil })
	ok, _ := watcher.Observe(iii.Source{FunctionID: cron.fn, Args: iii.Args{"expression": "*/5"}}, target)
	ready(t, ok)
	if args := cron.addedArgs(); len(args) != 1 || args[0]["expression"] != "*/5" {
		t.Fatalf("obs.Args %v", args)
	}
	missing, _ := watcher.Observe(iii.Source{FunctionID: cron.fn, Args: iii.Args{}}, target)
	if rej := readyFails(t, missing); rej.Code != "INVALID_ARGS" {
		t.Errorf("missing arg: %s", rej.Code)
	}
	onPlain, _ := watcher.Observe(iii.Source{FunctionID: plain, Args: iii.Args{"expression": "*/5"}}, target)
	if rej := readyFails(t, onPlain); rej.Code != "INVALID_ARGS" {
		t.Errorf("args on a function without args_format: %s", rej.Code)
	}
	if n := len(cron.addedArgs()); n != 1 {
		t.Errorf("the hook ran %d times", n)
	}
}

// Matching by keys: two observers with different args only receive the ticks of their
// own expression; a tick nobody is filed for reaches nobody.
func TestObserveSourceRouting(t *testing.T) {
	cron := newCronSource(t)
	watcher := observeWorker(t, "routing-watcher", nil)
	five := collector(t, watcher, fnID(t, "go-routing::five"))
	ten := collector(t, watcher, fnID(t, "go-routing::ten"))
	a, _ := watcher.Observe(iii.Source{FunctionID: cron.fn, Args: iii.Args{"expression": "*/5"}}, fnID(t, "go-routing::five"))
	b, _ := watcher.Observe(iii.Source{FunctionID: cron.fn, Args: iii.Args{"expression": "*/10"}}, fnID(t, "go-routing::ten"))
	ready(t, a)
	ready(t, b)
	if cron.client.IsObservedFor(cron.fn, &iii.Call{Input: raw(map[string]string{"expression": "* * *"})}) {
		t.Error("IsObservedFor true for an expression nobody observes")
	}
	cron.tick(t, "*/10", 1)
	cron.tick(t, "*/5", 2)
	cron.tick(t, "* * *", 3)
	tenGot := exactly(t, ten, 1, "the */10 activation", 10*time.Second)
	fiveGot := exactly(t, five, 1, "the */5 activation", 10*time.Second)
	if tenGot[0].Data["observer_id"] != b.ID() || tenGot[0].Data["function_id"] != cron.fn || field(tenGot[0].Data, "input.n") != float64(1) {
		t.Errorf("*/10 got %v", tenGot[0].Data)
	}
	if fiveGot[0].Data["observer_id"] != a.ID() || field(fiveGot[0].Data, "input.n") != float64(2) {
		t.Errorf("*/5 got %v", fiveGot[0].Data)
	}
}

// Hooks: once per add and before the ack; an error rejects Ready with its code;
// engine::observers::get shows the filters unchanged; the removed hook on dispose.
func TestObserveSourceHooks(t *testing.T) {
	cron := newCronSource(t)
	cron.mu.Lock()
	cron.reject, cron.addDelay = "bad", 300*time.Millisecond
	cron.mu.Unlock()
	watcher := observeWorker(t, "hooks-watcher", nil)
	target := fnID(t, "go-hooks::target")
	_, _ = watcher.RegisterFunction(target, func(context.Context, json.RawMessage) (any, error) { return nil, nil })
	start := time.Now()
	ok, _ := watcher.Observe(iii.Source{FunctionID: cron.fn, Args: iii.Args{"expression": "*/5"}, Filters: iii.Filters{"input.n": iii.Op{"$gte": 2}}}, target)
	ready(t, ok)
	if time.Since(start) < 250*time.Millisecond {
		t.Errorf("active after %s: before the hook returned", time.Since(start))
	}
	cron.mu.Lock()
	added := append([]*iii.Observer(nil), cron.added...)
	cron.mu.Unlock()
	if len(added) != 1 || added[0].ID != ok.ID() {
		t.Fatalf("hook calls %v", added)
	}
	if f, _ := json.Marshal(added[0].Filters); string(f) != `{"$and":[{"input.n":{"$gte":2}}]}` {
		t.Errorf("filters as the host sees them: %s", f)
	}
	got := call(t, watcher, iii.CallRequest{FunctionID: iii.FnGetObserver, Payload: raw(map[string]string{"id": ok.ID()})})
	if !strings.Contains(string(got), `"input.n"`) {
		t.Errorf("engine::observers::get: %s", got)
	}
	bad, _ := watcher.Observe(iii.Source{FunctionID: cron.fn, Args: iii.Args{"expression": "bad"}}, target)
	if rej := readyFails(t, bad); rej.Code != "INVALID_CRON_EXPRESSION" || bad.State() != iii.ObserverError {
		t.Errorf("rejected: %s %s", rej.Code, bad.State())
	}
	if err := ok.Dispose(context.Background()); err != nil {
		t.Fatal(err)
	}
	waitFor(t, "the removed hook after dispose", 10*time.Second, func() bool { return cron.wasRemoved(ok.ID()) })
	for _, o := range cron.client.Observers(cron.fn) {
		if o.ID == ok.ID() {
			t.Error("still in Observers()")
		}
	}
}

// Delivery of a source's calls: a tick the filters reject sends no frame; the target
// sees the envelope, the source's worker as the caller, and the observer's metadata
// sidecar.
func TestObserveSourceDelivery(t *testing.T) {
	cron := newCronSource(t)
	watcher := observeWorker(t, "delivery-watcher", nil)
	target := fnID(t, "go-delivery::target")
	got := collector(t, watcher, target)
	h, _ := watcher.Observe(iii.Source{
		FunctionID: cron.fn, Args: iii.Args{"expression": "*/5"},
		Filters: iii.Filters{"input.n": iii.Op{"$gt": 10}}, Capture: []iii.Capture{iii.CaptureInput, iii.CaptureCaller},
	}, target, iii.ObserverOptions{Metadata: raw(map[string]string{"__binding": "digest"})})
	ready(t, h)
	mark := cron.proxy.mark()
	cron.tick(t, "*/5", 1)
	time.Sleep(100 * time.Millisecond)
	if sent := without(cron.proxy.since(mark), "localinvocationstats"); len(sent) != 0 {
		t.Errorf("frames %v for a tick the filters reject", sent)
	}
	cron.tick(t, "*/5", 11)
	a := exactly(t, got, 1, "the delivered activation", 10*time.Second)[0]
	if a.Data["observer_id"] != h.ID() || a.Data["status"] != "success" || field(a.Data, "input.n") != float64(11) {
		t.Errorf("envelope %v", a.Data)
	}
	if name, _ := field(a.Data, "caller.worker_name").(string); !strings.Contains(name, "cron-source") {
		t.Errorf("caller %v", a.Data["caller"])
	}
	if string(a.Metadata) != `{"__binding":"digest"}` {
		t.Errorf("metadata sidecar %s", a.Metadata)
	}
}

// A call through an observer returns the target's result: the deliver hook routes a
// request to the observer of its path and answers with its target's result; a request
// the observer's filters reject is answered without a frame; after dispose the engine
// refuses the call; the target sees the observer's metadata sidecar.
func TestObserveCallThroughAnObserver(t *testing.T) {
	proxy := newFrameProxy(t)
	host := observeWorker(t, "http-host", proxy)
	onRequest, getUser := fnID(t, "go-http::on-request"), fnID(t, "go-users::get")
	var mu sync.Mutex
	routes := map[string]*iii.Observer{}
	pathOf := func(obs *iii.Observer) string {
		path, _ := obs.Args["path"].(string)
		return path
	}
	host.OnObserverAddedFor(onRequest, func(_ context.Context, obs *iii.Observer) error {
		mu.Lock()
		routes[pathOf(obs)] = obs
		mu.Unlock()
		return nil
	})
	host.OnObserverRemovedFor(onRequest, func(_ context.Context, obs *iii.Observer) {
		mu.Lock()
		delete(routes, pathOf(obs))
		mu.Unlock()
	})
	host.OnObserverDeliverFor(onRequest, func(ctx context.Context, call *iii.Call, _ []*iii.Observer) (any, error) {
		var req struct {
			Path string `json:"path"`
		}
		_ = json.Unmarshal(call.Input, &req)
		mu.Lock()
		obs := routes[req.Path]
		mu.Unlock()
		if obs == nil {
			return map[string]int{"status": 404}, nil
		}
		if !obs.Matches(call) {
			return map[string]int{"status": 422}, nil
		}
		return host.Call(ctx, iii.CallRequest{Observer: obs, Payload: call.Input})
	})
	_, _ = host.RegisterFunction(onRequest, iii.Identity, iii.FunctionOptions{
		ArgsFormat: json.RawMessage(`{"type":"object","required":["path"],"properties":{"path":{"type":"string"}}}`),
		Private:    true,
	})
	app := observeWorker(t, "http-app", nil)
	var seen json.RawMessage
	_, _ = app.RegisterFunction(getUser, func(ctx context.Context, data json.RawMessage) (any, error) {
		md, _ := iii.MetadataFromContext(ctx)
		mu.Lock()
		seen = md
		mu.Unlock()
		var req struct {
			Path string `json:"path"`
		}
		_ = json.Unmarshal(data, &req)
		return map[string]any{"status": 200, "body": map[string]string{"path": req.Path}}, nil
	})
	route, _ := app.Observe(iii.Source{FunctionID: onRequest, Args: iii.Args{"path": "/users"}, Filters: iii.Filters{"input.tenant": iii.Op{"$eq": "acme"}}},
		getUser, iii.ObserverOptions{Metadata: raw(map[string]string{"route": "users"})})
	ready(t, route)
	localDispatchReady(t, host, onRequest)

	out := call(t, host, iii.CallRequest{FunctionID: onRequest, Payload: raw(map[string]string{"path": "/users", "tenant": "acme"})})
	if string(out) != `{"body":{"path":"/users"},"status":200}` {
		t.Errorf("response %s", out)
	}
	mu.Lock()
	if string(seen) != `{"route":"users"}` {
		t.Errorf("the target's metadata %s", seen)
	}
	obs := routes["/users"]
	mu.Unlock()

	mark := proxy.mark()
	if out := call(t, host, iii.CallRequest{FunctionID: onRequest, Payload: raw(map[string]string{"path": "/users", "tenant": "other"})}); string(out) != `{"status":422}` {
		t.Errorf("a request the filters reject: %s", out)
	}
	if sent := without(proxy.since(mark), "localinvocationstats"); len(sent) != 0 {
		t.Errorf("frames %v for a rejected request", sent)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	_ = route.Dispose(ctx)
	waitFor(t, "the removed hook", 10*time.Second, func() bool { mu.Lock(); defer mu.Unlock(); return routes["/users"] == nil })
	var ie *iii.InvocationError
	if _, err := host.Call(ctx, iii.CallRequest{Observer: obs, Payload: raw(map[string]string{"path": "/users", "tenant": "acme"})}); !errors.As(err, &ie) ||
		(ie.Code != "OBSERVER_NOT_FOUND" && ie.Code != "OBSERVER_NOT_ACTIVE") {
		t.Errorf("after dispose: %v", err)
	}
}

// sdk.md §7 Reconnect: local dispatch keeps serving, reports flush after the
// reconnect, and the functionregistered diff calls the right hooks.
func TestObserveReconnect(t *testing.T) {
	proxy := newFrameProxy(t)
	owner := observeWorker(t, "rc-owner", proxy)
	watcher := observeWorker(t, "rc-watcher", nil)
	fn, target, source := fnID(t, "go-rc::fn"), fnID(t, "go-rc::target"), fnID(t, "go-rc::source")
	_, _ = owner.RegisterFunction(fn, func(_ context.Context, data json.RawMessage) (any, error) {
		var n int
		_ = json.Unmarshal(data, &n)
		return n * 2, nil
	})
	var mu sync.Mutex
	var added, removed []string
	owner.OnObserverAddedFor(source, func(_ context.Context, obs *iii.Observer) error {
		mu.Lock()
		added = append(added, obs.ID)
		mu.Unlock()
		return nil
	})
	owner.OnObserverRemovedFor(source, func(_ context.Context, obs *iii.Observer) {
		mu.Lock()
		removed = append(removed, obs.ID)
		mu.Unlock()
	})
	_, _ = owner.RegisterFunction(source, func(context.Context, json.RawMessage) (any, error) { return nil, nil })
	got := collector(t, watcher, target)
	_, _ = watcher.RegisterFunction(fnID(t, "go-rc::sink"), func(context.Context, json.RawMessage) (any, error) { return nil, nil })
	kept, _ := watcher.Observe(iii.Source{FunctionID: fn, Capture: []iii.Capture{iii.CaptureInput}}, target)
	staying, _ := watcher.Observe(iii.Source{FunctionID: source}, fnID(t, "go-rc::sink"))
	leaving, _ := watcher.Observe(iii.Source{FunctionID: source}, fnID(t, "go-rc::sink"))
	ready(t, kept)
	ready(t, staying)
	ready(t, leaving)
	localDispatchReady(t, owner, fn)

	proxy.drop()
	waitFor(t, "the disconnect", 5*time.Second, func() bool { return owner.State() != iii.StateConnected })
	if out := call(t, owner, iii.CallRequest{FunctionID: fn, Payload: raw(21)}); string(out) != "42" { // still local
		t.Errorf("local call while disconnected: %s", out)
	}
	// an observer removed while the owner is away
	_ = leaving.Dispose(context.Background())
	waitFor(t, "the reconnect", 10*time.Second, func() bool { return owner.State() == iii.StateConnected })
	a := exactly(t, got, 1, "the activation reported after the reconnect", 10*time.Second)[0]
	if a.Data["input"] != float64(21) {
		t.Errorf("reported input %v", a.Data["input"])
	}
	waitFor(t, "the diff removes the observer disposed meanwhile", 10*time.Second, func() bool {
		mu.Lock()
		defer mu.Unlock()
		return len(removed) == 1
	})
	mu.Lock()
	defer mu.Unlock()
	if removed[0] != leaving.ID() || len(added) != 2 {
		t.Errorf("added %v removed %v (staying %s, leaving %s): kept observers must not run the hook again", added, removed, staying.ID(), leaving.ID())
	}
}

// sdk.md §7 Reconnect: a FUNCTION_NAMESPACE_CONFLICT on the replay (another worker
// took the function while this one was away) disables local dispatch: the calls go
// through the engine to the new owner.
func TestObserveConflictOnReplayDisablesLocalDispatch(t *testing.T) {
	proxy := newFrameProxy(t)
	first := observeWorker(t, "conflict-first", proxy)
	fn := fnID(t, "go-conflict::fn")
	_, _ = first.RegisterFunction(fn, func(context.Context, json.RawMessage) (any, error) { return "first", nil })
	localDispatchReady(t, first, fn)

	proxy.hold()
	proxy.drop()
	waitFor(t, "the disconnect", 5*time.Second, func() bool { return first.State() != iii.StateConnected })
	if out := call(t, first, iii.CallRequest{FunctionID: fn}); string(out) != `"first"` { // local while away
		t.Errorf("local call while away: %s", out)
	}
	second := observeWorker(t, "conflict-second", nil)
	waitFor(t, "the second worker to own the function", 10*time.Second, func() bool {
		_, _ = second.RegisterFunction(fn, func(context.Context, json.RawMessage) (any, error) { return "second", nil })
		ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
		defer cancel()
		out, err := second.Call(ctx, iii.CallRequest{FunctionID: fn, Dispatch: iii.DispatchRemote})
		return err == nil && string(out) == `"second"`
	})
	proxy.release()
	waitFor(t, "local dispatch disabled by the conflict", 10*time.Second, func() bool {
		return first.State() == iii.StateConnected && !first.LocalDispatchActive(fn)
	})
	if out := call(t, first, iii.CallRequest{FunctionID: fn}); string(out) != `"second"` {
		t.Errorf("after the conflict the call must reach the new owner, got %s", out)
	}
}

package iii

import (
	"context"
	"encoding/json"
	"errors"
	"sync"
	"testing"
	"time"

	"github.com/google/uuid"
)

func readyErr(t *testing.T, h *ObserverHandle) error {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	return h.Ready(ctx)
}

func observeFrames(fe *fakeEngine, id string) []*ObserveMessage {
	var out []*ObserveMessage
	for _, d := range fe.frames(MsgObserve) {
		if d.Observe.ID == id {
			out = append(out, d.Observe)
		}
	}
	return out
}

// sdk.md §2: Observe returns a pending handle at once, before the connection is up,
// and sends it after WorkerRegistered, after the functions (local targets first).
func TestObservePendingBeforeConnectThenSentAfterFunctions(t *testing.T) {
	fe := newFakeEngine(t)
	c := New(fe.m.url, WithName("observe-test"))
	t.Cleanup(func() { _ = c.Close() })
	_, _ = c.RegisterFunction("local::target", echoHandler)
	h, err := c.Observe(Source{FunctionID: "orders::create"}, "local::target")
	if err != nil {
		t.Fatal(err)
	}
	if h.State() != ObserverPending || h.ID() == "" || h.Reason() != nil {
		t.Fatalf("state %s id %q reason %v", h.State(), h.ID(), h.Reason())
	}
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	if err := c.Connect(ctx); err != nil {
		t.Fatal(err)
	}
	fe.waitFrames(MsgObserve, 1, nil)
	msgs := fe.m.receivedMessages()
	fn, obs := indexOfType(msgs, string(MsgRegisterFunction)), indexOfType(msgs, string(MsgObserve))
	if fn < 0 || obs < fn {
		t.Errorf("registerfunction at %d, observe at %d: functions go first", fn, obs)
	}
}

func TestObserveNamespaceResolution(t *testing.T) {
	c := New("ws://unused.invalid", WithNamespace("acme"))
	t.Cleanup(func() { _ = c.Close() })
	cases := []struct {
		src       Source
		target    string
		opts      ObserverOptions
		sourceNS  string
		targetNS  string
		describes string
	}{
		{Source{FunctionID: "orders::create"}, "audit::log", ObserverOptions{}, "acme", "acme", "the worker's namespace"},
		{Source{FunctionID: "engine::on-functions-changed"}, "audit::log", ObserverOptions{}, "default", "acme", "engine builtins resolve to default"},
		{Source{FunctionID: "orders::create", Namespace: "shop"}, "audit::log", ObserverOptions{TargetNamespace: "ops"}, "shop", "ops", "explicit namespaces win"},
	}
	for _, tc := range cases {
		h, err := c.Observe(tc.src, tc.target, tc.opts)
		if err != nil {
			t.Fatal(err)
		}
		dec, _ := UnmarshalMessage(h.frame)
		if dec.Observe.Source.Namespace != tc.sourceNS || dec.Observe.Target.Namespace != tc.targetNS {
			t.Errorf("%s: source %q target %q", tc.describes, dec.Observe.Source.Namespace, dec.Observe.Target.Namespace)
		}
	}
}

func TestObserveStatesReadyAndListeners(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	h, err := c.Observe(Source{FunctionID: "orders::create"}, "audit::log")
	if err != nil {
		t.Fatal(err)
	}
	fe.waitFrames(MsgObserve, 1, nil)

	var seen []ObserverState
	var mu = make(chan struct{}, 1)
	mu <- struct{}{}
	off := h.OnStateChange(func(s ObserverState, _ *ObserveReason) {
		<-mu
		seen = append(seen, s)
		mu <- struct{}{}
	})
	h.OnStateChange(func(ObserverState, *ObserveReason) { panic("a listener that panics does not stop the others") })
	done := make(chan error, 1)
	go func() { done <- readyErr(t, h) }()

	fe.send(&ObserveResultMessage{ID: h.ID(), State: "pending"})
	fe.send(&ObserverStateChangedMessage{ID: h.ID(), State: "active", Fires: 0})
	if err := <-done; err != nil {
		t.Fatalf("Ready: %v", err)
	}
	if h.State() != ObserverActive {
		t.Fatalf("state %s", h.State())
	}
	// listeners run on the handle's own goroutine: wait for the delivery
	waitUntil(t, "the active delivery", func() bool {
		<-mu
		defer func() { mu <- struct{}{} }()
		return len(seen) == 1
	})
	off()
	fe.send(&ObserverStateChangedMessage{ID: h.ID(), State: "suspended", Reason: &ErrorBody{Code: "TARGET_UNAVAILABLE", Message: "gone"}})
	waitUntil(t, "suspended", func() bool { return h.State() == ObserverSuspended })
	if r := h.Reason(); r == nil || r.Code != "TARGET_UNAVAILABLE" {
		t.Errorf("reason %+v", r)
	}
	<-mu
	if len(seen) != 1 || seen[0] != ObserverActive {
		t.Errorf("listener saw %v (unsubscribed after active)", seen)
	}
	mu <- struct{}{}
}

func TestObserveReadyFailsWithTheEngineCode(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	h, _ := c.Observe(Source{FunctionID: "orders::create"}, "orders::create")
	fe.waitFrames(MsgObserve, 1, nil)
	fe.send(&ObserveResultMessage{ID: h.ID(), State: "error", Error: &ErrorBody{Code: "SELF_TARGET", Message: "target equals source"}})
	var rej *ObserveRejectionError
	if err := readyErr(t, h); !errors.As(err, &rej) || rej.Code != "SELF_TARGET" {
		t.Fatalf("Ready: %v", err)
	}
	// ctx bounds the wait
	h2, _ := c.Observe(Source{FunctionID: "orders::create"}, "audit::log")
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Millisecond)
	defer cancel()
	if err := h2.Ready(ctx); !errors.Is(err, context.DeadlineExceeded) {
		t.Errorf("Ready on a pending observer: %v", err)
	}
}

// protocol.md §1: against an engine without observe.v1, Observe fails with
// UNSUPPORTED (state error, Ready fails) and nothing is sent.
func TestObserveUnsupportedAgainstOldEngine(t *testing.T) {
	fe := newFakeEngine(t).oldEngine()
	c := New(fe.m.url)
	t.Cleanup(func() { _ = c.Close() })
	before, _ := c.Observe(Source{FunctionID: "orders::create"}, "audit::log")
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	if err := c.Connect(ctx); err != nil {
		t.Fatal(err)
	}
	waitUntil(t, "registered", c.registeredNow)
	after, err := c.Observe(Source{FunctionID: "orders::create"}, "audit::log")
	if err != nil {
		t.Fatal(err)
	}
	for _, h := range []*ObserverHandle{before, after} {
		var rej *ObserveRejectionError
		if err := readyErr(t, h); !errors.As(err, &rej) || rej.Code != "UNSUPPORTED" {
			t.Errorf("Ready: %v", err)
		}
	}
	time.Sleep(20 * time.Millisecond)
	if n := len(fe.frames(MsgObserve)); n != 0 {
		t.Errorf("%d observe frames sent to an old engine", n)
	}
}

func TestObserveReplayedAfterReconnectUntilDisposed(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	kept, _ := c.Observe(Source{FunctionID: "orders::create"}, "audit::log")
	gone, _ := c.Observe(Source{FunctionID: "orders::create"}, "audit::other")
	fe.waitFrames(MsgObserve, 2, nil)
	if err := gone.Dispose(context.Background()); err != nil {
		t.Fatal(err)
	}
	if gone.State() != ObserverDisposed {
		t.Fatalf("state %s", gone.State())
	}
	var rej *ObserveRejectionError
	if err := readyErr(t, gone); !errors.As(err, &rej) || rej.Code != "UNOBSERVED" {
		t.Errorf("Ready after dispose: %v", err)
	}
	fe.waitFrames(MsgUnobserve, 1, func(d *DecodedMessage) bool { return d.Unobserve.ID == gone.ID() })

	fe.reconnect(c)
	fe.waitFrames(MsgObserve, 3, nil)
	time.Sleep(20 * time.Millisecond)
	if n := len(observeFrames(fe, kept.ID())); n != 2 {
		t.Errorf("kept observer sent %d times, want 2 (once per connection, same id)", n)
	}
	if n := len(observeFrames(fe, gone.ID())); n != 1 {
		t.Errorf("disposed observer sent %d times, want 1", n)
	}
	// Dispose is idempotent
	if err := gone.Dispose(context.Background()); err != nil || len(fe.frames(MsgUnobserve)) != 1 {
		t.Errorf("second Dispose: %v, %d unobserve frames", err, len(fe.frames(MsgUnobserve)))
	}
}

// An observer disposed while disconnected is unobserved on the next connection (a
// Reattach would otherwise keep it alive), unless that engine lacks observe.v1.
func TestObserveDisposedWhileDisconnectedQueuesUnobserve(t *testing.T) {
	for _, supported := range []bool{true, false} {
		fe := newFakeEngine(t)
		c := fe.client()
		h, _ := c.Observe(Source{FunctionID: "orders::create"}, "audit::log")
		fe.waitFrames(MsgObserve, 1, nil)

		gate := make(chan struct{})
		fe.mu.Lock()
		fe.confirm = false
		if !supported {
			fe.features = nil
		}
		fe.gate = gate
		connects := fe.connects
		fe.mu.Unlock()
		// the next connection is held before workerregistered: disposed meanwhile
		fe.m.closeActiveConnection()
		waitUntil(t, "reconnect", func() bool { fe.mu.Lock(); defer fe.mu.Unlock(); return fe.connects > connects })
		if c.registeredNow() {
			t.Fatal("registered before workerregistered")
		}
		_ = h.Dispose(context.Background())
		time.Sleep(20 * time.Millisecond)
		if n := len(fe.frames(MsgUnobserve)); n != 0 {
			t.Fatalf("unobserve sent before workerregistered: %d", n)
		}
		close(gate)
		waitUntil(t, "registered", c.registeredNow)
		time.Sleep(30 * time.Millisecond)
		n := len(fe.frames(MsgUnobserve))
		if supported && n != 1 {
			t.Errorf("observe.v1 engine: %d unobserve frames, want 1", n)
		}
		if !supported && n != 0 {
			t.Errorf("engine without observe.v1: %d unobserve frames, want 0 (dropped)", n)
		}
	}
}

func TestObserveEngineDisposedIsNotReplayed(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	h, _ := c.Observe(Source{FunctionID: "orders::create"}, "audit::log", ObserverOptions{MaxFires: 1})
	fe.waitFrames(MsgObserve, 1, nil)
	fe.send(&ObserveResultMessage{ID: h.ID(), State: "active"})
	fe.send(&ObserverStateChangedMessage{ID: h.ID(), State: "disposed", Reason: &ErrorBody{Code: "MAX_FIRES", Message: "fired once"}, Fires: 1})
	waitUntil(t, "disposed", func() bool { return h.State() == ObserverDisposed })
	if h.Reason().Code != "MAX_FIRES" {
		t.Errorf("reason %+v", h.Reason())
	}
	fe.reconnect(c)
	time.Sleep(30 * time.Millisecond)
	if n := len(observeFrames(fe, h.ID())); n != 1 {
		t.Errorf("sent %d times, want 1", n)
	}
}

func TestObserveRefusesBadRequests(t *testing.T) {
	c := New("ws://unused.invalid")
	t.Cleanup(func() { _ = c.Close() })
	if _, err := c.Observe(Source{}, "x::y"); err == nil {
		t.Error("missing function id accepted")
	}
	if _, err := c.Observe(Source{FunctionID: "a::b"}, " "); err == nil {
		t.Error("missing target accepted")
	}
	if _, err := c.Observe(Source{FunctionID: "a::b"}, "x::y", ObserverOptions{}, ObserverOptions{}); err == nil {
		t.Error("two options accepted")
	}
	if _, err := c.Observe(Source{FunctionID: "a::b", Namespace: " "}, "x::y"); err == nil {
		t.Error("blank namespace accepted")
	}
	if _, err := c.Observe(Source{FunctionID: "a::b"}, "x::y", ObserverOptions{Metadata: []byte("{")}); err == nil {
		t.Error("invalid metadata accepted")
	}
	if _, err := c.Observe(Source{FunctionID: "a::b", Args: Args{"f": func() {}}}, "x::y"); err == nil {
		t.Error("args that are not JSON accepted")
	}
	if _, err := c.Observe(Source{FunctionID: "a::b"}, "x::y", ObserverOptions{ID: "same"}); err != nil {
		t.Fatal(err)
	}
	var rej *ObserveRejectionError
	if _, err := c.Observe(Source{FunctionID: "a::b"}, "x::y", ObserverOptions{ID: "same"}); !errors.As(err, &rej) || rej.Code != "OBSERVER_ID_CONFLICT" {
		t.Errorf("held id: %v, want OBSERVER_ID_CONFLICT", err)
	}
}

// SDK-PARITY decision 27: state listeners never run on the read loop: a listener that
// blocks (or calls Call) does not hold up the engine's other messages, and changes
// reach a listener in order.
func TestStateListenersDoNotRunOnTheReadLoop(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	h, _ := c.Observe(Source{FunctionID: "orders::create"}, "audit::log")
	fe.waitFrames(MsgObserve, 1, nil)
	release := make(chan struct{})
	var mu sync.Mutex
	var seen []ObserverState
	h.OnStateChange(func(s ObserverState, _ *ObserveReason) {
		<-release
		mu.Lock()
		seen = append(seen, s)
		mu.Unlock()
	})
	answered := make(chan struct{})
	_, _ = c.RegisterFunction("ping::fn", func(context.Context, json.RawMessage) (any, error) {
		close(answered)
		return "pong", nil
	})
	fe.send(&ObserveResultMessage{ID: h.ID(), State: "active"})
	fe.send(&ObserverStateChangedMessage{ID: h.ID(), State: "suspended", Reason: &ErrorBody{Code: "TARGET_UNAVAILABLE", Message: "x"}})
	id := uuid.New()
	fe.send(&InvokeFunctionMessage{InvocationID: &id, FunctionID: "ping::fn", Data: json.RawMessage(`{}`)})
	select {
	case <-answered:
	case <-time.After(2 * time.Second):
		t.Fatal("the read loop is held by a blocked state listener")
	}
	waitUntil(t, "state applied", func() bool { return h.State() == ObserverSuspended })
	close(release)
	waitUntil(t, "listener calls", func() bool { mu.Lock(); defer mu.Unlock(); return len(seen) == 2 })
	mu.Lock()
	defer mu.Unlock()
	if seen[0] != ObserverActive || seen[1] != ObserverSuspended {
		t.Errorf("listener saw %v, want [active suspended]", seen)
	}
}

// SDK-PARITY decision 16: observe-family frames left in the offline buffer wait for
// WorkerRegistered: observes are replayed from the registry, unobserves sent only to
// an engine with observe.v1.
func TestOfflineObserveFramesWaitForTheEngineFeatures(t *testing.T) {
	for _, supported := range []bool{true, false} {
		fe := newFakeEngine(t)
		if !supported {
			fe.oldEngine()
		}
		c := New(fe.m.url, WithName("observe-test"))
		t.Cleanup(func() { _ = c.Close() })
		h, _ := c.Observe(Source{FunctionID: "orders::create"}, "audit::log")
		stale, _ := prepareJSON(&ObserveMessage{ID: "stale", Source: ObserveMessageSource{FunctionID: "a"}, Target: ObserveMessageTarget{FunctionID: "b"}})
		gone, _ := prepareJSON(&UnobserveMessage{ID: "gone"})
		ack, _ := prepareJSON(&ObserverAckMessage{FunctionID: "f", ID: "x", Revision: 1})
		c.mu.Lock()
		c.offline = append(c.offline, stale, gone, ack)
		c.mu.Unlock()
		ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
		defer cancel()
		if err := c.Connect(ctx); err != nil {
			t.Fatal(err)
		}
		waitUntil(t, "registered", c.registeredNow)
		time.Sleep(30 * time.Millisecond)
		msgs := fe.m.receivedMessages()
		if n := countType(msgs, string(MsgObserverAck)); n != 0 {
			t.Errorf("supported=%v: %d stale acks replayed", supported, n)
		}
		for _, d := range fe.frames(MsgObserve) {
			if d.Observe.ID != h.ID() {
				t.Errorf("supported=%v: a stale observe frame %s was replayed", supported, d.Observe.ID)
			}
		}
		unobserves := fe.frames(MsgUnobserve)
		if supported && (len(unobserves) != 1 || unobserves[0].Unobserve.ID != "gone" ||
			indexOfType(msgs, string(MsgUnobserve)) < indexOfFunctionID(msgs, FnRegisterWorker)) {
			t.Errorf("observe.v1 engine: unobserves %v (after workerregistered)", unobserves)
		}
		if !supported && len(unobserves) != 0 {
			t.Errorf("old engine: %d unobserves", len(unobserves))
		}
	}
}

// An unobserve never precedes the replayed observe of the same observer: Dispose and
// the replay are ordered.
func TestDisposeAndReplayAreOrdered(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	for round := 0; round < 5; round++ {
		var handles []*ObserverHandle
		for i := 0; i < 20; i++ {
			h, _ := c.Observe(Source{FunctionID: "orders::create"}, "audit::log")
			handles = append(handles, h)
		}
		var wg sync.WaitGroup
		wg.Add(1)
		go func() {
			defer wg.Done()
			for _, h := range handles {
				_ = h.Dispose(context.Background())
			}
		}()
		fe.reconnect(c)
		wg.Wait()
	}
	time.Sleep(50 * time.Millisecond)
	unobserved := map[string]bool{}
	for _, m := range fe.m.receivedMessages() {
		switch messageType(m) {
		case string(MsgUnobserve):
			unobserved[messageID(m)] = true
		case string(MsgObserve):
			if unobserved[messageID(m)] {
				t.Fatalf("observe %s sent after its unobserve", messageID(m))
			}
		}
	}
}

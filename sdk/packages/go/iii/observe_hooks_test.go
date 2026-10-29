package iii

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"slices"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"
)

// hookLog records hook calls for one test: the function-level adds and removes by
// observer id, and every hook call in order as "<added|removed>:<level>:<id>".
type hookLog struct {
	mu      sync.Mutex
	added   []string
	removed []string
	events  []string
}

func (l *hookLog) add(id string) {
	l.mu.Lock()
	l.added = append(l.added, id)
	l.mu.Unlock()
}

func (l *hookLog) remove(id string) {
	l.mu.Lock()
	l.removed = append(l.removed, id)
	l.mu.Unlock()
}

func (l *hookLog) event(e string) {
	l.mu.Lock()
	l.events = append(l.events, e)
	l.mu.Unlock()
}

func (l *hookLog) snapshot() (added, removed []string) {
	l.mu.Lock()
	defer l.mu.Unlock()
	return append([]string(nil), l.added...), append([]string(nil), l.removed...)
}

func (l *hookLog) eventList() []string {
	l.mu.Lock()
	defer l.mu.Unlock()
	return append([]string(nil), l.events...)
}

// recordHooks sets functionID's added and removed hooks to record in l.
func recordHooks(c *Client, functionID string, l *hookLog) {
	c.OnObserverAddedFor(functionID, func(_ context.Context, obs *Observer) error {
		l.add(obs.ID)
		return nil
	})
	c.OnObserverRemovedFor(functionID, func(_ context.Context, obs *Observer) { l.remove(obs.ID) })
}

// recordLevels sets the worker's hooks and functionID's hooks to record every call in
// l's events.
func recordLevels(c *Client, functionID string, l *hookLog) {
	c.OnObserverAdded(func(_ context.Context, obs *Observer) error {
		l.event("added:worker:" + obs.ID)
		return nil
	})
	c.OnObserverRemoved(func(_ context.Context, obs *Observer) { l.event("removed:worker:" + obs.ID) })
	c.OnObserverAddedFor(functionID, func(_ context.Context, obs *Observer) error {
		l.event("added:function:" + obs.ID)
		return nil
	})
	c.OnObserverRemovedFor(functionID, func(_ context.Context, obs *Observer) { l.event("removed:function:" + obs.ID) })
}

// registerHost registers functionID and waits until the engine's functionregistered
// was applied and acked.
func registerHost(t *testing.T, c *Client, functionID string, handler Handler, opts ...RegisterFunctionOptions) *FunctionRef {
	t.Helper()
	ref, err := c.RegisterFunction(functionID, handler, opts...)
	if err != nil {
		t.Fatal(err)
	}
	waitUntil(t, "functionregistered for "+functionID, func() bool {
		f := c.functionObservers(functionID)
		return f != nil && f.syncedOn(c.currentEpoch())
	})
	return ref
}

func countEvents(events []string, prefix string) int {
	n := 0
	for _, e := range events {
		if strings.HasPrefix(e, prefix) {
			n++
		}
	}
	return n
}

// The added hook runs once per add, before the ack, and the observer is in the
// compiled set when the ack goes out. The ack is observerack.json.
func TestHooksRunOncePerAddBeforeTheAck(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	var calls atomic.Int32
	var observedAtHook, observedAtAck atomic.Bool
	c.OnObserverAddedFor("cron::on-tick", func(_ context.Context, obs *Observer) error {
		calls.Add(1)
		observedAtHook.Store(c.IsObserved("cron::on-tick"))
		if obs.Args["expression"] != "0 0 9 * * Mon-Fri" || obs.Target != "billing::send-reminders" || obs.FunctionID != "cron::on-tick" {
			t.Errorf("observer %+v", obs)
		}
		return nil
	})
	registerHost(t, c, "cron::on-tick", Identity)
	fe.setOnFrame(func(msg map[string]json.RawMessage) {
		if messageType(msg) == string(MsgObserverAck) {
			observedAtAck.Store(c.IsObserved("cron::on-tick"))
		}
	})
	fe.sendRaw(string(observeFixture(t, "observeradded.json")))
	ack := fe.waitAck("cron::on-tick", "a07d4c2e-6b1f-4a3d-9e8c-5f2a1b3c4d5e", 13)
	got, _ := MarshalMessage(ack)
	assertJSONEqual(t, "observerack.json", got, observeFixture(t, "observerack.json"))
	if calls.Load() != 1 || observedAtHook.Load() || !observedAtAck.Load() {
		t.Errorf("calls %d, observed at hook %v (want false), at ack %v (want true)", calls.Load(), observedAtHook.Load(), observedAtAck.Load())
	}
	obs := c.Observers("cron::on-tick")
	if len(obs) != 1 || !obs[0].Matches(&Call{Caller: &Caller{WorkerName: "billing"}}) || obs[0].Matches(&Call{Caller: &Caller{WorkerName: "replayer"}}) {
		t.Errorf("observers %v: the normalized filters must apply", obs)
	}
	// the same revision again is ignored: no second hook call
	fe.sendRaw(string(observeFixture(t, "observeradded.json")))
	time.Sleep(30 * time.Millisecond)
	if calls.Load() != 1 {
		t.Errorf("hook ran %d times", calls.Load())
	}
}

// Hook order at both levels: added runs the worker's hook, then the function's;
// removed runs the function's, then the worker's. A function without hooks of its own
// runs the worker's only.
func TestHookOrderAtBothLevels(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	var l hookLog
	recordLevels(c, "order::fn", &l)
	registerHost(t, c, "order::fn", echoHandler)
	registerHost(t, c, "plain::fn", echoHandler)
	rev := fe.add("order::fn", observerInfo("o1", "x::y", "", ""))
	fe.waitAck("order::fn", "o1", rev)
	fe.remove("order::fn", "o1")
	waitUntil(t, "four hooks", func() bool { return len(l.eventList()) == 4 })
	want := []string{"added:worker:o1", "added:function:o1", "removed:function:o1", "removed:worker:o1"}
	if got := l.eventList(); !slices.Equal(got, want) {
		t.Errorf("events %v, want %v", got, want)
	}
	rev = fe.add("plain::fn", observerInfo("p1", "x::y", "", ""))
	fe.waitAck("plain::fn", "p1", rev)
	fe.remove("plain::fn", "p1")
	waitUntil(t, "worker hooks of plain::fn", func() bool { return len(l.eventList()) == 6 })
	if got := l.eventList()[4:]; !slices.Equal(got, []string{"added:worker:p1", "removed:worker:p1"}) {
		t.Errorf("plain::fn events %v", got)
	}
}

// Rejection unwinding: when the function's added hook rejects an observer, the
// worker's removed hook runs (its added hook resolved) and the function's does not.
// When the worker's added hook rejects, the function's added hook never runs and no
// removed hook does. Neither observer is known, so the engine's observerremoved runs
// nothing.
func TestRejectionUnwindsTheLevelsThatResolved(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	var l hookLog
	c.OnObserverAdded(func(_ context.Context, obs *Observer) error {
		l.event("added:worker:" + obs.ID)
		if obs.ID == "w-rejects" {
			return ObserveRejection("WORKER_SAYS_NO", "the worker refuses it")
		}
		return nil
	})
	c.OnObserverRemoved(func(_ context.Context, obs *Observer) { l.event("removed:worker:" + obs.ID) })
	c.OnObserverAddedFor("unwind::fn", func(_ context.Context, obs *Observer) error {
		l.event("added:function:" + obs.ID)
		if obs.ID == "f-rejects" {
			return errors.New("no such table")
		}
		return nil
	})
	c.OnObserverRemovedFor("unwind::fn", func(_ context.Context, obs *Observer) { l.event("removed:function:" + obs.ID) })
	registerHost(t, c, "unwind::fn", echoHandler)

	r1 := fe.add("unwind::fn", observerInfo("f-rejects", "x::y", "", ""))
	if a := fe.waitAck("unwind::fn", "f-rejects", r1); a.Error == nil || a.Error.Code != "OBSERVE_REJECTED" || a.Error.Message != "no such table" {
		t.Fatalf("ack %+v", a.Error)
	}
	r2 := fe.add("unwind::fn", observerInfo("w-rejects", "x::y", "", ""))
	if a := fe.waitAck("unwind::fn", "w-rejects", r2); a.Error == nil || a.Error.Code != "WORKER_SAYS_NO" {
		t.Fatalf("ack %+v", a.Error)
	}
	fe.remove("unwind::fn", "f-rejects")
	fe.remove("unwind::fn", "w-rejects")
	time.Sleep(30 * time.Millisecond)
	want := []string{"added:worker:f-rejects", "added:function:f-rejects", "removed:worker:f-rejects", "added:worker:w-rejects"}
	if got := l.eventList(); !slices.Equal(got, want) {
		t.Errorf("events %v, want %v", got, want)
	}
	if c.IsObserved("unwind::fn") {
		t.Error("a rejected observer is active")
	}
}

// One removal per accepted observer, at each level: on the engine's observerremoved,
// on a late rejection (once the engine removes it), and on Unregister. A stale
// observerremoved removes nothing twice, and an observer that was never accepted is
// never removed.
func TestOneRemovalPerAcceptedObserver(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	var l hookLog
	recordLevels(c, "once::fn", &l)
	c.OnObserverAddedFor("once::fn", func(_ context.Context, obs *Observer) error {
		l.event("added:function:" + obs.ID)
		if obs.ID == "never" {
			return ObserveRejection("NOPE", "not accepted")
		}
		return nil
	})
	ref := registerHost(t, c, "once::fn", echoHandler)
	for _, id := range []string{"o1", "o2", "o3", "never"} {
		rev := fe.add("once::fn", observerInfo(id, "x::y", "", ""))
		fe.waitAck("once::fn", id, rev)
	}
	// o1: unobserved; a stale observerremoved for it again does nothing
	rev := fe.remove("once::fn", "o1")
	fe.send(&ObserverRemovedMessage{FunctionID: "once::fn", Namespace: "default", Revision: rev, ID: "o1"})
	// o2: rejected late, then removed by the engine
	for _, obs := range c.Observers("once::fn") {
		if obs.ID == "o2" {
			obs.Reject("GONE", "its resource went away")
		}
	}
	fe.remove("once::fn", "o2")
	waitUntil(t, "o1 and o2 removed", func() bool {
		events := l.eventList()
		return countEvents(events, "removed:worker:o1") == 1 && countEvents(events, "removed:worker:o2") == 1
	})
	// o3: the function is unregistered
	if err := ref.Unregister(); err != nil {
		t.Fatal(err)
	}
	waitUntil(t, "o3 removed", func() bool { return countEvents(l.eventList(), "removed:worker:o3") == 1 })
	time.Sleep(30 * time.Millisecond)
	events := l.eventList()
	for _, id := range []string{"o1", "o2", "o3"} {
		if countEvents(events, "removed:function:"+id) != 1 || countEvents(events, "removed:worker:"+id) != 1 {
			t.Errorf("%s: removals %v", id, events)
		}
	}
	if countEvents(events, "removed:function:never") != 0 || countEvents(events, "removed:worker:never") != 1 {
		t.Errorf("never: only the worker's resolved add is undone: %v", events)
	}
}

func TestHooksRejectWithTheirCode(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	c.OnObserverAddedFor("cron::on-tick", func(_ context.Context, obs *Observer) error {
		if obs.ID == "panics" {
			panic("boom")
		}
		return errors.New("no such table")
	})
	registerHost(t, c, "cron::on-tick", Identity)
	r1 := fe.add("cron::on-tick", observerInfo("plain", "x::y", "", ""))
	r2 := fe.add("cron::on-tick", observerInfo("panics", "x::y", "", ""))
	if a := fe.waitAck("cron::on-tick", "plain", r1); a.Error == nil || a.Error.Code != "OBSERVE_REJECTED" || a.Error.Message != "no such table" {
		t.Errorf("plain error ack %+v", a.Error)
	}
	if a := fe.waitAck("cron::on-tick", "panics", r2); a.Error == nil || a.Error.Code != "OBSERVE_REJECTED" {
		t.Errorf("panic ack %+v", a.Error)
	}
	if c.IsObserved("cron::on-tick") {
		t.Error("a rejected observer is in the compiled set")
	}
}

// The hook of observerack-error.json: an ObserveRejection's code and message travel.
func TestHookRejectionBuildsObserverackError(t *testing.T) {
	fe := newFakeEngine(t)
	fe.setObservers("cron::on-tick", 14)
	c := fe.client()
	c.OnObserverAddedFor("cron::on-tick", func(_ context.Context, obs *Observer) error {
		return ObserveRejection("INVALID_CRON_EXPRESSION", fmt.Sprintf("invalid day-of-week field 'Mon-Fry' in '%s'", obs.Args["expression"]))
	})
	registerHost(t, c, "cron::on-tick", Identity)
	rev := fe.add("cron::on-tick", ObserverInfo{ID: "e5b7d9f1-3a2c-4e6b-8d0f-2a4c6e8b0d1f", Target: "x::y", Args: json.RawMessage(`{"expression":"0 0 9 * * Mon-Fry"}`), Filters: json.RawMessage(`{}`), Capture: []Capture{CaptureInput}, On: OnSuccess})
	got, _ := MarshalMessage(fe.waitAck("cron::on-tick", "e5b7d9f1-3a2c-4e6b-8d0f-2a4c6e8b0d1f", rev))
	assertJSONEqual(t, "observerack-error.json", got, observeFixture(t, "observerack-error.json"))
}

// A hook still running after the timeout rejects with OWNER_TIMEOUT; if it then
// succeeds, the removed hook of its level undoes what it set up.
func TestHooksOwnerTimeoutUndoesALateSuccess(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	c.hookTimeout = 40 * time.Millisecond
	var l hookLog
	cancelled := make(chan struct{})
	release := make(chan struct{})
	c.OnObserverAddedFor("slow::fn", func(ctx context.Context, obs *Observer) error {
		<-ctx.Done() // cancelled at the timeout
		close(cancelled)
		<-release
		l.add(obs.ID)
		return nil // finished late, after the timeout
	})
	c.OnObserverRemovedFor("slow::fn", func(_ context.Context, obs *Observer) { l.remove(obs.ID) })
	registerHost(t, c, "slow::fn", echoHandler)
	rev := fe.add("slow::fn", observerInfo("late", "x::y", "", ""))
	if a := fe.waitAck("slow::fn", "late", rev); a.Error == nil || a.Error.Code != "OWNER_TIMEOUT" {
		t.Fatalf("ack %+v", a.Error)
	}
	<-cancelled
	close(release)
	waitUntil(t, "late success undone", func() bool { _, removed := l.snapshot(); return len(removed) == 1 })
	if c.IsObserved("slow::fn") {
		t.Error("a timed-out observer is in the compiled set")
	}
}

// Hooks of one function run one at a time in revision order, both levels included;
// hooks of different functions run concurrently.
func TestHooksSerializedPerFunctionConcurrentAcross(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	var mu sync.Mutex
	var order []string
	running := 0
	overlap := false
	enter := func() {
		mu.Lock()
		running++
		if running > 1 {
			overlap = true
		}
		mu.Unlock()
	}
	leave := func(event string) {
		mu.Lock()
		running--
		order = append(order, event)
		mu.Unlock()
	}
	c.OnObserverAdded(func(_ context.Context, obs *Observer) error {
		if obs.FunctionID != "serial::fn" {
			return nil
		}
		enter()
		time.Sleep(5 * time.Millisecond)
		leave("worker:" + obs.ID)
		return nil
	})
	c.OnObserverAddedFor("serial::fn", func(_ context.Context, obs *Observer) error {
		enter()
		time.Sleep(10 * time.Millisecond)
		leave(obs.ID)
		return nil
	})
	registerHost(t, c, "serial::fn", echoHandler)
	for _, id := range []string{"o1", "o2", "o3"} {
		fe.add("serial::fn", observerInfo(id, "x::y", "", ""))
	}
	fe.remove("serial::fn", "o2")
	waitUntil(t, "six hooks", func() bool { mu.Lock(); defer mu.Unlock(); return len(order) == 6 })
	mu.Lock()
	want := []string{"worker:o1", "o1", "worker:o2", "o2", "worker:o3", "o3"}
	if overlap || !slices.Equal(order, want) {
		t.Errorf("order %v (want %v) overlap %v", order, want, overlap)
	}
	mu.Unlock()
	waitUntil(t, "o2 removed", func() bool { return len(c.Observers("serial::fn")) == 2 })

	// A's hook waits for B's: it would never finish if functions shared one queue.
	bRan := make(chan struct{})
	c.OnObserverAddedFor("a::fn", func(ctx context.Context, _ *Observer) error {
		select {
		case <-bRan:
			return nil
		case <-ctx.Done():
			return ctx.Err()
		}
	})
	c.OnObserverAddedFor("b::fn", func(context.Context, *Observer) error {
		close(bRan)
		return nil
	})
	registerHost(t, c, "a::fn", echoHandler)
	registerHost(t, c, "b::fn", echoHandler)
	ra := fe.add("a::fn", observerInfo("a1", "x::y", "", ""))
	rb := fe.add("b::fn", observerInfo("b1", "x::y", "", ""))
	if a := fe.waitAck("a::fn", "a1", ra); a.Error != nil {
		t.Errorf("a1 rejected: %+v", a.Error)
	}
	if b := fe.waitAck("b::fn", "b1", rb); b.Error != nil {
		t.Errorf("b1 rejected: %+v", b.Error)
	}
}

// A hook set once the function has active observers is caught up: it runs for each
// of them at once, an error late-rejects that observer, and one warning names the
// function and how many observers were active without it. An observer added later
// runs it once, without a warning; the worker's hook is caught up per function.
func TestCatchUpRunsALateHookAndWarnsOnce(t *testing.T) {
	logs := captureLog(t)
	fe := newFakeEngine(t)
	fe.setObservers("late::fn", 1, observerInfo("o1", "x::y", "", ""), observerInfo("o2", "x::y", "", ""))
	fe.setObservers("other::fn", 1, observerInfo("p1", "x::y", "", ""))
	c := fe.client()
	ref := registerHost(t, c, "late::fn", echoHandler)
	registerHost(t, c, "other::fn", echoHandler)
	fe.waitAck("late::fn", "o2", 1)
	fe.waitAck("other::fn", "p1", 1)

	var l hookLog
	ref.OnObserverAdded(func(_ context.Context, obs *Observer) error {
		l.add(obs.ID)
		if obs.ID == "o2" {
			return ObserveRejection("LATE", "o2 cannot be set up")
		}
		return nil
	})
	waitUntil(t, "caught up", func() bool { a, _ := l.snapshot(); return len(a) == 2 })
	waitUntil(t, "o2 late-rejected", func() bool {
		for _, a := range fe.acks("late::fn") {
			if a.ID == "o2" && a.Error != nil && a.Error.Code == "LATE" && a.Revision == 1 {
				return true
			}
		}
		return false
	})
	if obs := c.Observers("late::fn"); len(obs) != 1 || obs[0].ID != "o1" {
		t.Errorf("observers %v, want o1 only", obs)
	}
	const warning = `OnObserverAddedFor("late::fn") was set while 2 observers were active without it`
	waitUntil(t, "the warning", func() bool { return logs.count(warning) == 1 })

	rev := fe.add("late::fn", observerInfo("o3", "x::y", "", ""))
	fe.waitAck("late::fn", "o3", rev)
	time.Sleep(20 * time.Millisecond)
	if added, _ := l.snapshot(); !slices.Equal(added, []string{"o1", "o2", "o3"}) {
		t.Errorf("added %v", added)
	}
	if logs.count(warning) != 1 || logs.count("was set while") != 1 {
		t.Errorf("logs:\n%s", logs.String())
	}

	// the worker's hook: every function with active observers is caught up
	var worker hookLog
	c.OnObserverAdded(func(_ context.Context, obs *Observer) error {
		worker.add(obs.ID)
		return nil
	})
	waitUntil(t, "worker hook caught up", func() bool { a, _ := worker.snapshot(); return len(a) == 3 })
	waitUntil(t, "a warning per function", func() bool {
		return logs.count(`OnObserverAdded was set while 2 observers of "late::fn" were active without it`) == 1 &&
			logs.count(`OnObserverAdded was set while 1 observers of "other::fn" were active without it`) == 1
	})
}

// Hooks are stored by function id: set before the function is registered, they run
// for every observer; they stay across Unregister and run for a later registration of
// the id. Each method chains, and nil removes the hook.
func TestHooksByIDSurviveUnregisterAndChain(t *testing.T) {
	fe := newFakeEngine(t)
	fe.setObservers("re::fn", 1, observerInfo("o1", "x::y", "", ""))
	c := fe.newClient()
	var l hookLog
	if c.OnObserverAddedFor("re::fn", func(_ context.Context, obs *Observer) error { l.add(obs.ID); return nil }).
		OnObserverRemovedFor("re::fn", func(_ context.Context, obs *Observer) { l.remove(obs.ID) }) != c {
		t.Fatal("the client's hook methods return the client")
	}
	fe.connect(c)
	ref := registerHost(t, c, "re::fn", echoHandler)
	fe.waitAck("re::fn", "o1", 1)
	if err := ref.Unregister(); err != nil {
		t.Fatal(err)
	}
	waitUntil(t, "removed on unregister", func() bool { _, r := l.snapshot(); return len(r) == 1 })
	ref = registerHost(t, c, "re::fn", echoHandler)
	waitUntil(t, "added again", func() bool { a, _ := l.snapshot(); return len(a) == 2 })

	if ref.OnObserverAdded(nil).OnObserverRemoved(nil).OnObserverDeliver(nil) != ref {
		t.Fatal("the FunctionRef's hook methods return the handle")
	}
	c.mu.Lock()
	_, kept := c.functionHooks["re::fn"]
	c.mu.Unlock()
	if kept {
		t.Error("nil did not remove the hooks")
	}
	fe.remove("re::fn", "o1")
	time.Sleep(20 * time.Millisecond)
	if _, r := l.snapshot(); len(r) != 1 {
		t.Errorf("a removed hook ran: removed %v", r)
	}
}

// Once the connection is registered, a hook id this worker has not registered is
// reported once, which catches a mistyped id.
func TestUnregisteredHookIDIsReportedOnce(t *testing.T) {
	logs := captureLog(t)
	fe := newFakeEngine(t)
	c := fe.newClient()
	c.OnObserverAddedFor("orders::craete", func(context.Context, *Observer) error { return nil })
	c.OnObserverDeliverFor("orders::create", nil)
	c.OnObserverRemovedFor("orders::create", func(context.Context, *Observer) {})
	if _, err := c.RegisterFunction("orders::create", echoHandler); err != nil {
		t.Fatal(err)
	}
	fe.connect(c)
	const warning = `observer hooks are set for "orders::craete", which this worker has not registered`
	waitUntil(t, "the warning", func() bool { return logs.count(warning) == 1 })
	fe.reconnect(c)
	time.Sleep(20 * time.Millisecond)
	if logs.count(warning) != 1 || logs.count(`"orders::create", which`) != 0 {
		t.Errorf("logs:\n%s", logs.String())
	}
}

// Accepted resolves once the SDK acked the observer, and a call through the observer
// made after it goes out behind the ack. It fails with the rejection for a rejected
// observer, and waits while the added hooks run.
func TestAcceptedResolvesAfterTheAck(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	observers := make(chan *Observer, 2)
	c.OnObserverAddedFor("prices::on-change", func(_ context.Context, obs *Observer) error {
		ctx, cancel := context.WithTimeout(context.Background(), 20*time.Millisecond)
		defer cancel()
		if err := obs.Accepted(ctx); !errors.Is(err, context.DeadlineExceeded) {
			t.Errorf("Accepted while the hook runs: %v", err)
		}
		observers <- obs
		if obs.ID == "bad" {
			return ObserveRejection("NOPE", "no prices for you")
		}
		return nil
	})
	registerHost(t, c, "prices::on-change", Identity, RegisterFunctionOptions{Private: true})
	r1 := fe.add("prices::on-change", observerInfo("good", "ui::price", "", ""))
	good := <-observers
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	if err := good.Accepted(ctx); err != nil {
		t.Fatalf("Accepted: %v", err)
	}
	if _, err := c.Call(ctx, CallRequest{Observer: good, Payload: json.RawMessage(`{"price":12}`), Action: VoidAction()}); err != nil {
		t.Fatal(err)
	}
	fe.waitFrames(MsgInvokeFunction, 1, func(d *DecodedMessage) bool { return d.InvokeFunction.Activation != nil })
	ackAt := fe.frameIndex(func(d *DecodedMessage) bool {
		return d.Type == MsgObserverAck && d.ObserverAck.ID == "good" && d.ObserverAck.Revision == r1
	})
	callAt := fe.frameIndex(func(d *DecodedMessage) bool { return d.Type == MsgInvokeFunction && d.InvokeFunction.Activation != nil })
	if ackAt < 0 || callAt < ackAt {
		t.Errorf("the call through the observer (%d) must follow its ack (%d)", callAt, ackAt)
	}

	fe.add("prices::on-change", observerInfo("bad", "ui::price", "", ""))
	bad := <-observers
	var rej *ObserveRejectionError
	if err := bad.Accepted(ctx); !errors.As(err, &rej) || rej.Code != "NOPE" {
		t.Errorf("Accepted of a rejected observer: %v", err)
	}
	if err := (&Observer{}).Accepted(ctx); err == nil {
		t.Error("Accepted on an observer the SDK did not hand out")
	}
}

// After a reconnect the fresh functionregistered is diffed: removed -> removed hooks,
// new -> added hooks, kept -> acked again without the hooks; an observer whose args
// changed is removed, then added.
func TestFunctionRegisteredDiffAfterReconnect(t *testing.T) {
	fe := newFakeEngine(t)
	fe.setObservers("diff::fn", 2,
		observerInfo("gone", "x::y", `{"k":1}`, ""),
		observerInfo("kept", "x::y", `{"k":2}`, ""),
		observerInfo("changed", "x::y", `{"k":3}`, ""))
	c := fe.newClient()
	var l hookLog
	recordHooks(c, "diff::fn", &l)
	fe.connect(c)
	registerHost(t, c, "diff::fn", echoHandler)
	for _, id := range []string{"gone", "kept", "changed"} {
		fe.waitAck("diff::fn", id, 2)
	}
	keptBefore := c.Observers("diff::fn")

	fe.setObservers("diff::fn", 0,
		observerInfo("kept", "x::y", `{"k":2}`, ""),
		observerInfo("changed", "x::y", `{"k":30}`, ""),
		observerInfo("new", "x::y", `{"k":4}`, ""))
	fe.reconnect(c)
	for _, id := range []string{"kept", "changed", "new"} {
		if a := fe.waitAck("diff::fn", id, 0); a.Error != nil {
			t.Errorf("%s: %+v", id, a.Error)
		}
	}
	waitUntil(t, "diff applied", func() bool { _, r := l.snapshot(); return len(r) == 2 })
	added, removed := l.snapshot()
	if want := []string{"gone", "kept", "changed", "changed", "new"}; !slices.Equal(added, want) {
		t.Fatalf("added %v, want %v", added, want)
	}
	if len(removed) != 2 || removed[0] != "gone" || removed[1] != "changed" {
		t.Errorf("removed %v, want [gone changed]", removed)
	}
	var kept *Observer
	for _, o := range c.Observers("diff::fn") {
		if o.ID == "kept" {
			kept = o
		}
	}
	for _, o := range keptBefore {
		if o.ID == "kept" && o != kept {
			t.Error("a kept observer must keep its identity (hosts hold it in their index)")
		}
	}
}

// An added / removed message whose revision is not greater than the one held on the
// connection is ignored.
func TestHooksIgnoreStaleRevisions(t *testing.T) {
	fe := newFakeEngine(t)
	fe.setObservers("rev::fn", 5)
	c := fe.client()
	var l hookLog
	recordHooks(c, "rev::fn", &l)
	registerHost(t, c, "rev::fn", echoHandler)
	fe.send(&ObserverAddedMessage{FunctionID: "rev::fn", Namespace: "default", Revision: 5, Observer: observerInfo("stale", "x::y", "", "")})
	fe.send(&ObserverAddedMessage{FunctionID: "rev::fn", Namespace: "default", Revision: 6, Observer: observerInfo("fresh", "x::y", "", "")})
	fe.waitAck("rev::fn", "fresh", 6)
	fe.send(&ObserverRemovedMessage{FunctionID: "rev::fn", Namespace: "default", Revision: 6, ID: "fresh"})
	time.Sleep(30 * time.Millisecond)
	added, removed := l.snapshot()
	if len(added) != 1 || added[0] != "fresh" || len(removed) != 0 {
		t.Errorf("added %v removed %v", added, removed)
	}
	fe.send(&ObserverRemovedMessage{FunctionID: "rev::fn", Namespace: "default", Revision: 7, ID: "fresh"})
	waitUntil(t, "removed", func() bool { _, r := l.snapshot(); return len(r) == 1 })
	if c.IsObserved("rev::fn") {
		t.Error("still observed")
	}
}

// FUNCTION_NAMESPACE_CONFLICT turns local dispatch off and drops the observer set
// through the removed hooks; the worker stays connected.
func TestNamespaceConflictDropsObserversAndLocalDispatch(t *testing.T) {
	fe := newFakeEngine(t)
	fe.setObservers("conflict::fn", 1, observerInfo("o1", "x::y", "", ""))
	c := fe.client()
	var l hookLog
	recordHooks(c, "conflict::fn", &l)
	registerHost(t, c, "conflict::fn", echoHandler)
	fe.waitAck("conflict::fn", "o1", 1)
	if !c.LocalDispatchActive("conflict::fn") || !c.IsObserved("conflict::fn") {
		t.Fatal("not confirmed")
	}
	fe.send(&RegistrationRejectedMessage{Code: "FUNCTION_NAMESPACE_CONFLICT", Namespace: "default", FunctionID: "conflict::fn", OwnerWorkerID: "other"})
	waitUntil(t, "removed", func() bool { _, r := l.snapshot(); return len(r) == 1 })
	if c.LocalDispatchActive("conflict::fn") || c.IsObserved("conflict::fn") {
		t.Error("local dispatch or observers survived the conflict")
	}
	if c.ConnectionState() != StateConnected {
		t.Error("a function conflict is not fatal")
	}
}

// A registration the engine refuses for one function (INVALID_ARGS_FORMAT,
// UNSUPPORTED) costs that function only: it is logged, and the worker stays connected.
func TestFunctionRejectionIsNotFatal(t *testing.T) {
	logs := captureLog(t)
	fe := newFakeEngine(t)
	c := fe.client()
	registerHost(t, c, "queue::on-message", Identity, RegisterFunctionOptions{ArgsFormat: json.RawMessage(`{"type":"nope"}`)})
	fe.sendRaw(string(observeFixture(t, "registrationrejected-invalid-args-format.json")))
	waitUntil(t, "released", func() bool { return !c.LocalDispatchActive("queue::on-message") })
	waitUntil(t, "logged", func() bool {
		return logs.count(`function registration rejected (INVALID_ARGS_FORMAT): "queue::on-message"`) == 1
	})
	if c.ConnectionState() != StateConnected || c.FatalError() != nil {
		t.Errorf("state %s, fatal %v", c.ConnectionState(), c.FatalError())
	}
}

// Standby: on FUNCTION_NAMESPACE_CONFLICT the registration is sent again, about once
// an interval, until the engine accepts it; the hooks then run for the current
// observers. Without Standby there is no retry.
func TestStandbyRegistersWhenTheIDIsFree(t *testing.T) {
	logs := captureLog(t)
	fe := newFakeEngine(t)
	fe.setObservers("sb::fn", 1, observerInfo("o1", "x::y", "", ""))
	fe.mu.Lock()
	fe.conflicts["sb::fn"] = 2
	fe.conflicts["nosb::fn"] = 1
	fe.mu.Unlock()
	c := fe.client()
	c.standbyInterval = 20 * time.Millisecond
	var l hookLog
	recordHooks(c, "sb::fn", &l)
	registerHost(t, c, "sb::fn", echoHandler, RegisterFunctionOptions{Standby: true})
	if a := fe.waitAck("sb::fn", "o1", 1); a.Error != nil {
		t.Fatalf("o1 %+v", a.Error)
	}
	if added, _ := l.snapshot(); !slices.Equal(added, []string{"o1"}) {
		t.Errorf("added %v", added)
	}
	if n := countRegister(fe.m.receivedMessages(), string(MsgRegisterFunction), "sb::fn"); n != 3 {
		t.Errorf("%d registrations, want 3 (two refused, one accepted)", n)
	}
	if logs.count(`function "sb::fn" in namespace "default" is hosted by worker other-host; standing by`) != 1 {
		t.Errorf("logs:\n%s", logs.String())
	}

	if _, err := c.RegisterFunction("nosb::fn", echoHandler); err != nil {
		t.Fatal(err)
	}
	waitUntil(t, "refused", func() bool { return logs.count(`"nosb::fn" in namespace "default" already owned`) == 1 })
	time.Sleep(80 * time.Millisecond)
	if n := countRegister(fe.m.receivedMessages(), string(MsgRegisterFunction), "nosb::fn"); n != 1 {
		t.Errorf("%d registrations of a function without Standby, want 1", n)
	}
}

func TestOnObserverRemovedFailureIsLogged(t *testing.T) {
	logs := captureLog(t)
	fe := newFakeEngine(t)
	fe.setObservers("rm::fn", 1, observerInfo("o1", "x::y", "", ""), observerInfo("o2", "x::y", "", ""))
	c := fe.newClient()
	var removed atomic.Int32
	c.OnObserverRemovedFor("rm::fn", func(_ context.Context, obs *Observer) {
		removed.Add(1)
		if obs.ID == "o1" {
			panic("index corrupted")
		}
	})
	fe.connect(c)
	registerHost(t, c, "rm::fn", echoHandler)
	fe.waitAck("rm::fn", "o2", 1)
	fe.remove("rm::fn", "o1")
	fe.remove("rm::fn", "o2")
	waitUntil(t, "both removed", func() bool { return removed.Load() == 2 })
	waitUntil(t, "logged", func() bool { return logs.count("OnObserverRemoved failed for observer o1") == 1 })
}

// ArgsFormat, like the other formats, must be valid JSON; a refused registration is
// not stored. Observable false travels, true is the default and stays off the wire.
func TestRegisterFunctionOptionsValidation(t *testing.T) {
	c := New("ws://unused.invalid")
	t.Cleanup(func() { _ = c.Close() })
	_, err := c.RegisterFunction("bad::fn", echoHandler, RegisterFunctionOptions{ArgsFormat: json.RawMessage(`{"type":`)})
	if err == nil || !strings.Contains(err.Error(), "ArgsFormat must contain valid JSON") {
		t.Errorf("invalid args_format: %v", err)
	}
	_, err = RegisterFunctionTyped(c, "typed::fn", func(context.Context, struct{}) (struct{}, error) { return struct{}{}, nil },
		RegisterFunctionOptions{ArgsFormat: json.RawMessage(`nope`)})
	if err == nil {
		t.Error("typed registration with an invalid ArgsFormat")
	}
	if c.functionObservers("bad::fn") != nil || c.functionObservers("typed::fn") != nil {
		t.Error("a refused registration was stored")
	}
	open, closed := true, false
	for _, tc := range []struct {
		opts RegisterFunctionOptions
		want string
	}{
		{RegisterFunctionOptions{Observable: &open}, `{"id":"f"}`},
		{RegisterFunctionOptions{Observable: &closed}, `{"id":"f","observable":false}`},
		{RegisterFunctionOptions{Private: true, Standby: true}, `{"id":"f","private":true}`},
		{RegisterFunctionOptions{ArgsFormat: json.RawMessage(`{"type":"object"}`)}, `{"id":"f","args_format":{"type":"object"}}`},
	} {
		msg, err := registerFunctionMessage("RegisterFunction", "f", tc.opts)
		if err != nil {
			t.Fatal(err)
		}
		got, _ := json.Marshal(msg)
		if !jsonEq(t, got, []byte(tc.want)) {
			t.Errorf("%+v: %s, want %s", tc.opts, got, tc.want)
		}
	}
}

// A function that needs observe.v2 waits for WorkerRegistered: its registerfunction
// follows the worker's announcement, while a plain function goes out on connect.
func TestRegistrationsThatNeedObserveV2WaitForWorkerRegistered(t *testing.T) {
	fe := newFakeEngine(t)
	gate := make(chan struct{})
	fe.mu.Lock()
	fe.gate = gate
	fe.mu.Unlock()
	c := fe.newClient()
	if _, err := c.RegisterFunction("plain::fn", echoHandler); err != nil {
		t.Fatal(err)
	}
	if _, err := c.RegisterFunction("source::fn", Identity, RegisterFunctionOptions{Private: true}); err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	if err := c.Connect(ctx); err != nil {
		t.Fatal(err)
	}
	close(gate)
	waitUntil(t, "registered", c.registeredNow)
	waitFrame(t, fe.m, MsgRegisterFunction, "source::fn")
	msgs := fe.m.receivedMessages()
	plain := firstIndex(msgs, func(m map[string]json.RawMessage) bool {
		return messageType(m) == string(MsgRegisterFunction) && messageID(m) == "plain::fn"
	})
	meta := indexOfFunctionID(msgs, FnRegisterWorker)
	source := firstIndex(msgs, func(m map[string]json.RawMessage) bool {
		return messageType(m) == string(MsgRegisterFunction) && messageID(m) == "source::fn"
	})
	if !(plain < meta && meta < source) {
		t.Errorf("plain %d, worker metadata %d, source %d: the source must follow WorkerRegistered", plain, meta, source)
	}
}

// Against an engine without observe.v2 the functions that need it (args_format,
// private, observable false, a deliver hook) stay unregistered and UNSUPPORTED is
// logged once per function, also across reconnects; everything else registers. A
// deliver hook set on a registered function unregisters it, and removing the hook
// registers it again. An engine with observe.v2 registers them all.
func TestFunctionsThatNeedObserveV2AreUnsupportedOnOlderEngines(t *testing.T) {
	logs := captureLog(t)
	fe := newFakeEngine(t).v1Engine()
	c := fe.newClient()
	deliverAll := func(ctx context.Context, call *Call, observers []*Observer, next Next) (any, error) {
		return next(ctx, call, observers)
	}
	closed := false
	c.OnObserverDeliverFor("hooked::fn", deliverAll)
	for id, opts := range map[string]RegisterFunctionOptions{
		"args::fn":    {ArgsFormat: json.RawMessage(`{"type":"object"}`)},
		"private::fn": {Private: true},
		"closed::fn":  {Observable: &closed},
		"hooked::fn":  {},
		"plain::fn":   {},
	} {
		if _, err := c.RegisterFunction(id, echoHandler, opts); err != nil {
			t.Fatal(err)
		}
	}
	fe.connect(c)
	msgs := sentinel(t, fe.m, c, "sentinel::v1")
	unsupported := []string{"args::fn", "private::fn", "closed::fn", "hooked::fn"}
	for _, id := range unsupported {
		if n := countRegister(msgs, string(MsgRegisterFunction), id); n != 0 {
			t.Errorf("%s registered on an engine without observe.v2", id)
		}
		if n := logs.count(fmt.Sprintf(`UNSUPPORTED: function %q`, id)); n != 1 {
			t.Errorf("%s: UNSUPPORTED logged %d times", id, n)
		}
	}
	if countRegister(msgs, string(MsgRegisterFunction), "plain::fn") != 1 {
		t.Error("plain::fn not registered")
	}
	if _, err := c.RegisterFunction("later::fn", Identity, RegisterFunctionOptions{Private: true}); err != nil {
		t.Fatal(err)
	}
	waitUntil(t, "later::fn UNSUPPORTED", func() bool { return logs.count(`UNSUPPORTED: function "later::fn"`) == 1 })

	// a deliver hook set on a registered function: unregistered; removed: registered again
	c.OnObserverDeliverFor("plain::fn", deliverAll)
	waitFrame(t, fe.m, MsgUnregisterFunction, "plain::fn")
	if logs.count(`UNSUPPORTED: function "plain::fn"`) != 1 {
		t.Error("no UNSUPPORTED for plain::fn")
	}
	c.OnObserverDeliverFor("plain::fn", nil)
	waitUntil(t, "plain::fn registered again", func() bool {
		return countRegister(fe.m.receivedMessages(), string(MsgRegisterFunction), "plain::fn") == 2
	})

	fe.reconnect(c)
	sentinel(t, fe.m, c, "sentinel::v1-again")
	if logs.count("UNSUPPORTED") != 6 {
		t.Errorf("UNSUPPORTED logged again after a reconnect:\n%s", logs.String())
	}

	fe.setFeatures(FeatureObserveV1, FeatureObserveV2, FeatureLocalDispatchV1)
	mark := fe.mark()
	fe.reconnect(c)
	for _, id := range append(unsupported, "later::fn") {
		waitUntil(t, id+" registered on observe.v2", func() bool {
			return countRegister(fe.m.receivedMessages()[mark:], string(MsgRegisterFunction), id) == 1
		})
	}
}

// A late rejection leaves the compiled set at once, carries the revision the engine
// last announced, survives a reconnect (re-acked with the error on the replayed
// functionregistered) until a fresh observeradded starts the observer over.
func TestLateRejectionSurvivesReconnect(t *testing.T) {
	fe := newFakeEngine(t)
	fe.setObservers("late::fn", 3, observerInfo("o1", "x::y", "", ""))
	c := fe.client()
	var l hookLog
	recordHooks(c, "late::fn", &l)
	registerHost(t, c, "late::fn", echoHandler)
	fe.waitAck("late::fn", "o1", 3)
	fe.add("late::fn", observerInfo("o2", "x::y", "", "")) // revision 4
	fe.waitAck("late::fn", "o2", 4)
	obs := c.Observers("late::fn")[0]
	if obs.ID != "o1" {
		t.Fatalf("observers %v", c.Observers("late::fn"))
	}

	// rejected while the socket is down: the ack cannot go out now
	gate := make(chan struct{})
	fe.mu.Lock()
	fe.gate = gate
	fe.mu.Unlock()
	fe.m.closeActiveConnection()
	waitUntil(t, "offline", func() bool { return !c.registeredNow() })
	obs.Reject("UNKNOWN_DB", "primary was removed")
	if len(c.Observers("late::fn")) != 1 {
		t.Error("a rejected observer stays in the compiled set")
	}
	fe.setObservers("late::fn", 7, observerInfo("o1", "x::y", "", ""), observerInfo("o2", "x::y", "", ""))
	close(gate)
	waitUntil(t, "registered", c.registeredNow)
	a := fe.waitAck("late::fn", "o1", 7)
	if a.Error == nil || a.Error.Code != "UNKNOWN_DB" {
		t.Errorf("replayed rejection ack %+v", a.Error)
	}
	if b := fe.waitAck("late::fn", "o2", 7); b.Error != nil {
		t.Errorf("o2 %+v", b.Error)
	}
	if len(c.Observers("late::fn")) != 1 {
		t.Errorf("observers %v", c.Observers("late::fn"))
	}
	// a late rejection on the live connection carries the last announced revision (7)
	o2 := c.Observers("late::fn")[0]
	mark := len(fe.acks("late::fn"))
	o2.Reject("UNKNOWN_DB", "gone too")
	waitUntil(t, "late ack", func() bool { return len(fe.acks("late::fn")) > mark })
	if last := fe.acks("late::fn")[mark]; last.ID != "o2" || last.Revision != 7 || last.Error.Code != "UNKNOWN_DB" {
		t.Errorf("late rejection %+v", last)
	}
	// a fresh observeradded starts o1 over: removed, then added again
	fe.send(&ObserverAddedMessage{FunctionID: "late::fn", Namespace: "default", Revision: 8, Observer: observerInfo("o1", "x::y", "", "")})
	if a := fe.waitAck("late::fn", "o1", 8); a.Error != nil {
		t.Errorf("reactivated %+v", a.Error)
	}
	waitUntil(t, "o1 active", func() bool { return len(c.Observers("late::fn")) == 1 && c.Observers("late::fn")[0].ID == "o1" })
	added, removed := l.snapshot()
	if len(added) != 3 || added[2] != "o1" || len(removed) != 1 || removed[0] != "o1" {
		t.Errorf("hooks: added %v removed %v", added, removed)
	}
}

// A Reject made while an added hook is still running wins: the add acks with it
// rather than activating, and the engine's observerremoved runs the removed hooks to
// undo what the added hooks set up.
func TestRejectDuringOnObserverAddedWins(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	var l hookLog
	c.OnObserverAddedFor("rej::fn", func(_ context.Context, obs *Observer) error {
		l.add(obs.ID)
		obs.Reject("UNKNOWN_DB", "the db went away during setup")
		return nil
	})
	c.OnObserverRemovedFor("rej::fn", func(_ context.Context, obs *Observer) { l.remove(obs.ID) })
	registerHost(t, c, "rej::fn", echoHandler)
	rev := fe.add("rej::fn", observerInfo("o1", "x::y", "", ""))
	a := fe.waitAck("rej::fn", "o1", rev)
	if a.Error == nil || a.Error.Code != "UNKNOWN_DB" {
		t.Fatalf("ack %+v, want the rejection", a.Error)
	}
	if c.IsObserved("rej::fn") {
		t.Error("an observer rejected during its hook was activated")
	}
	fe.remove("rej::fn", "o1")
	waitUntil(t, "OnObserverRemoved", func() bool { _, r := l.snapshot(); return len(r) == 1 })
}

// Value semantics: an Observer keeps its own copy of the engine's capture list.
func TestObserverDoesNotShareItsSources(t *testing.T) {
	f := newFunctionObservers(New("ws://unused.invalid"), "own::fn", true, false)
	info := observerInfo("o1", "x::y", "", "", CaptureInput, CaptureCaller)
	obs, err := newObserver(f, info, 1)
	if err != nil {
		t.Fatal(err)
	}
	info.Capture[0] = CaptureMetadata
	if obs.Capture[0] != CaptureInput || obs.capture[0] != CaptureInput {
		t.Error("the observer shares the decoded capture slice")
	}
}

func firstIndex(msgs []map[string]json.RawMessage, pred func(map[string]json.RawMessage) bool) int {
	for i, m := range msgs {
		if pred(m) {
			return i
		}
	}
	return -1
}

package iii

import (
	"context"
	"encoding/json"
	"errors"
	"sync"
	"sync/atomic"
	"testing"
	"time"
)

// hookLog records hook calls for one test.
type hookLog struct {
	mu      sync.Mutex
	added   []string
	removed []string
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

func (l *hookLog) snapshot() (added, removed []string) {
	l.mu.Lock()
	defer l.mu.Unlock()
	return append([]string(nil), l.added...), append([]string(nil), l.removed...)
}

func recordingHooks(l *hookLog, delivery Delivery) *ObserveHooks {
	return &ObserveHooks{
		Delivery: delivery,
		OnObserverAdded: func(_ context.Context, obs *Observer) error {
			l.add(obs.ID)
			return nil
		},
		OnObserverRemoved: func(_ context.Context, obs *Observer) { l.remove(obs.ID) },
	}
}

func registerOwner(t *testing.T, fe *fakeEngine, c *Client, id string, hooks *ObserveHooks) {
	t.Helper()
	registerOwnerWith(t, c, id, echoHandler, hooks)
}

func registerOwnerWith(t *testing.T, c *Client, id string, handler Handler, hooks *ObserveHooks) {
	t.Helper()
	if _, err := c.RegisterFunction(id, handler, RegisterFunctionOptions{Observe: hooks}); err != nil {
		t.Fatal(err)
	}
	waitUntil(t, "functionregistered for "+id, func() bool {
		f := c.functionObservers(id)
		return f != nil && f.syncedOn(c.currentEpoch())
	})
}

// sdk.md §1, §4.2: OnObserverAdded runs once per add, before the ack, and the observer
// is in the compiled set when the ack goes out. The ack is observerack.json.
func TestHooksRunOncePerAddBeforeTheAck(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	var calls atomic.Int32
	var observedAtHook, observedAtAck atomic.Bool
	registerOwner(t, fe, c, "cron::on-tick", &ObserveHooks{
		Delivery: DeliveryOwner,
		OnObserverAdded: func(_ context.Context, obs *Observer) error {
			calls.Add(1)
			observedAtHook.Store(c.IsObserved("cron::on-tick"))
			if obs.Args["expression"] != "0 0 9 * * Mon-Fri" || obs.Target != "billing::send-reminders" || obs.FunctionID != "cron::on-tick" {
				t.Errorf("observer %+v", obs)
			}
			return nil
		},
	})
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
	if len(obs) != 1 || !obs[0].Matches(ObserveDoc{Caller: &Caller{WorkerName: "billing"}}) || obs[0].Matches(ObserveDoc{Caller: &Caller{WorkerName: "replayer"}}) {
		t.Errorf("observers %v: the normalized filters must apply", obs)
	}
	// the same revision again is ignored: no second hook call
	fe.sendRaw(string(observeFixture(t, "observeradded.json")))
	time.Sleep(30 * time.Millisecond)
	if calls.Load() != 1 {
		t.Errorf("hook ran %d times", calls.Load())
	}
}

func TestHooksRejectWithTheirCode(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	errs := map[string]error{
		"a07d4c2e-6b1f-4a3d-9e8c-5f2a1b3c4d5e": ObserveRejection("INVALID_CRON_EXPRESSION", "invalid day-of-week field"),
		"plain":                                errors.New("no such table"),
	}
	registerOwner(t, fe, c, "cron::on-tick", &ObserveHooks{
		Delivery: DeliveryOwner,
		OnObserverAdded: func(_ context.Context, obs *Observer) error {
			if obs.ID == "panics" {
				panic("boom")
			}
			return errs[obs.ID]
		},
	})
	fe.sendRaw(string(observeFixture(t, "observeradded.json")))
	ack := fe.waitAck("cron::on-tick", "a07d4c2e-6b1f-4a3d-9e8c-5f2a1b3c4d5e", 13)
	got, _ := MarshalMessage(ack)
	assertJSONEqual(t, "observerack-error.json", got, observeFixture(t, "observerack-error.json"))

	fe.setObservers("cron::on-tick", 13)
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

// CONTRACT D11: a hook still running after the timeout rejects with OWNER_TIMEOUT;
// if it then succeeds, OnObserverRemoved undoes what it indexed.
func TestHooksOwnerTimeoutUndoesALateSuccess(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	c.hookTimeout = 40 * time.Millisecond
	var log hookLog
	cancelled := make(chan struct{})
	release := make(chan struct{})
	registerOwner(t, fe, c, "slow::fn", &ObserveHooks{
		Delivery: DeliveryOwner,
		OnObserverAdded: func(ctx context.Context, obs *Observer) error {
			<-ctx.Done() // cancelled at the timeout
			close(cancelled)
			<-release
			log.add(obs.ID)
			return nil // finished late, after the timeout
		},
		OnObserverRemoved: func(_ context.Context, obs *Observer) { log.remove(obs.ID) },
	})
	rev := fe.add("slow::fn", observerInfo("late", "x::y", "", ""))
	if a := fe.waitAck("slow::fn", "late", rev); a.Error == nil || a.Error.Code != "OWNER_TIMEOUT" {
		t.Fatalf("ack %+v", a.Error)
	}
	<-cancelled
	close(release)
	waitUntil(t, "late success undone", func() bool { _, removed := log.snapshot(); return len(removed) == 1 })
	if c.IsObserved("slow::fn") {
		t.Error("a timed-out observer is in the compiled set")
	}
}

// sdk.md §1: hooks of one function run one at a time in revision order; hooks of
// different functions run concurrently.
func TestHooksSerializedPerFunctionConcurrentAcross(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	var mu sync.Mutex
	var order []string
	running := 0
	overlap := false
	serial := &ObserveHooks{
		Delivery: DeliveryOwner,
		OnObserverAdded: func(_ context.Context, obs *Observer) error {
			mu.Lock()
			running++
			if running > 1 {
				overlap = true
			}
			mu.Unlock()
			time.Sleep(15 * time.Millisecond)
			mu.Lock()
			running--
			order = append(order, obs.ID)
			mu.Unlock()
			return nil
		},
	}
	registerOwner(t, fe, c, "serial::fn", serial)
	for _, id := range []string{"o1", "o2", "o3"} {
		fe.add("serial::fn", observerInfo(id, "x::y", "", ""))
	}
	fe.remove("serial::fn", "o2")
	waitUntil(t, "three hooks", func() bool { mu.Lock(); defer mu.Unlock(); return len(order) == 3 })
	mu.Lock()
	if overlap || order[0] != "o1" || order[1] != "o2" || order[2] != "o3" {
		t.Errorf("order %v overlap %v", order, overlap)
	}
	mu.Unlock()
	waitUntil(t, "o2 removed", func() bool { return len(c.Observers("serial::fn")) == 2 })

	// A's hook waits for B's: it would never finish if functions shared one queue.
	bRan := make(chan struct{})
	registerOwner(t, fe, c, "a::fn", &ObserveHooks{Delivery: DeliveryOwner, OnObserverAdded: func(ctx context.Context, _ *Observer) error {
		select {
		case <-bRan:
			return nil
		case <-ctx.Done():
			return ctx.Err()
		}
	}})
	registerOwner(t, fe, c, "b::fn", &ObserveHooks{Delivery: DeliveryOwner, OnObserverAdded: func(context.Context, *Observer) error {
		close(bRan)
		return nil
	}})
	ra := fe.add("a::fn", observerInfo("a1", "x::y", "", ""))
	rb := fe.add("b::fn", observerInfo("b1", "x::y", "", ""))
	if a := fe.waitAck("a::fn", "a1", ra); a.Error != nil {
		t.Errorf("a1 rejected: %+v", a.Error)
	}
	if b := fe.waitAck("b::fn", "b1", rb); b.Error != nil {
		t.Errorf("b1 rejected: %+v", b.Error)
	}
}

// protocol.md §2.3: an added / removed message whose revision is not greater than the
// one held on the connection is ignored.
func TestHooksIgnoreStaleRevisions(t *testing.T) {
	fe := newFakeEngine(t)
	fe.setObservers("rev::fn", 5)
	c := fe.client()
	var log hookLog
	registerOwner(t, fe, c, "rev::fn", recordingHooks(&log, DeliveryOwner))
	fe.send(&ObserverAddedMessage{FunctionID: "rev::fn", Namespace: "default", Revision: 5, Observer: observerInfo("stale", "x::y", "", "")})
	fe.send(&ObserverAddedMessage{FunctionID: "rev::fn", Namespace: "default", Revision: 6, Observer: observerInfo("fresh", "x::y", "", "")})
	fe.waitAck("rev::fn", "fresh", 6)
	fe.send(&ObserverRemovedMessage{FunctionID: "rev::fn", Namespace: "default", Revision: 6, ID: "fresh"})
	time.Sleep(30 * time.Millisecond)
	added, removed := log.snapshot()
	if len(added) != 1 || added[0] != "fresh" || len(removed) != 0 {
		t.Errorf("added %v removed %v", added, removed)
	}
	fe.send(&ObserverRemovedMessage{FunctionID: "rev::fn", Namespace: "default", Revision: 7, ID: "fresh"})
	waitUntil(t, "removed", func() bool { _, r := log.snapshot(); return len(r) == 1 })
	if c.IsObserved("rev::fn") {
		t.Error("still observed")
	}
}

// sdk.md §4.2, §4.5: after a reconnect, the fresh functionregistered is diffed:
// removed -> OnObserverRemoved, new -> OnObserverAdded, kept -> acked again without the
// hook; an observer whose args changed is removed, then added.
func TestFunctionRegisteredDiffAfterReconnect(t *testing.T) {
	fe := newFakeEngine(t)
	fe.setObservers("diff::fn", 2,
		observerInfo("gone", "x::y", `{"k":1}`, ""),
		observerInfo("kept", "x::y", `{"k":2}`, ""),
		observerInfo("changed", "x::y", `{"k":3}`, ""))
	c := fe.client()
	var log hookLog
	registerOwner(t, fe, c, "diff::fn", recordingHooks(&log, DeliveryOwner))
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
	waitUntil(t, "diff applied", func() bool { _, r := log.snapshot(); return len(r) == 2 })
	added, removed := log.snapshot()
	wantAdded := []string{"gone", "kept", "changed", "changed", "new"}
	if len(added) != len(wantAdded) {
		t.Fatalf("added %v, want %v", added, wantAdded)
	}
	for i := range wantAdded {
		if added[i] != wantAdded[i] {
			t.Fatalf("added %v, want %v", added, wantAdded)
		}
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
			t.Error("a kept observer must keep its identity (owners hold it in their index)")
		}
	}
}

// sdk.md §4.1: FUNCTION_NAMESPACE_CONFLICT turns local dispatch off and drops the
// observer set through OnObserverRemoved.
func TestNamespaceConflictDropsObserversAndLocalDispatch(t *testing.T) {
	fe := newFakeEngine(t)
	fe.setObservers("conflict::fn", 1, observerInfo("o1", "x::y", "", ""))
	c := fe.client()
	var log hookLog
	registerOwner(t, fe, c, "conflict::fn", recordingHooks(&log, DeliveryOwner))
	fe.waitAck("conflict::fn", "o1", 1)
	if !c.LocalDispatchActive("conflict::fn") || !c.IsObserved("conflict::fn") {
		t.Fatal("not confirmed")
	}
	fe.send(&RegistrationRejectedMessage{Code: "FUNCTION_NAMESPACE_CONFLICT", Namespace: "default", FunctionID: "conflict::fn", OwnerWorkerID: "other"})
	waitUntil(t, "removed", func() bool { _, r := log.snapshot(); return len(r) == 1 })
	if c.LocalDispatchActive("conflict::fn") || c.IsObserved("conflict::fn") {
		t.Error("local dispatch or observers survived the conflict")
	}
	if c.State() != StateConnected {
		t.Error("a function conflict is not fatal")
	}
}

// A function registered again under the same id replaces the previous one: its
// observers go through the previous hooks' OnObserverRemoved.
func TestReRegisteringAFunctionReplacesItsObserveState(t *testing.T) {
	fe := newFakeEngine(t)
	fe.setObservers("re::fn", 1, observerInfo("o1", "x::y", "", ""))
	c := fe.client()
	var first, second hookLog
	registerOwner(t, fe, c, "re::fn", recordingHooks(&first, DeliveryOwner))
	fe.waitAck("re::fn", "o1", 1)
	registerOwner(t, fe, c, "re::fn", recordingHooks(&second, DeliveryOwner))
	waitUntil(t, "previous hooks removed", func() bool { _, r := first.snapshot(); return len(r) == 1 })
	waitUntil(t, "new hooks added", func() bool { a, _ := second.snapshot(); return len(a) == 1 })
}

func TestOnObserverRemovedFailureIsLogged(t *testing.T) {
	logs := captureLog(t)
	fe := newFakeEngine(t)
	fe.setObservers("rm::fn", 1, observerInfo("o1", "x::y", "", ""), observerInfo("o2", "x::y", "", ""))
	c := fe.client()
	var removed atomic.Int32
	registerOwner(t, fe, c, "rm::fn", &ObserveHooks{OnObserverRemoved: func(_ context.Context, obs *Observer) {
		removed.Add(1)
		if obs.ID == "o1" {
			panic("index corrupted")
		}
	}})
	fe.waitAck("rm::fn", "o2", 1)
	fe.remove("rm::fn", "o1")
	fe.remove("rm::fn", "o2")
	waitUntil(t, "both removed", func() bool { return removed.Load() == 2 })
	waitUntil(t, "logged", func() bool { return logs.count("OnObserverRemoved failed for observer o1") == 1 })
}

// protocol.md §6: an observer the legacy shim created carries the old trigger config.
func TestLegacyShimObserverCarriesTheOldConfig(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	got := make(chan *Observer, 1)
	registerOwner(t, fe, c, "http::on-request", &ObserveHooks{
		Delivery: DeliveryOwner, LegacyTriggerType: "http",
		OnObserverAdded: func(_ context.Context, obs *Observer) error { got <- obs; return nil },
	})
	fe.sendRaw(string(observeFixture(t, "observeradded-legacy.json")))
	obs := <-got
	if obs.Legacy == nil || obs.Legacy.TriggerType != "http" || len(obs.Args) != 0 {
		t.Fatalf("legacy %+v args %v", obs.Legacy, obs.Args)
	}
	var cfg struct {
		APIPath    string `json:"api_path"`
		HTTPMethod string `json:"http_method"`
	}
	if err := json.Unmarshal(obs.Legacy.Config, &cfg); err != nil || cfg.APIPath != "/users/:id" || cfg.HTTPMethod != "GET" {
		t.Errorf("config %s: %v", obs.Legacy.Config, err)
	}
}

// sdk.md §1: ArgsFormat with auto delivery fails at RegisterFunction
// (INVALID_OBSERVE_DESCRIPTOR); the descriptor travels on registerfunction.
func TestObserveDescriptorValidation(t *testing.T) {
	c := New("ws://unused.invalid")
	t.Cleanup(func() { _ = c.Close() })
	var rej *ObserveRejectionError
	_, err := c.RegisterFunction("auto::fn", echoHandler, RegisterFunctionOptions{Observe: &ObserveHooks{ArgsFormat: json.RawMessage(`{"type":"object"}`)}})
	if !errors.As(err, &rej) || rej.Code != "INVALID_OBSERVE_DESCRIPTOR" {
		t.Errorf("args_format on auto: %v", err)
	}
	_, err = c.RegisterFunction("bad::fn", echoHandler, RegisterFunctionOptions{Observe: &ObserveHooks{Delivery: "push"}})
	if !errors.As(err, &rej) || rej.Code != "INVALID_OBSERVE_DESCRIPTOR" {
		t.Errorf("unknown delivery: %v", err)
	}
	if c.functionObservers("auto::fn") != nil || c.functionObservers("bad::fn") != nil {
		t.Error("a refused registration was stored")
	}
	_, err = RegisterFunctionTyped(c, "typed::fn", func(context.Context, struct{}) (struct{}, error) { return struct{}{}, nil },
		RegisterFunctionOptions{Observe: &ObserveHooks{ArgsFormat: json.RawMessage(`{}`)}})
	if !errors.As(err, &rej) {
		t.Errorf("typed registration: %v", err)
	}

	fe := newFakeEngine(t)
	cc := fe.client()
	off := false
	if _, err := cc.RegisterFunction("owner::fn", echoHandler, RegisterFunctionOptions{
		Observe:       &ObserveHooks{Delivery: DeliveryOwner, ArgsFormat: json.RawMessage(`{"type":"object"}`)},
		LocalDispatch: &off,
	}); err != nil {
		t.Fatal(err)
	}
	reg := fe.waitFrames(MsgRegisterFunction, 1, func(d *DecodedMessage) bool { return d.RegisterFunction.ID == "owner::fn" })[0]
	if d := reg.RegisterFunction.Observe; d == nil || d.Delivery != DeliveryOwner || string(d.ArgsFormat) != `{"type":"object"}` {
		t.Errorf("descriptor %+v", d)
	}
	waitUntil(t, "confirmed", func() bool { return cc.functionObservers("owner::fn").syncedOn(cc.currentEpoch()) })
	if cc.LocalDispatchActive("owner::fn") {
		t.Error("LocalDispatch: false must keep the function remote")
	}
}

// protocol.md §1: against an engine without observe.v1, a function with hooks or owner
// delivery is a plain function, and one log line says its hooks are inactive.
func TestHooksInactiveAgainstOldEngineLogOnce(t *testing.T) {
	logs := captureLog(t)
	fe := newFakeEngine(t).oldEngine()
	c := New(fe.m.url)
	t.Cleanup(func() { _ = c.Close() })
	var log hookLog
	_, _ = c.RegisterFunction("before::fn", echoHandler, RegisterFunctionOptions{Observe: recordingHooks(&log, DeliveryOwner)})
	_, _ = c.RegisterFunction("plain::fn", echoHandler)
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	if err := c.Connect(ctx); err != nil {
		t.Fatal(err)
	}
	waitUntil(t, "registered", c.registeredNow)
	_, _ = c.RegisterFunction("after::fn", echoHandler, RegisterFunctionOptions{Observe: recordingHooks(&log, DeliveryAuto)})
	fe.reconnect(c)
	const inactive = "its observe hooks are inactive"
	if logs.count(inactive) != 2 || logs.count(`"before::fn"`) != 1 || logs.count(`"after::fn"`) != 1 || logs.count("plain::fn") != 0 {
		t.Errorf("logs:\n%s", logs.String())
	}
	if c.LocalDispatchActive("before::fn") {
		t.Error("local dispatch without functionregistered")
	}
}

// SDK-PARITY decisions 2, 11, 18: Reject leaves the compiled set at once, sends the
// revision the engine last announced, survives a reconnect (re-acked with the error on
// the replayed functionregistered) until a fresh observeradded.
func TestLateRejectionSurvivesReconnect(t *testing.T) {
	fe := newFakeEngine(t)
	fe.setObservers("late::fn", 3, observerInfo("o1", "x::y", "", ""))
	c := fe.client()
	var log hookLog
	registerOwner(t, fe, c, "late::fn", recordingHooks(&log, DeliveryOwner))
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
	if c.IsObserved("late::fn") && len(c.Observers("late::fn")) != 1 {
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
	added, removed := log.snapshot()
	if len(added) != 3 || added[2] != "o1" || len(removed) != 1 || removed[0] != "o1" {
		t.Errorf("hooks: added %v removed %v", added, removed)
	}
}

// SDK-PARITY decision 24: a Reject made while OnObserverAdded is still running wins:
// the add acks with it instead of activating, and the engine's observerremoved runs
// OnObserverRemoved to undo what the hook indexed.
func TestRejectDuringOnObserverAddedWins(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	var log hookLog
	registerOwner(t, fe, c, "rej::fn", &ObserveHooks{
		Delivery: DeliveryOwner,
		OnObserverAdded: func(_ context.Context, obs *Observer) error {
			log.add(obs.ID)
			obs.Reject("UNKNOWN_DB", "the db went away during setup")
			return nil
		},
		OnObserverRemoved: func(_ context.Context, obs *Observer) { log.remove(obs.ID) },
	})
	rev := fe.add("rej::fn", observerInfo("o1", "x::y", "", ""))
	a := fe.waitAck("rej::fn", "o1", rev)
	if a.Error == nil || a.Error.Code != "UNKNOWN_DB" {
		t.Fatalf("ack %+v, want the rejection", a.Error)
	}
	if c.IsObserved("rej::fn") {
		t.Error("an observer rejected during its hook was activated")
	}
	fe.remove("rej::fn", "o1")
	waitUntil(t, "OnObserverRemoved", func() bool { _, r := log.snapshot(); return len(r) == 1 })
}

// Value semantics: an Observer keeps its own copy of the engine's capture list, and
// an event's caller is a copy of the invocation's.
func TestObserverAndEventCallerDoNotShareTheirSources(t *testing.T) {
	f := newFunctionObservers(New("ws://unused.invalid"), "own::fn", &ObserveHooks{Delivery: DeliveryOwner}, true)
	info := observerInfo("o1", "x::y", "", "", CaptureInput, CaptureCaller)
	obs, err := newObserver(f, info, 1)
	if err != nil {
		t.Fatal(err)
	}
	info.Capture[0] = CaptureMetadata
	if obs.Capture[0] != CaptureInput {
		t.Error("the observer shares the decoded capture slice")
	}
	inv := &invocation{functionID: "own::fn", invocationID: "i", caller: &Caller{FunctionID: "up::fn", Parent: &Caller{WorkerName: "w"}}}
	caller, own := f.eventCaller(inv)
	if !own || caller == inv.caller || caller.Parent == inv.caller.Parent || caller.FunctionID != "up::fn" {
		t.Errorf("event caller %+v shares the invocation's", caller)
	}
}

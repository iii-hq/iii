package iii

import (
	"context"
	"encoding/json"
	"errors"
	"reflect"
	"slices"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/google/uuid"
)

// Observer ids (decision 56): a handle holds its id until it is disposed; an Observe of
// a held id is refused with nothing sent; once disposed the id is free at once, and
// the engine's answers reach the handle they answer, by the order of the frames.

var idSource = Source{FunctionID: "orders::create"}

func unobserveFrames(fe *fakeEngine, id string) int {
	n := 0
	for _, d := range fe.frames(MsgUnobserve) {
		if d.Unobserve.ID == id {
			n++
		}
	}
	return n
}

// An Observe of an id a handle holds, in any state, returns an *ObserveRejectionError
// with OBSERVER_ID_CONFLICT, sends nothing and leaves the holder alone.
func TestObserveRefusesAHeldID(t *testing.T) {
	for _, held := range []string{"pending", "active", "error"} {
		t.Run(held, func(t *testing.T) {
			fe := newFakeEngine(t)
			c := fe.client()
			first, err := c.Observe(idSource, "audit::log", ObserverOptions{ID: "held"})
			if err != nil {
				t.Fatal(err)
			}
			fe.waitFrames(MsgObserve, 1, nil)
			switch held {
			case "active":
				fe.send(&ObserveResultMessage{ID: "held", State: "active"})
			case "error":
				fe.send(&ObserveResultMessage{ID: "held", State: "error", Error: &ErrorBody{Code: "FORBIDDEN", Message: "no"}})
			}
			waitUntil(t, "the holder's state", func() bool { return string(first.State()) == held })

			second, err := c.Observe(idSource, "audit::other", ObserverOptions{ID: "held"})
			var rej *ObserveRejectionError
			if second != nil || !errors.As(err, &rej) || rej.Code != "OBSERVER_ID_CONFLICT" || rej.Message != "observer id already in use: held" {
				t.Fatalf("Observe of a held id: %v, %v", second, err)
			}
			if got := err.Error(); got != "iii: observe rejected: OBSERVER_ID_CONFLICT: observer id already in use: held" {
				t.Errorf("Error() %q", got)
			}
			time.Sleep(20 * time.Millisecond)
			if n := len(observeFrames(fe, "held")); n != 1 {
				t.Errorf("%d observe frames, want 1 (the refusal sends nothing)", n)
			}
			if string(first.State()) != held {
				t.Errorf("holder state %s, want %s", first.State(), held)
			}
			// the holder still holds it: its Dispose unobserves, except a refused one's,
			// which the engine does not hold
			_ = first.Dispose(context.Background())
			if held == "error" {
				time.Sleep(20 * time.Millisecond)
				if n := unobserveFrames(fe, "held"); n != 0 {
					t.Errorf("the refused holder's Dispose sent %d unobserves", n)
				}
				return
			}
			fe.waitFrames(MsgUnobserve, 1, func(d *DecodedMessage) bool { return d.Unobserve.ID == "held" })
		})
	}
	// before the connection is up too
	c := New("ws://unused.invalid")
	t.Cleanup(func() { _ = c.Close() })
	if _, err := c.Observe(idSource, "audit::log", ObserverOptions{ID: "offline"}); err != nil {
		t.Fatal(err)
	}
	var rej *ObserveRejectionError
	if _, err := c.Observe(idSource, "audit::log", ObserverOptions{ID: "offline"}); !errors.As(err, &rej) || rej.Code != "OBSERVER_ID_CONFLICT" {
		t.Errorf("offline: %v", err)
	}
}

// Disposed, the id is free at once. The engine's disposed for the earlier observer,
// sent before it read the new observe, reaches the earlier handle: the new one turns
// active on its own observeresult, hears the state changes after it, and its Dispose
// unobserves (the engine-side observer does not leak).
func TestReusedIDHearsOnlyItsOwnAnswers(t *testing.T) {
	for _, earlierAnswered := range []bool{true, false} {
		name := "earlier answered"
		if !earlierAnswered {
			name = "earlier unanswered"
		}
		t.Run(name, func(t *testing.T) {
			const id = "queue-consumer"
			fe := newFakeEngine(t)
			c := fe.client()
			first, _ := c.Observe(idSource, "audit::log", ObserverOptions{ID: id})
			fe.waitFrames(MsgObserve, 1, nil)
			if earlierAnswered {
				fe.send(&ObserveResultMessage{ID: id, State: "active"})
				waitUntil(t, "first active", func() bool { return first.State() == ObserverActive })
			}
			if err := first.Dispose(context.Background()); err != nil {
				t.Fatal(err)
			}
			second, err := c.Observe(idSource, "audit::log", ObserverOptions{ID: id})
			if err != nil {
				t.Fatalf("Observe right after Dispose: %v", err)
			}
			fe.waitFrames(MsgObserve, 2, nil)
			if unobserveFrames(fe, id) != 1 {
				t.Fatal("the unobserve went out after the new observe")
			}

			// the engine answers in the order it read observe, unobserve, observe
			if !earlierAnswered {
				fe.send(&ObserveResultMessage{ID: id, State: "active"})
			}
			fe.send(&ObserverStateChangedMessage{ID: id, State: "disposed", Reason: &ErrorBody{Code: "UNOBSERVED", Message: "unobserved"}})
			fe.send(&ObserveResultMessage{ID: id, State: "active"})
			if err := readyErr(t, second); err != nil {
				t.Fatalf("new handle: %v (state %s)", err, second.State())
			}
			fe.send(&ObserverStateChangedMessage{ID: id, State: "suspended", Reason: &ErrorBody{Code: "TARGET_UNAVAILABLE", Message: "gone"}})
			waitUntil(t, "the new handle suspended", func() bool { return second.State() == ObserverSuspended })
			if first.State() != ObserverDisposed || first.Reason().Code != "UNOBSERVED" {
				t.Errorf("earlier handle %s %+v", first.State(), first.Reason())
			}
			if err := second.Dispose(context.Background()); err != nil {
				t.Fatal(err)
			}
			fe.waitFrames(MsgUnobserve, 2, func(d *DecodedMessage) bool { return d.Unobserve.ID == id })
			c.mu.Lock()
			_, held := c.observers[id]
			c.mu.Unlock()
			if held {
				t.Error("the id is still held after both handles were disposed")
			}
		})
	}
}

// An observerstatechanged that arrives before any observeresult of its id is dropped:
// the observeresult carries the current state.
func TestStateChangeBeforeTheAnswerIsDropped(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	h, _ := c.Observe(idSource, "audit::log", ObserverOptions{ID: "early"})
	fe.waitFrames(MsgObserve, 1, nil)
	fe.send(&ObserverStateChangedMessage{ID: "early", State: "disposed", Reason: &ErrorBody{Code: "UNOBSERVED", Message: "an earlier observer's"}})
	fe.send(&ObserveResultMessage{ID: "early", State: "active"})
	if err := readyErr(t, h); err != nil {
		t.Fatalf("Ready: %v (state %s)", err, h.State())
	}
	// an answer for an id nothing was sent for is dropped as well
	fe.send(&ObserveResultMessage{ID: "unknown", State: "active"})
	fe.send(&ObserverStateChangedMessage{ID: "unknown", State: "disposed"})
	fe.send(&ObserverStateChangedMessage{ID: "early", State: "suspended", Reason: &ErrorBody{Code: "TARGET_UNAVAILABLE", Message: "gone"}})
	waitUntil(t, "suspended", func() bool { return h.State() == ObserverSuspended })
}

// The engine's disposal (max_fires, expiry, engine::observers::dispose) frees the id.
func TestEngineDisposalFreesTheID(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	h, _ := c.Observe(idSource, "audit::log", ObserverOptions{ID: "once", MaxFires: 1})
	fe.waitFrames(MsgObserve, 1, nil)
	fe.send(&ObserveResultMessage{ID: "once", State: "active"})
	fe.send(&ObserverStateChangedMessage{ID: "once", State: "disposed", Reason: &ErrorBody{Code: "MAX_FIRES", Message: "fired once"}, Fires: 1})
	waitUntil(t, "disposed", func() bool { return h.State() == ObserverDisposed })
	again, err := c.Observe(idSource, "audit::log", ObserverOptions{ID: "once"})
	if err != nil {
		t.Fatalf("Observe after the engine disposed the holder: %v", err)
	}
	fe.waitFrames(MsgObserve, 2, nil)
	fe.send(&ObserveResultMessage{ID: "once", State: "active"})
	if err := readyErr(t, again); err != nil {
		t.Fatal(err)
	}
}

// An observer disposed while disconnected, its id observed again meanwhile: the next
// connection gets the unobserve before the new observe, and the engine's answers to
// the earlier observer (resumed by a reattach, then disposed) reach no handle.
func TestUnobserveQueuedWhileAwayGoesBeforeTheReplay(t *testing.T) {
	const id = "stable"
	fe := newFakeEngine(t)
	c := fe.client()
	first, _ := c.Observe(idSource, "audit::log", ObserverOptions{ID: id})
	fe.waitFrames(MsgObserve, 1, nil)
	fe.send(&ObserveResultMessage{ID: id, State: "active"})
	waitUntil(t, "first active", func() bool { return first.State() == ObserverActive })

	gate := make(chan struct{})
	fe.mu.Lock()
	fe.gate = gate
	connects := fe.connects
	fe.mu.Unlock()
	mark := fe.mark()
	fe.m.closeActiveConnection()
	waitUntil(t, "reconnect", func() bool { fe.mu.Lock(); defer fe.mu.Unlock(); return fe.connects > connects })
	_ = first.Dispose(context.Background())
	second, err := c.Observe(idSource, "audit::log", ObserverOptions{ID: id})
	if err != nil {
		t.Fatal(err)
	}
	close(gate)
	waitUntil(t, "registered", c.registeredNow)
	fe.waitFrames(MsgObserve, 2, nil)

	unobserveAt, observeAt := -1, -1
	for i, m := range fe.m.receivedMessages()[mark:] {
		if messageID(m) != id {
			continue
		}
		switch messageType(m) {
		case string(MsgUnobserve):
			if unobserveAt < 0 {
				unobserveAt = i
			}
		case string(MsgObserve):
			if observeAt < 0 {
				observeAt = i
			}
		}
	}
	if unobserveAt < 0 || observeAt < unobserveAt {
		t.Fatalf("unobserve at %d, observe at %d: the queued unobserve goes first", unobserveAt, observeAt)
	}
	fe.send(&ObserverStateChangedMessage{ID: id, State: "active"})
	fe.send(&ObserverStateChangedMessage{ID: id, State: "disposed", Reason: &ErrorBody{Code: "UNOBSERVED", Message: "unobserved"}})
	fe.send(&ObserveResultMessage{ID: id, State: "active"})
	if err := readyErr(t, second); err != nil {
		t.Fatalf("new handle: %v (state %s)", err, second.State())
	}
}

// An answer too large to send is payload_too_large, which activates no observer: to an
// engine with observe.v2 it carries delivered when the deliver hook delivered the call
// and matched [] otherwise, whatever the hooks; an engine without observe.v2 gets the
// bare error.
func TestTooLargeAnswerActivatesNoObserver(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	epoch := c.currentEpoch()
	id := uuid.New()
	cases := []struct {
		name      string
		answer    InvocationResultMessage
		epoch     uint64
		matched   bool
		delivered bool
	}{
		{"no hook", InvocationResultMessage{}, epoch, true, false},
		{"a match hook", InvocationResultMessage{Matched: []string{"o1", "o2"}}, epoch, true, false},
		{"a deliver hook", InvocationResultMessage{Delivered: true}, epoch, false, true},
		{"a connection that is gone", InvocationResultMessage{Delivered: true}, epoch + 1, false, false},
	}
	for _, tc := range cases {
		tc.answer.InvocationID, tc.answer.FunctionID = id, "big::fn"
		tc.answer.Result = json.RawMessage(`"x"`)
		frame, err := c.tooLargeAnswer(tc.epoch, &tc.answer, maxJSONMessageBytes+1)
		if err != nil {
			t.Fatal(err)
		}
		var m map[string]json.RawMessage
		_ = json.Unmarshal(frame, &m)
		var body ErrorBody
		_ = json.Unmarshal(m["error"], &body)
		if body.Code != "payload_too_large" || body.Message != payloadTooLargeMessage(maxJSONMessageBytes+1) || m["result"] != nil || stringField(m, "invocation_id") != id.String() {
			t.Errorf("%s: %s", tc.name, frame)
		}
		if got := string(m["matched"]); (got == "[]") != tc.matched || (!tc.matched && got != "") {
			t.Errorf("%s: matched %q", tc.name, got)
		}
		if got := string(m["delivered"]); (got == "true") != tc.delivered || (!tc.delivered && got != "") {
			t.Errorf("%s: delivered %q", tc.name, got)
		}
	}
	old := newFakeEngine(t).v1Engine()
	oc := old.client()
	frame, _ := oc.tooLargeAnswer(oc.currentEpoch(), &InvocationResultMessage{InvocationID: id, FunctionID: "big::fn", Delivered: true}, maxJSONMessageBytes+1)
	var m map[string]json.RawMessage
	_ = json.Unmarshal(frame, &m)
	if m["matched"] != nil || m["delivered"] != nil || m["error"] == nil {
		t.Errorf("engine without observe.v2: %s, want the bare error", frame)
	}

	// end to end: a match hook that matched the observer, and an answer too large
	huge := json.RawMessage(`"` + strings.Repeat("x", maxJSONMessageBytes) + `"`)
	c.OnObservationMatchFor("big::matched", MatchFunc(func(*Observer, *Call) bool { return true }))
	registerHost(t, c, "big::matched", func(context.Context, json.RawMessage) (any, error) { return huge, nil })
	addObserver(t, fe, "big::matched", observerInfo("o-big", "x::y", "", ""))
	waitUntil(t, "the match hook installed", func() bool { return c.functionObservers("big::matched").state.Load().match != nil })
	call := fe.invoke(InvokeFunctionMessage{FunctionID: "big::matched"})
	// encoding a 64 MiB answer takes seconds under -race
	deadline := time.Now().Add(30 * time.Second)
	var res *InvocationResultMessage
	for res == nil && time.Now().Before(deadline) {
		for _, d := range fe.frames(MsgInvocationResult) {
			if d.InvocationResult.InvocationID == call {
				res = d.InvocationResult
			}
		}
		time.Sleep(5 * time.Millisecond)
	}
	if res == nil || res.Error == nil || res.Error.Code != "payload_too_large" || res.Matched == nil || len(res.Matched) != 0 || res.Delivered {
		t.Fatalf("answer %+v", res)
	}
}

// The engine disposes an observer whose worker stayed away past the grace period, and
// answers its replay on the next connection with disposed, OBSERVER_DISCONNECTED. That
// connection heard the disposal, so it may observe the id again as a new observer
// (acceptance E17).
var disconnected = &ErrorBody{Code: "OBSERVER_DISCONNECTED", Message: "the observing worker disconnected and the grace period ended"}

// An observer without counters is observed again at once, with the same id and frame:
// it never reports disposed, goes pending, follows the new answer, keeps its id and is
// replayed on later connections, where an observerstatechanged disposal is handled the
// same way.
func TestDisconnectedDisposalObservesAgain(t *testing.T) {
	const id = "long-lived"
	logs := captureLog(t)
	fe := newFakeEngine(t)
	c := fe.client()
	h, _ := c.Observe(idSource, "audit::log", ObserverOptions{ID: id, Metadata: json.RawMessage(`{"team":"ops"}`)})
	fe.waitFrames(MsgObserve, 1, nil)
	fe.send(&ObserveResultMessage{ID: id, State: "active"})
	waitUntil(t, "active", func() bool { return h.State() == ObserverActive })
	var mu sync.Mutex
	var seen []ObserverState
	h.OnStateChange(func(s ObserverState, _ *ObserveReason) { mu.Lock(); seen = append(seen, s); mu.Unlock() })

	fe.reconnect(c)
	fe.waitFrames(MsgObserve, 2, nil)
	fe.send(&ObserveResultMessage{ID: id, State: "disposed", Error: disconnected})
	fe.waitFrames(MsgObserve, 3, nil)
	if frames := observeFrames(fe, id); !reflect.DeepEqual(frames[2], frames[0]) {
		t.Errorf("observed again with %+v, want the first observe %+v", frames[2], frames[0])
	}
	waitUntil(t, "pending", func() bool { return h.State() == ObserverPending })
	if r := h.Reason(); r != nil {
		t.Errorf("pending with reason %+v", r)
	}
	fe.send(&ObserveResultMessage{ID: id, State: "active"})
	if err := readyErr(t, h); err != nil {
		t.Fatalf("Ready: %v (state %s)", err, h.State())
	}
	waitUntil(t, "two state changes", func() bool { mu.Lock(); defer mu.Unlock(); return len(seen) == 2 })
	mu.Lock()
	if !slices.Equal(seen, []ObserverState{ObserverPending, ObserverActive}) {
		t.Errorf("listener saw %v, want pending then active (never disposed)", seen)
	}
	mu.Unlock()
	if n := logs.count("observer " + id + " was disposed by the engine while this worker was away past the grace period; observing it again"); n != 1 {
		t.Errorf("%d log lines, want 1: %q", n, logs.String())
	}
	var rej *ObserveRejectionError
	if _, err := c.Observe(idSource, "audit::log", ObserverOptions{ID: id}); !errors.As(err, &rej) || rej.Code != "OBSERVER_ID_CONFLICT" {
		t.Errorf("the handle no longer holds its id: %v", err)
	}

	fe.reconnect(c)
	fe.waitFrames(MsgObserve, 4, nil)
	fe.send(&ObserveResultMessage{ID: id, State: "active"})
	fe.send(&ObserverStateChangedMessage{ID: id, State: "disposed", Reason: disconnected})
	fe.waitFrames(MsgObserve, 5, nil)
	fe.send(&ObserveResultMessage{ID: id, State: "active"})
	waitUntil(t, "active again", func() bool { return h.State() == ObserverActive })
}

// An observer with counters (max_fires, expires_in) is disposed and forgotten as
// before: a new observer would reset them (decision 56). Its id is free.
func TestDisconnectedDisposalForgetsObserversWithCounters(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	opts := []ObserverOptions{{ID: "fires", MaxFires: 5}, {ID: "expires", ExpiresIn: time.Hour}, {ID: "plain"}}
	handles := map[string]*ObserverHandle{}
	for _, o := range opts {
		handles[o.ID], _ = c.Observe(idSource, "audit::log", o)
	}
	fe.waitFrames(MsgObserve, 3, nil)
	fe.reconnect(c)
	fe.waitFrames(MsgObserve, 6, nil)
	for _, o := range opts {
		fe.send(&ObserveResultMessage{ID: o.ID, State: "disposed", Error: disconnected})
	}
	// plain, answered last, is observed again: the earlier answers were applied
	fe.waitFrames(MsgObserve, 7, nil)
	for _, id := range []string{"fires", "expires"} {
		h := handles[id]
		if h.State() != ObserverDisposed || h.Reason() == nil || h.Reason().Code != "OBSERVER_DISCONNECTED" {
			t.Errorf("%s: %s %+v, want disposed OBSERVER_DISCONNECTED", id, h.State(), h.Reason())
		}
		if n := len(observeFrames(fe, id)); n != 2 {
			t.Errorf("%s sent %d times, want 2", id, n)
		}
		if _, err := c.Observe(idSource, "audit::log", ObserverOptions{ID: id}); err != nil {
			t.Errorf("%s: the id is still held: %v", id, err)
		}
	}
	if s := handles["plain"].State(); s != ObserverPending {
		t.Errorf("plain: %s, want pending", s)
	}
}

// A handle its Dispose released is not observed again: the disposal stands.
func TestDisconnectedDisposalOfADisposedHandle(t *testing.T) {
	logs := captureLog(t)
	fe := newFakeEngine(t)
	c := fe.client()
	gone, _ := c.Observe(idSource, "audit::log", ObserverOptions{ID: "gone"})
	kept, _ := c.Observe(idSource, "audit::log", ObserverOptions{ID: "kept"})
	fe.waitFrames(MsgObserve, 2, nil)
	fe.reconnect(c)
	fe.waitFrames(MsgObserve, 4, nil)
	_ = gone.Dispose(context.Background())
	fe.waitFrames(MsgUnobserve, 1, nil)
	fe.send(&ObserveResultMessage{ID: "gone", State: "disposed", Error: disconnected})
	fe.send(&ObserveResultMessage{ID: "kept", State: "disposed", Error: disconnected})
	fe.waitFrames(MsgObserve, 5, nil) // kept is observed again
	if n := len(observeFrames(fe, "gone")); n != 2 {
		t.Errorf("disposed handle sent %d times, want 2", n)
	}
	if gone.State() != ObserverDisposed || gone.Reason().Code != "UNOBSERVED" {
		t.Errorf("disposed handle %s %+v", gone.State(), gone.Reason())
	}
	if n := logs.count("observer gone was disposed"); n != 0 {
		t.Errorf("the disposed handle was observed again: %q", logs.String())
	}
	if kept.State() != ObserverPending {
		t.Errorf("kept: %s, want pending", kept.State())
	}
}

// Loop guard: when the observe sent again is disposed with OBSERVER_DISCONNECTED on the
// same connection too, the handle is disposed and forgotten.
func TestDisconnectedDisposalObservesAgainOncePerConnection(t *testing.T) {
	const id = "twice"
	fe := newFakeEngine(t)
	c := fe.client()
	h, _ := c.Observe(idSource, "audit::log", ObserverOptions{ID: id})
	fe.waitFrames(MsgObserve, 1, nil)
	fe.reconnect(c)
	fe.waitFrames(MsgObserve, 2, nil)
	fe.send(&ObserveResultMessage{ID: id, State: "disposed", Error: disconnected})
	fe.waitFrames(MsgObserve, 3, nil)
	fe.send(&ObserveResultMessage{ID: id, State: "disposed", Error: disconnected})
	waitUntil(t, "disposed", func() bool { return h.State() == ObserverDisposed })
	if h.Reason().Code != "OBSERVER_DISCONNECTED" {
		t.Errorf("reason %+v", h.Reason())
	}
	fe.reconnect(c)
	time.Sleep(30 * time.Millisecond)
	if n := len(observeFrames(fe, id)); n != 3 {
		t.Errorf("sent %d times, want 3 (forgotten after the second disposal)", n)
	}
}

// A handle in error (here an observerstatechanged, which leaves it replayed) is not
// observed again: the disposal of its replay stands and frees its id.
func TestDisconnectedDisposalOfAHandleInError(t *testing.T) {
	const id = "failing"
	fe := newFakeEngine(t)
	c := fe.client()
	h, _ := c.Observe(idSource, "audit::log", ObserverOptions{ID: id})
	fe.waitFrames(MsgObserve, 1, nil)
	fe.send(&ObserveResultMessage{ID: id, State: "active"})
	fe.send(&ObserverStateChangedMessage{ID: id, State: "error", Reason: &ErrorBody{Code: "OWNER_TIMEOUT", Message: "the owner did not answer"}})
	waitUntil(t, "error", func() bool { return h.State() == ObserverError })
	fe.reconnect(c)
	fe.waitFrames(MsgObserve, 2, nil)
	fe.send(&ObserveResultMessage{ID: id, State: "disposed", Error: disconnected})
	waitUntil(t, "disposed", func() bool { return h.State() == ObserverDisposed })
	if h.Reason().Code != "OBSERVER_DISCONNECTED" {
		t.Errorf("reason %+v", h.Reason())
	}
	time.Sleep(30 * time.Millisecond)
	if n := len(observeFrames(fe, id)); n != 2 {
		t.Errorf("sent %d times, want 2 (not observed again)", n)
	}
	if _, err := c.Observe(idSource, "audit::log", ObserverOptions{ID: id}); err != nil {
		t.Errorf("the id is still held: %v", err)
	}
}

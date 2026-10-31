package iii

import (
	"context"
	"errors"
	"testing"
	"time"
)

// Refused observes: an observeresult in state error, for a handle whose observer the
// engine never created, refuses the observe, and the engine stores nothing for it. The
// refusal is the handle's final answer: the handle leaves its id's list when it
// arrives and is not sent again, its Dispose sends nothing and returns at once, and a
// later observe of the id works as any other.

// quickDispose bounds a Dispose that has nothing to wait for, far under the 5 s an
// SDK that waits for the engine's disposed gives it.
const quickDispose = 250 * time.Millisecond

// listed is the answer list of id: the handles whose observe went out on the current
// connection and are still listed.
func listed(c *Client, id string) []*ObserverHandle {
	c.mu.Lock()
	defer c.mu.Unlock()
	var out []*ObserverHandle
	for _, s := range c.observeSent[id] {
		out = append(out, s.handle)
	}
	return out
}

func refusal(id, code string) *ObserveResultMessage {
	return &ObserveResultMessage{ID: id, State: "error", Error: &ErrorBody{Code: code, Message: "refused by the engine"}}
}

func disposeQuickly(t *testing.T, h *ObserverHandle) {
	t.Helper()
	start := time.Now()
	if err := h.Dispose(context.Background()); err != nil {
		t.Fatalf("Dispose: %v", err)
	}
	if d := time.Since(start); d > quickDispose {
		t.Errorf("Dispose took %s", d)
	}
	if h.State() != ObserverDisposed {
		t.Errorf("state %s after Dispose", h.State())
	}
}

// Every engine refusal (RBAC, invalid args or filters, the engine's id conflict, the
// per-connection limit, the registration hook) unlists the handle: nothing the engine
// sends for the id afterwards reaches it, it keeps its id until it is disposed, and
// its Dispose sends no unobserve.
func TestRefusedObserveIsFinal(t *testing.T) {
	for _, code := range []string{"FORBIDDEN", "INVALID_ARGS", "INVALID_FILTER", "OBSERVER_ID_CONFLICT", "LIMIT_EXCEEDED", "OBSERVE_REJECTED"} {
		t.Run(code, func(t *testing.T) {
			const id = "refused"
			fe := newFakeEngine(t)
			c := fe.client()
			h, err := c.Observe(idSource, "audit::log", ObserverOptions{ID: id})
			if err != nil {
				t.Fatal(err)
			}
			fe.waitFrames(MsgObserve, 1, nil)
			fe.send(refusal(id, code))
			var rej *ObserveRejectionError
			if err := readyErr(t, h); !errors.As(err, &rej) || rej.Code != code {
				t.Fatalf("Ready: %v", err)
			}
			if l := listed(c, id); len(l) != 0 {
				t.Errorf("%d handles listed after the refusal", len(l))
			}
			fe.send(&ObserverStateChangedMessage{ID: id, State: "active"})
			fe.send(&ObserveResultMessage{ID: id, State: "active"})
			time.Sleep(30 * time.Millisecond)
			if h.State() != ObserverError || h.Reason().Code != code {
				t.Errorf("refused handle %s %+v after later answers for its id", h.State(), h.Reason())
			}
			if again, err := c.Observe(idSource, "audit::log", ObserverOptions{ID: id}); again != nil || !errors.As(err, &rej) || rej.Code != "OBSERVER_ID_CONFLICT" {
				t.Errorf("Observe of the refused handle's id: %v, %v", again, err)
			}

			disposeQuickly(t, h)
			time.Sleep(30 * time.Millisecond)
			if n := unobserveFrames(fe, id); n != 0 {
				t.Errorf("Dispose of a refused observer sent %d unobserves", n)
			}
			c.mu.Lock()
			_, held := c.observers[id]
			c.mu.Unlock()
			if held {
				t.Error("the id is still held after Dispose")
			}
		})
	}
}

// A Dispose sent before the answer returns at once with its unobserve on the wire; the
// refusal then unlists the handle, the engine ignores the unobserve (it holds
// nothing), and an observe of the id afterwards hears its own answers.
func TestDisposeBeforeTheRefusal(t *testing.T) {
	const id = "early-dispose"
	fe := newFakeEngine(t)
	c := fe.client()
	first, _ := c.Observe(idSource, "audit::log", ObserverOptions{ID: id})
	fe.waitFrames(MsgObserve, 1, nil)
	disposeQuickly(t, first)
	fe.waitFrames(MsgUnobserve, 1, func(d *DecodedMessage) bool { return d.Unobserve.ID == id })
	if l := listed(c, id); len(l) != 1 || l[0] != first {
		t.Fatalf("listed %v before the answer, want the disposed handle", l)
	}

	fe.send(refusal(id, "FORBIDDEN"))
	waitUntil(t, "the refusal unlists the handle", func() bool { return len(listed(c, id)) == 0 })
	if first.State() != ObserverDisposed || first.Reason().Code != "UNOBSERVED" {
		t.Errorf("disposed handle %s %+v after its refusal", first.State(), first.Reason())
	}

	second, err := c.Observe(idSource, "audit::log", ObserverOptions{ID: id})
	if err != nil {
		t.Fatal(err)
	}
	fe.waitFrames(MsgObserve, 2, nil)
	fe.send(&ObserveResultMessage{ID: id, State: "active"})
	if err := readyErr(t, second); err != nil {
		t.Fatalf("new handle: %v (state %s)", err, second.State())
	}
	fe.send(&ObserverStateChangedMessage{ID: id, State: "suspended", Reason: &ErrorBody{Code: "TARGET_UNAVAILABLE", Message: "gone"}})
	waitUntil(t, "the new handle suspended", func() bool { return second.State() == ObserverSuspended })
	disposeQuickly(t, second)
	fe.waitFrames(MsgUnobserve, 2, func(d *DecodedMessage) bool { return d.Unobserve.ID == id })
}

// Refused, then disposed (nothing sent): the id observed again goes out as a new
// observe, whose answers and state changes reach the new handle, and whose Dispose
// unobserves.
func TestRefusalThenANewObserveOfTheID(t *testing.T) {
	const id = "retry"
	fe := newFakeEngine(t)
	c := fe.client()
	first, _ := c.Observe(idSource, "audit::log", ObserverOptions{ID: id})
	fe.waitFrames(MsgObserve, 1, nil)
	fe.send(refusal(id, "INVALID_ARGS"))
	waitUntil(t, "refused", func() bool { return first.State() == ObserverError })
	disposeQuickly(t, first)

	second, err := c.Observe(idSource, "audit::other", ObserverOptions{ID: id})
	if err != nil {
		t.Fatalf("Observe after the refused handle was disposed: %v", err)
	}
	fe.waitFrames(MsgObserve, 2, nil)
	if l := listed(c, id); len(l) != 1 || l[0] != second {
		t.Fatalf("listed %v, want the new handle alone", l)
	}
	fe.send(&ObserveResultMessage{ID: id, State: "active"})
	if err := readyErr(t, second); err != nil {
		t.Fatalf("new handle: %v (state %s)", err, second.State())
	}
	fe.send(&ObserverStateChangedMessage{ID: id, State: "suspended", Reason: &ErrorBody{Code: "TARGET_UNAVAILABLE", Message: "gone"}})
	waitUntil(t, "the new handle suspended", func() bool { return second.State() == ObserverSuspended })
	if first.State() != ObserverDisposed {
		t.Errorf("refused handle %s", first.State())
	}
	disposeQuickly(t, second)
	fe.waitFrames(MsgUnobserve, 1, func(d *DecodedMessage) bool { return d.Unobserve.ID == id })
	time.Sleep(20 * time.Millisecond)
	if n := unobserveFrames(fe, id); n != 1 {
		t.Errorf("%d unobserves, want the new handle's only", n)
	}
}

// The older handle of an id (disposed before its answer) is refused while a newer
// one's observe is on the wire: the refusal answers the older handle (rule d), the
// newer stays pending and listed, and its own answer decides it.
func TestRefusalOfAnOlderHandleWhileANewerIsPending(t *testing.T) {
	for _, newer := range []string{"accepted", "refused"} {
		t.Run("newer "+newer, func(t *testing.T) {
			const id = "reused"
			fe := newFakeEngine(t)
			c := fe.client()
			first, _ := c.Observe(idSource, "audit::log", ObserverOptions{ID: id})
			fe.waitFrames(MsgObserve, 1, nil)
			disposeQuickly(t, first)
			second, err := c.Observe(idSource, "audit::log", ObserverOptions{ID: id})
			if err != nil {
				t.Fatal(err)
			}
			fe.waitFrames(MsgObserve, 2, nil)
			if unobserveFrames(fe, id) != 1 {
				t.Fatal("the unobserve went out after the new observe")
			}

			fe.send(refusal(id, "FORBIDDEN"))
			waitUntil(t, "the older handle unlisted", func() bool {
				l := listed(c, id)
				return len(l) == 1 && l[0] == second
			})
			// a state change before the newer handle's answer is dropped
			fe.send(&ObserverStateChangedMessage{ID: id, State: "active"})
			time.Sleep(30 * time.Millisecond)
			if second.State() != ObserverPending {
				t.Fatalf("newer handle %s %+v before its own answer", second.State(), second.Reason())
			}
			if first.State() != ObserverDisposed || first.Reason().Code != "UNOBSERVED" {
				t.Errorf("older handle %s %+v", first.State(), first.Reason())
			}

			if newer == "accepted" {
				fe.send(&ObserveResultMessage{ID: id, State: "active"})
				if err := readyErr(t, second); err != nil {
					t.Fatalf("newer handle: %v (state %s)", err, second.State())
				}
				disposeQuickly(t, second)
				fe.waitFrames(MsgUnobserve, 2, func(d *DecodedMessage) bool { return d.Unobserve.ID == id })
				return
			}
			fe.send(refusal(id, "LIMIT_EXCEEDED"))
			var rej *ObserveRejectionError
			if err := readyErr(t, second); !errors.As(err, &rej) || rej.Code != "LIMIT_EXCEEDED" {
				t.Fatalf("newer handle: %v", err)
			}
			if l := listed(c, id); len(l) != 0 {
				t.Errorf("%d handles listed after both refusals", len(l))
			}
			disposeQuickly(t, second)
			time.Sleep(30 * time.Millisecond)
			if n := unobserveFrames(fe, id); n != 1 {
				t.Errorf("%d unobserves, want the older handle's only", n)
			}
		})
	}
}

// A refusal is the handle's final answer: a later connection does not send the
// refused handle again. It keeps its state and its id, and its Dispose sends nothing;
// disposed while away, it parks nothing for the next connection.
func TestRefusedHandleIsNotSentAgain(t *testing.T) {
	for _, next := range []string{"kept", "disposed while away"} {
		t.Run(next, func(t *testing.T) {
			const id = "refused-once"
			fe := newFakeEngine(t)
			c := fe.client()
			h, _ := c.Observe(idSource, "audit::log", ObserverOptions{ID: id})
			fe.waitFrames(MsgObserve, 1, nil)
			fe.send(refusal(id, "FORBIDDEN"))
			waitUntil(t, "refused", func() bool { return h.State() == ObserverError })

			if next == "disposed while away" {
				gate := make(chan struct{})
				fe.mu.Lock()
				fe.gate = gate
				connects := fe.connects
				fe.mu.Unlock()
				fe.m.closeActiveConnection()
				waitUntil(t, "reconnect", func() bool { fe.mu.Lock(); defer fe.mu.Unlock(); return fe.connects > connects })
				disposeQuickly(t, h)
				c.mu.Lock()
				parked := len(c.pendingUnobserve)
				c.mu.Unlock()
				if parked != 0 {
					t.Errorf("%d unobserves parked for a refused observer", parked)
				}
				close(gate)
				waitUntil(t, "registered", c.registeredNow)
				time.Sleep(30 * time.Millisecond)
				if n, o := unobserveFrames(fe, id), len(observeFrames(fe, id)); n != 0 || o != 1 {
					t.Errorf("%d unobserves and %d observes, want none and the first", n, o)
				}
				return
			}

			fe.reconnect(c)
			fe.reconnect(c)
			time.Sleep(30 * time.Millisecond)
			if o := len(observeFrames(fe, id)); o != 1 {
				t.Fatalf("%d observes for a refused handle, want the first only", o)
			}
			if l := listed(c, id); len(l) != 0 {
				t.Errorf("listed %v on a later connection", l)
			}
			if h.State() != ObserverError || h.Reason().Code != "FORBIDDEN" {
				t.Errorf("refused handle %s %+v on a later connection", h.State(), h.Reason())
			}
			var rej *ObserveRejectionError
			if again, err := c.Observe(idSource, "audit::log", ObserverOptions{ID: id}); again != nil || !errors.As(err, &rej) || rej.Code != "OBSERVER_ID_CONFLICT" {
				t.Errorf("Observe of the refused handle's id: %v, %v", again, err)
			}
			disposeQuickly(t, h)
			time.Sleep(30 * time.Millisecond)
			if n := unobserveFrames(fe, id); n != 0 {
				t.Errorf("Dispose of a refused handle sent %d unobserves", n)
			}
			second, err := c.Observe(idSource, "audit::log", ObserverOptions{ID: id})
			if err != nil {
				t.Fatalf("Observe after the refused handle was disposed: %v", err)
			}
			fe.waitFrames(MsgObserve, 2, func(d *DecodedMessage) bool { return d.Observe.ID == id })
			if l := listed(c, id); len(l) != 1 || l[0] != second {
				t.Errorf("listed %v, want the new handle alone", l)
			}
		})
	}
}

// An observeresult in state error for a handle whose observer the engine may hold is
// that observer's state, not a refusal: on a reattached connection the engine answers
// the replay of an observer it holds in error (a host rejection, OWNER_TIMEOUT) with
// error. The handle was answered pending, active or suspended before, or its answer
// was lost to the closed connection. It stays listed, is replayed again on the next
// connection, and its Dispose unobserves it.
func TestAnErrorAnswerForAHeldObserver(t *testing.T) {
	for _, before := range []string{"pending", "active", "suspended", "answer lost"} {
		t.Run(before, func(t *testing.T) {
			const id = "held-errored"
			fe := newFakeEngine(t)
			c := fe.client()
			h, _ := c.Observe(idSource, "audit::log", ObserverOptions{ID: id})
			fe.waitFrames(MsgObserve, 1, nil)
			if before != "answer lost" {
				fe.send(&ObserveResultMessage{ID: id, State: before})
				waitUntil(t, "the first answer", func() bool { return string(h.State()) == before })
				fe.send(&ObserverStateChangedMessage{ID: id, State: "error", Reason: &ErrorBody{Code: "UNKNOWN_DB", Message: "rejected by the host"}})
				waitUntil(t, "the host's rejection", func() bool { return h.State() == ObserverError })
			}

			fe.reconnect(c)
			fe.waitFrames(MsgObserve, 2, func(d *DecodedMessage) bool { return d.Observe.ID == id })
			fe.send(&ObserveResultMessage{ID: id, State: "error", Error: &ErrorBody{Code: "OWNER_TIMEOUT", Message: "the host did not answer"}})
			waitUntil(t, "the replay's answer", func() bool { r := h.Reason(); return r != nil && r.Code == "OWNER_TIMEOUT" })
			if l := listed(c, id); len(l) != 1 || l[0] != h {
				t.Errorf("listed %v after the replay's error answer, want the handle", l)
			}
			fe.send(&ObserverStateChangedMessage{ID: id, State: "active"})
			waitUntil(t, "a later state change reaches it", func() bool { return h.State() == ObserverActive })

			fe.reconnect(c)
			fe.waitFrames(MsgObserve, 3, func(d *DecodedMessage) bool { return d.Observe.ID == id })
			disposeQuickly(t, h)
			fe.waitFrames(MsgUnobserve, 1, func(d *DecodedMessage) bool { return d.Unobserve.ID == id })
		})
	}
}

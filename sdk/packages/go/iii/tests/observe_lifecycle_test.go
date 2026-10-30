//go:build integration

package iii_test

import (
	"context"
	"slices"
	"sync"
	"testing"
	"time"

	iii "github.com/iii-hq/iii/sdk/packages/go/iii"
)

// An observer's lifecycle in the worker that hosts its function, against the real
// engine: the removed hooks of a rejected observer, and a standby taking over a
// function's observers when its host leaves.

// hookRecord records the added and removed hooks of one function by observer id, and
// keeps the last Observer the added hook got.
type hookRecord struct {
	mu      sync.Mutex
	added   []string
	removed []string
	last    *iii.Observer
}

func (r *hookRecord) set(c *iii.Client, fn string, added func(*iii.Observer)) {
	c.OnObserverAddedFor(fn, func(_ context.Context, obs *iii.Observer) error {
		r.mu.Lock()
		r.added = append(r.added, obs.ID)
		r.last = obs
		r.mu.Unlock()
		if added != nil {
			added(obs)
		}
		return nil
	})
	c.OnObserverRemovedFor(fn, func(_ context.Context, obs *iii.Observer) {
		r.mu.Lock()
		r.removed = append(r.removed, obs.ID)
		r.mu.Unlock()
	})
}

func (r *hookRecord) snapshot() (added, removed []string, last *iii.Observer) {
	r.mu.Lock()
	defer r.mu.Unlock()
	return slices.Clone(r.added), slices.Clone(r.removed), r.last
}

// obs.Reject runs the observer's removed hooks exactly once, whether the host is
// connected when it rejects or not. A rejection the host sends on the replay after a
// reconnect answers an announcement it had not acked yet, so the engine sends no
// observerremoved for it; neither does a later reconnect run the hooks again.
func TestObserveRejectRunsTheRemovedHooksOnce(t *testing.T) {
	for _, tc := range []struct {
		name    string
		offline bool
	}{{"live", false}, {"offline", true}} {
		t.Run(tc.name, func(t *testing.T) {
			proxy := newFrameProxy(t)
			host := observeWorker(t, "reject-host", proxy)
			watcher := observeWorker(t, "reject-watcher", nil)
			fn, target := fnID(t, "go-reject::source"), fnID(t, "go-reject::target")
			var rec hookRecord
			rec.set(host, fn, nil)
			if _, err := host.RegisterFunction(fn, iii.Identity); err != nil {
				t.Fatal(err)
			}
			_ = collector(t, watcher, target)
			h, err := watcher.ObserveFunction(fn, target)
			if err != nil {
				t.Fatal(err)
			}
			ready(t, h)
			_, _, obs := rec.snapshot()
			if obs == nil || obs.ID != h.ID() {
				t.Fatalf("added hook got %v, want observer %s", obs, h.ID())
			}

			if tc.offline {
				proxy.hold()
				proxy.drop()
				waitFor(t, "the disconnect", 5*time.Second, func() bool { return host.State() != iii.StateConnected })
				obs.Reject("GONE", "rejected while offline")
				proxy.release()
				waitFor(t, "the reconnect", 10*time.Second, func() bool { return host.State() == iii.StateConnected })
			} else {
				obs.Reject("GONE", "rejected live")
			}
			waitFor(t, "the observer in error", 10*time.Second, func() bool { return h.State() == iii.ObserverError })
			if r := h.Reason(); r == nil || r.Code != "GONE" {
				t.Errorf("reason %+v, want GONE", r)
			}
			waitFor(t, "the removed hook", 10*time.Second, func() bool {
				_, removed, _ := rec.snapshot()
				return len(removed) > 0
			})
			// a reconnect replays the function: the observer is gone, nothing runs again
			proxy.drop()
			waitFor(t, "the reconnect", 10*time.Second, func() bool { return host.State() == iii.StateConnected })
			time.Sleep(500 * time.Millisecond)
			added, removed, _ := rec.snapshot()
			if !slices.Equal(added, []string{h.ID()}) || !slices.Equal(removed, []string{h.ID()}) {
				t.Errorf("added %v removed %v, want one of each for %s", added, removed, h.ID())
			}
			if len(host.Observers(fn)) != 0 {
				t.Errorf("observers %v after the rejection", host.Observers(fn))
			}
		})
	}
}

// A Reject made while the observer's added hooks run rejects it with that code: Ready
// fails with it, and the removed hook of the level whose added hook resolved runs once.
func TestObserveRejectDuringTheAddedHook(t *testing.T) {
	host := observeWorker(t, "reject-add-host", nil)
	watcher := observeWorker(t, "reject-add-watcher", nil)
	fn, target := fnID(t, "go-reject-add::source"), fnID(t, "go-reject-add::target")
	var rec hookRecord
	rec.set(host, fn, func(obs *iii.Observer) { obs.Reject("UNKNOWN_DB", "the db went away during setup") })
	if _, err := host.RegisterFunction(fn, iii.Identity); err != nil {
		t.Fatal(err)
	}
	_ = collector(t, watcher, target)
	h, err := watcher.ObserveFunction(fn, target)
	if err != nil {
		t.Fatal(err)
	}
	if rej := readyFails(t, h); rej.Code != "UNKNOWN_DB" {
		t.Errorf("code %s, want UNKNOWN_DB", rej.Code)
	}
	waitFor(t, "the removed hook", 10*time.Second, func() bool {
		_, removed, _ := rec.snapshot()
		return len(removed) > 0
	})
	time.Sleep(500 * time.Millisecond)
	if added, removed, _ := rec.snapshot(); !slices.Equal(added, []string{h.ID()}) || !slices.Equal(removed, []string{h.ID()}) {
		t.Errorf("added %v removed %v, want one of each for %s", added, removed, h.ID())
	}
}

// Standby: a second worker registering a function another connection hosts gets
// FUNCTION_NAMESPACE_CONFLICT and registers it again about once a second. When the
// host leaves, the standby's registration is accepted, it runs its added hook for the
// function's observer, and the observer is active again: the standby's calls reach the
// target.
func TestObserveStandbyTakesOverTheObservers(t *testing.T) {
	host := observeWorker(t, "standby-host", nil)
	standby := observeWorker(t, "standby-standby", nil)
	watcher := observeWorker(t, "standby-watcher", nil)
	fn, target := fnID(t, "go-standby::source"), fnID(t, "go-standby::target")
	var hostHooks, standbyHooks hookRecord
	hostHooks.set(host, fn, nil)
	standbyHooks.set(standby, fn, nil)
	if _, err := host.RegisterFunction(fn, iii.Identity); err != nil {
		t.Fatal(err)
	}
	localDispatchReady(t, host, fn)
	got := collector(t, watcher, target)
	h, err := watcher.ObserveFunction(fn, target)
	if err != nil {
		t.Fatal(err)
	}
	ready(t, h)
	if _, err := standby.RegisterFunction(fn, iii.Identity, iii.RegisterFunctionOptions{Standby: true}); err != nil {
		t.Fatal(err)
	}
	time.Sleep(1500 * time.Millisecond) // a retry or two, refused while the host is there
	if standby.LocalDispatchActive(fn) {
		t.Fatal("the standby was accepted while the host is connected")
	}

	_ = host.Close()
	waitFor(t, "the standby's registration", 10*time.Second, func() bool { return standby.LocalDispatchActive(fn) })
	waitFor(t, "the standby's added hook", 10*time.Second, func() bool {
		added, _, _ := standbyHooks.snapshot()
		return slices.Equal(added, []string{h.ID()})
	})
	waitFor(t, "the observer active again", 10*time.Second, func() bool {
		return standby.IsObserved(fn) && h.State() == iii.ObserverActive
	})
	call(t, standby, iii.CallRequest{FunctionID: fn, Payload: raw(map[string]int{"n": 7})})
	a := exactly(t, got, 1, "the activation from the standby", 10*time.Second)[0]
	if a.Data["observer_id"] != h.ID() || field(a.Data, "input.n") != float64(7) {
		t.Errorf("envelope %v", a.Data)
	}
	if added, _, _ := hostHooks.snapshot(); !slices.Equal(added, []string{h.ID()}) {
		t.Errorf("host added %v", added)
	}
}

//go:build integration

package iii_test

import (
	"context"
	"encoding/json"
	"errors"
	"testing"
	"time"

	iii "github.com/iii-hq/iii/sdk/packages/go/iii"
)

// Observer ids against a real engine (decision 56): a held id is refused with nothing
// sent; an id observed again right after Dispose gets a handle that the engine's late
// disposed of the earlier observer does not reach. The new observer delivers, and its
// Dispose stops it on the engine.
func TestObserveIDReusedRightAfterDispose(t *testing.T) {
	owner := observeWorker(t, "ids-owner", nil)
	proxy := newFrameProxy(t)
	watcher := observeWorker(t, "ids-watcher", proxy)
	src, tgt, id := fnID(t, "go-ids::src"), fnID(t, "go-ids::tgt"), fnID(t, "go-ids")
	if _, err := owner.RegisterFunction(src, func(_ context.Context, data json.RawMessage) (any, error) { return data, nil }); err != nil {
		t.Fatal(err)
	}
	got := collector(t, watcher, tgt)

	first, err := watcher.Observe(iii.Source{FunctionID: src}, tgt, iii.ObserverOptions{ID: id})
	if err != nil {
		t.Fatal(err)
	}
	ready(t, first)
	mark := proxy.mark()
	var rej *iii.ObserveRejectionError
	if again, err := watcher.Observe(iii.Source{FunctionID: src}, tgt, iii.ObserverOptions{ID: id}); again != nil || !errors.As(err, &rej) || rej.Code != "OBSERVER_ID_CONFLICT" {
		t.Fatalf("Observe of a held id: %v, %v", again, err)
	}
	if n := countOf(proxy.since(mark), "observe"); n != 0 {
		t.Fatalf("the refused Observe sent %d observe frames", n)
	}

	if err := first.Dispose(context.Background()); err != nil {
		t.Fatal(err)
	}
	second, err := watcher.Observe(iii.Source{FunctionID: src}, tgt, iii.ObserverOptions{ID: id})
	if err != nil {
		t.Fatalf("Observe right after Dispose: %v", err)
	}
	ready(t, second)
	settle() // the engine's disposed of the first observer has arrived by now
	if second.State() != iii.ObserverActive {
		t.Fatalf("new handle %s %+v, want active", second.State(), second.Reason())
	}
	call(t, owner, iii.CallRequest{FunctionID: src, Payload: raw(1)})
	exactly(t, got, 1, "the new observer's activation", 10*time.Second)

	if err := second.Dispose(context.Background()); err != nil {
		t.Fatal(err)
	}
	waitFor(t, "the engine to dispose the new observer", 10*time.Second, func() bool {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		out, err := watcher.Call(ctx, iii.CallRequest{FunctionID: iii.FnGetObserver, Payload: raw(map[string]string{"id": id})})
		if err != nil {
			return true // OBSERVER_NOT_FOUND
		}
		var obs struct {
			State string `json:"state"`
		}
		_ = json.Unmarshal(out, &obs)
		return obs.State != "active"
	})
	call(t, owner, iii.CallRequest{FunctionID: src, Payload: raw(2)})
	time.Sleep(300 * time.Millisecond)
	if n := got.len(); n != 1 {
		t.Errorf("%d activations after the new handle was disposed, want 1", n)
	}
}

// A refused observe against a real engine (decision 56, R1): the engine stores
// nothing for it, the handle's Dispose sends nothing, an unobserve already on the wire
// is ignored, and the id observed again right after works and delivers.
func TestObserveRefusedIsFinal(t *testing.T) {
	owner := observeWorker(t, "refused-owner", nil)
	proxy := newFrameProxy(t)
	watcher := observeWorker(t, "refused-watcher", proxy)
	src, tgt, id := fnID(t, "go-refused::src"), fnID(t, "go-refused::tgt"), fnID(t, "go-refused")
	if _, err := owner.RegisterFunction(src, func(_ context.Context, data json.RawMessage) (any, error) { return data, nil }); err != nil {
		t.Fatal(err)
	}
	got := collector(t, watcher, tgt)
	bad := iii.Source{FunctionID: src, Filters: iii.Filters{"input.op": "insert"}}
	// held: the engine has an observer under id that is not disposed
	held := func() bool {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		out, err := watcher.Call(ctx, iii.CallRequest{FunctionID: iii.FnGetObserver, Payload: raw(map[string]string{"id": id})})
		if err != nil {
			return false // OBSERVER_NOT_FOUND
		}
		var obs struct {
			State string `json:"state"`
		}
		_ = json.Unmarshal(out, &obs)
		return obs.State != "disposed"
	}
	quick := func(h *iii.ObserverHandle) {
		t.Helper()
		start := time.Now()
		if err := h.Dispose(context.Background()); err != nil {
			t.Fatal(err)
		}
		if d := time.Since(start); d > 250*time.Millisecond {
			t.Errorf("Dispose took %s", d)
		}
	}

	// refused, then disposed: nothing goes out
	refused, err := watcher.Observe(bad, tgt, iii.ObserverOptions{ID: id})
	if err != nil {
		t.Fatal(err)
	}
	if rej := readyFails(t, refused); rej.Code != "INVALID_FILTER" {
		t.Fatalf("code %s (%s), want INVALID_FILTER", rej.Code, rej.Message)
	}
	if held() {
		t.Fatal("the engine holds a refused observer")
	}
	mark := proxy.mark()
	quick(refused)
	settle()
	if n := countOf(proxy.since(mark), "unobserve"); n != 0 {
		t.Fatalf("Dispose of a refused observer sent %d unobserves", n)
	}

	// disposed before its refusal: the unobserve goes out and the engine ignores it
	early, err := watcher.Observe(bad, tgt, iii.ObserverOptions{ID: id})
	if err != nil {
		t.Fatal(err)
	}
	quick(early)

	// the id observed again works
	second, err := watcher.Observe(iii.Source{FunctionID: src}, tgt, iii.ObserverOptions{ID: id})
	if err != nil {
		t.Fatalf("Observe after the refused handles were disposed: %v", err)
	}
	ready(t, second)
	settle()
	if second.State() != iii.ObserverActive {
		t.Fatalf("new handle %s %+v, want active", second.State(), second.Reason())
	}
	call(t, owner, iii.CallRequest{FunctionID: src, Payload: raw(1)})
	exactly(t, got, 1, "the new observer's activation", 10*time.Second)
	mark = proxy.mark()
	quick(second)
	waitFor(t, "the unobserve", 5*time.Second, func() bool { return countOf(proxy.since(mark), "unobserve") == 1 })
	waitFor(t, "the engine to dispose the new observer", 10*time.Second, func() bool { return !held() })
}

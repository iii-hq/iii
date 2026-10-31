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

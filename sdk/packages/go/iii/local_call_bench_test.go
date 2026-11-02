package iii

import (
	"context"
	"encoding/json"
	"fmt"
	"testing"
)

// The cost of a local call of a function whose observers each filter on their own
// value of input.tenant ({"input.tenant": {"$eq": "tenant-<i>"}}), 1,000 or 4 of them: a
// call that none of them match, and one that one of them matches (and is reported). A
// function with a deliver hook, with no observers, and with 4 whose filters the hook
// evaluates for none of them or for each (Observer.Matches). 64 observers each on its own
// path ({"input.p<i>": {"$eq": 1}}) and a call that meets every one: where the index's
// lookups spare nothing. A function nobody observes is the floor. The engine is the fake
// one; report frames are discarded.
func BenchmarkLocalCall(b *testing.B) {
	fe := newFakeEngine(b)
	tenants := func(n int) []ObserverInfo {
		set := make([]ObserverInfo, n)
		for i := range set {
			set[i] = observerInfo(fmt.Sprintf("o-%04d", i), "x::y", "", fmt.Sprintf(`{"$and":[{"input.tenant":{"$eq":"tenant-%d"}}]}`, i))
		}
		return set
	}
	sizes := map[string]int{
		"bench::filtered": 1000, "bench::few": 4,
		"bench::deliver-unread": 4, "bench::deliver-matches": 4,
	}
	for fn, n := range sizes {
		fe.setObservers(fn, 1, tenants(n)...)
	}
	paths := make([]ObserverInfo, 64)
	for i := range paths {
		paths[i] = observerInfo(fmt.Sprintf("p-%04d", i), "x::y", "", fmt.Sprintf(`{"input.p%d":{"$eq":1}}`, i))
	}
	fe.setObservers("bench::paths", 1, paths...)
	sizes["bench::paths"] = len(paths)
	c := fe.client()
	deliver := func(_ context.Context, call *Call, _ []*Observer) (any, error) { return call.Output, nil }
	c.OnObserverDeliverFor("bench::deliver-none", deliver)
	c.OnObserverDeliverFor("bench::deliver-unread", deliver)
	c.OnObserverDeliverFor("bench::deliver-matches", func(_ context.Context, call *Call, observers []*Observer) (any, error) {
		for _, obs := range observers {
			obs.Matches(call)
		}
		return call.Output, nil
	})
	fns := []string{"bench::plain", "bench::filtered", "bench::few", "bench::paths", "bench::deliver-none", "bench::deliver-unread", "bench::deliver-matches"}
	for _, fn := range fns {
		registerHost(b, c, fn, echoHandler)
	}
	waitUntil(b, "the observers active", func() bool {
		for fn, n := range sizes {
			if len(c.Observers(fn)) != n {
				return false
			}
		}
		return true
	})
	waitUntil(b, "local dispatch", func() bool {
		for _, fn := range fns {
			if !c.LocalDispatchActive(fn) {
				return false
			}
		}
		return true
	})

	run := func(fn, payload string) func(b *testing.B) {
		return func(b *testing.B) {
			ctx := context.Background()
			req := CallRequest{FunctionID: fn, Payload: json.RawMessage(payload)}
			b.ReportAllocs()
			n := 0
			for b.Loop() {
				if _, err := c.Call(ctx, req); err != nil {
					b.Fatal(err)
				}
				if n++; n%4096 == 0 {
					fe.m.clear()
				}
			}
		}
	}
	const one, none = `{"tenant":"tenant-2","total":1250}`, `{"tenant":"nobody","total":1250}`
	b.Run("no_observers", run("bench::plain", `{"tenant":"tenant-500","total":1250}`))
	b.Run("1000_eq_observers_none_matching", run("bench::filtered", none))
	b.Run("1000_eq_observers_1_matching", run("bench::filtered", `{"tenant":"tenant-500","total":1250}`))
	b.Run("4_eq_observers_none_matching", run("bench::few", none))
	b.Run("4_eq_observers_1_matching", run("bench::few", one))
	allPaths := map[string]int{}
	for i := range paths {
		allPaths[fmt.Sprintf("p%d", i)] = 1
	}
	meetsAll, _ := json.Marshal(allPaths)
	b.Run("64_paths_all_matching", run("bench::paths", string(meetsAll)))
	b.Run("deliver_hook_0_observers", run("bench::deliver-none", one))
	b.Run("deliver_hook_4_observers_no_filters_read", run("bench::deliver-unread", one))
	b.Run("deliver_hook_4_observers_matches_each", run("bench::deliver-matches", one))
}

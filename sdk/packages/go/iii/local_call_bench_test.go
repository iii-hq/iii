package iii

import (
	"context"
	"encoding/json"
	"fmt"
	"testing"
)

// The cost of a local call of a function with 1,000 observers that each filter on their
// own value of input.tenant ({"input.tenant": {"$eq": "tenant-<i>"}}): a call that none
// of them match, and one that one of them matches (and is reported). A function nobody
// observes is the floor. The engine is the fake one; report frames are discarded.
func BenchmarkLocalCall(b *testing.B) {
	const observers = 1000
	fe := newFakeEngine(b)
	set := make([]ObserverInfo, observers)
	for i := range set {
		set[i] = observerInfo(fmt.Sprintf("o-%04d", i), "x::y", "", fmt.Sprintf(`{"$and":[{"input.tenant":{"$eq":"tenant-%d"}}]}`, i))
	}
	fe.setObservers("bench::filtered", 1, set...)
	c := fe.client()
	registerHost(b, c, "bench::plain", echoHandler)
	registerHost(b, c, "bench::filtered", echoHandler)
	waitUntil(b, "the observers active", func() bool { return len(c.Observers("bench::filtered")) == observers })
	waitUntil(b, "local dispatch", func() bool {
		return c.LocalDispatchActive("bench::plain") && c.LocalDispatchActive("bench::filtered")
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
	b.Run("no_observers", run("bench::plain", `{"tenant":"tenant-500","total":1250}`))
	b.Run("1000_eq_observers_none_matching", run("bench::filtered", `{"tenant":"nobody","total":1250}`))
	b.Run("1000_eq_observers_1_matching", run("bench::filtered", `{"tenant":"tenant-500","total":1250}`))
}

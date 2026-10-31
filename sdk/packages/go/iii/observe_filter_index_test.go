package iii

import (
	"context"
	"encoding/json"
	"fmt"
	"maps"
	"slices"
	"sync"
	"testing"
	"time"
)

// The filter index of a function follows its active set through the engine's
// messages: an observer removed, announced again with another view, or dropped with
// its function's registration and confirmed again is reported as the active set says,
// in its order. A call handed to a hook evaluates Observer.Matches through a view made
// on its first use, and a copy of the call with other fields is evaluated on its own
// fields.

// reportedTo makes a local call of fn with input (compact JSON, distinct across the
// test's calls) and returns the observers its report item lists.
func reportedTo(t *testing.T, fe *fakeEngine, c *Client, fn, input string) []string {
	t.Helper()
	call(t, c, CallRequest{FunctionID: fn, Payload: json.RawMessage(input)})
	var matched []string
	waitUntil(t, "the report of "+input, func() bool {
		for _, item := range fe.reportItems() {
			if item.FunctionID == fn && string(item.Input) == input {
				matched = item.Matched
				return true
			}
		}
		return false
	})
	return matched
}

// A local call after an observer is removed reaches the others only, whether the
// removed one shared its value with them or not. Every call matches the scanned
// observer s, so every call is reported.
func TestALocalCallAfterARemovalReachesTheRestOnly(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	const fn = "fx::remove"
	registerHost(t, c, fn, echoHandler)
	for _, id := range []string{"a", "b", "c"} {
		addObserver(t, fe, fn, observerInfo(id, "x::y", "", `{"$and":[{"input.k":{"$eq":1}}]}`))
	}
	addObserver(t, fe, fn, observerInfo("d", "x::y", "", `{"input.k":{"$eq":2}}`))
	addObserver(t, fe, fn, observerInfo("s", "x::y", "", `{"input.n":{"$gte":0}}`))

	n := 0
	check := func(k int, want ...string) {
		t.Helper()
		n++
		if got := reportedTo(t, fe, c, fn, fmt.Sprintf(`{"k":%d,"n":%d}`, k, n)); !slices.Equal(got, want) {
			t.Errorf("call %d (k %d): reported to %v, want %v", n, k, got, want)
		}
	}
	remove := func(id string) {
		t.Helper()
		left := len(c.Observers(fn)) - 1
		fe.remove(fn, id)
		waitUntil(t, id+" removed", func() bool { return len(c.Observers(fn)) == left })
	}
	check(1, "a", "b", "c", "s")
	remove("b")
	check(1, "a", "c", "s")
	remove("a")
	check(1, "c", "s")
	check(2, "d", "s")
	remove("d")
	check(2, "s")
	remove("s")
	check(1, "c")
}

// An observer announced again with other filters (a changed view) keeps its place in
// the active set: a call reports it where it was, its old view is not reported, and
// the order holds when the view moves it to the list every call scans.
func TestAChangedViewKeepsItsPlaceInTheReports(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	const fn = "fx::view"
	registerHost(t, c, fn, echoHandler)
	for _, id := range []string{"a", "b", "c"} {
		addObserver(t, fe, fn, observerInfo(id, "x::y", "", `{"input.k":{"$eq":1}}`))
	}

	n := 0
	check := func(k int, want ...string) {
		t.Helper()
		n++
		if got := reportedTo(t, fe, c, fn, fmt.Sprintf(`{"k":%d,"n":%d}`, k, n)); !slices.Equal(got, want) {
			t.Errorf("call %d (k %d): reported to %v, want %v", n, k, got, want)
		}
	}
	for _, view := range []struct {
		filters string
		k1, k2  []string
	}{
		{`{"input.k":{"$in":[1,2]}}`, []string{"a", "b", "c"}, []string{"b"}},
		{`{"input.k":{"$eq":2}}`, []string{"a", "c"}, []string{"b"}},
		{`{"input.n":{"$gte":0}}`, []string{"a", "b", "c"}, []string{"b"}},
		{`{"input.k":{"$eq":1}}`, []string{"a", "b", "c"}, nil},
	} {
		addObserver(t, fe, fn, observerInfo("b", "x::y", "", view.filters))
		check(1, view.k1...)
		if view.k2 != nil {
			check(2, view.k2...)
		}
	}
	var ids []string
	for _, obs := range c.Observers(fn) {
		ids = append(ids, obs.ID)
	}
	if !slices.Equal(ids, []string{"a", "b", "c"}) {
		t.Errorf("observers %v, want [a b c]", ids)
	}
}

// When the engine refuses a function its observers leave the index with the active
// set; when it confirms the function again, a call reaches the observers of the new
// confirmation only.
func TestALocalCallAfterARefusalAndANewConfirmation(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	c.standbyInterval = 20 * time.Millisecond
	const fn = "fx::release"
	registerHost(t, c, fn, echoHandler, RegisterFunctionOptions{Standby: true})
	addObserver(t, fe, fn, observerInfo("a", "x::y", "", `{"input.k":{"$eq":1}}`))
	addObserver(t, fe, fn, observerInfo("b", "x::y", "", `{"input.k":{"$eq":1}}`))
	if got := reportedTo(t, fe, c, fn, `{"k":1,"n":1}`); !slices.Equal(got, []string{"a", "b"}) {
		t.Fatalf("before the refusal: reported to %v, want [a b]", got)
	}

	// The confirmation the standby registration gets carries b and c.
	fe.setObservers(fn, 3,
		observerInfo("b", "x::y", "", `{"input.k":{"$eq":1}}`),
		observerInfo("c", "x::y", "", `{"input.n":{"$gte":0}}`))
	fe.send(&RegistrationRejectedMessage{Code: "FUNCTION_NAMESPACE_CONFLICT", Namespace: "default", FunctionID: fn, OwnerWorkerID: "other-host"})
	for _, id := range []string{"b", "c"} {
		if a := fe.waitAck(fn, id, 3); a.Error != nil {
			t.Fatalf("%s %+v", id, a.Error)
		}
	}
	waitUntil(t, "confirmed again", func() bool { return c.LocalDispatchActive(fn) && len(c.Observers(fn)) == 2 })
	if got := reportedTo(t, fe, c, fn, `{"k":1,"n":2}`); !slices.Equal(got, []string{"b", "c"}) {
		t.Errorf("after the new confirmation: reported to %v, want [b c]", got)
	}
	if got := reportedTo(t, fe, c, fn, `{"k":2,"n":3}`); !slices.Equal(got, []string{"c"}) {
		t.Errorf("after the new confirmation: reported to %v, want [c]", got)
	}
}

// A hook may evaluate Observer.Matches on a copy of the call with other fields: the
// copy is evaluated on its own fields, the call and an unchanged copy on the call's.
func TestMatchesOnAModifiedCopyOfTheCall(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	const fn = "fx::copy"
	observers := map[string]string{
		"in":     `{"input.k":{"$eq":1}}`,
		"out":    `{"output.k":{"$eq":1}}`,
		"meta":   `{"metadata.tag":{"$eq":"x"}}`,
		"scan":   `{"input.n":{"$gte":1}}`,
		"err":    `{"error.code":{"$eq":"E"}}`,
		"caller": `{"caller.function_id":{"$eq":"f"}}`,
		"other":  `{"input.k":{"$eq":2}}`,
	}
	type result struct {
		what, id  string
		got, want bool
	}
	results := make(chan []result, 1)
	c.OnObserverDeliverFor(fn, func(_ context.Context, call *Call, obs []*Observer) (any, error) {
		bare := func(call Call) *Call {
			call.hook = nil
			return &call
		}
		failed := *call
		failed.Status, failed.Output, failed.Error = "error", nil, &ErrorBody{Code: "E", Message: "m"}
		copies := map[string]func(Call) Call{
			"unchanged": func(c Call) Call { return c },
			"input":     func(c Call) Call { c.Input = json.RawMessage(`{"k":2,"n":0}`); return c },
			"output":    func(c Call) Call { c.Output = json.RawMessage(`{"k":3}`); return c },
			"metadata":  func(c Call) Call { c.Metadata = json.RawMessage(`{"tag":"y"}`); return c },
			"caller":    func(c Call) Call { c.Caller = &Caller{FunctionID: "f"}; return c },
			"failed":    func(Call) Call { return failed },
		}
		var out []result
		for _, o := range obs {
			// the call first, so the view is made from it
			out = append(out, result{"call", o.ID, o.Matches(call), o.Matches(bare(*call))})
			for what, change := range copies {
				changed := change(*call)
				out = append(out, result{what, o.ID, o.Matches(&changed), o.Matches(bare(changed))})
			}
		}
		results <- out
		return call.Output, nil
	})
	registerHost(t, c, fn, echoHandler)
	for _, id := range slices.Sorted(maps.Keys(observers)) {
		info := observerInfo(id, "x::y", "", observers[id])
		info.On = OnComplete
		addObserver(t, fe, fn, info)
	}
	call(t, c, CallRequest{FunctionID: fn, Payload: json.RawMessage(`{"k":1,"n":1}`), Metadata: json.RawMessage(`{"tag":"x"}`)})
	out := <-results
	if len(out) != len(observers)*7 {
		t.Fatalf("%d results, want %d", len(out), len(observers)*7)
	}
	matchedCall := 0
	for _, r := range out {
		if r.got != r.want {
			t.Errorf("%s: %s matches %v, want %v", r.what, r.id, r.got, r.want)
		}
		if r.what == "call" && r.got {
			matchedCall++
		}
	}
	// in, out, meta and scan match the call itself; each copy fails at least one
	if matchedCall != 4 {
		t.Errorf("%d observers match the call, want 4", matchedCall)
	}
}

// Deliver hooks may evaluate Observer.Matches from several goroutines at once, on
// observers filed under a condition and on observers every call scans: each part of
// the call is decoded once for all of them (run under -race).
func TestMatchesFromConcurrentGoroutinesOfADeliverHook(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	const fn = "fx::concurrent"
	filters := []string{
		`{"output.n":{"$gte":2}}`,
		`{"input.n":{"$lt":3}}`,
		`{"metadata.tag":{"$exists":true}}`,
		`{"input.k":{"$eq":1}}`,
		`{"input.k":{"$eq":0},"output.n":{"$gte":0}}`,
	}
	const goroutines = 8
	mismatches := make(chan string, 64)
	c.OnObserverDeliverFor(fn, func(_ context.Context, call *Call, obs []*Observer) (any, error) {
		bare := *call
		bare.hook = nil
		want := make([]bool, len(obs))
		start := make(chan struct{})
		var wg sync.WaitGroup
		for range goroutines {
			wg.Add(1)
			go func() {
				defer wg.Done()
				<-start
				for i, o := range obs {
					if o.Matches(call) != want[i] {
						select {
						case mismatches <- fmt.Sprintf("%s on %s", o.ID, call.Input):
						default:
						}
					}
				}
			}()
		}
		for i, o := range obs {
			want[i] = o.Matches(&bare)
		}
		close(start)
		wg.Wait()
		return call.Output, nil
	})
	registerHost(t, c, fn, echoHandler)
	for i, f := range filters {
		addObserver(t, fe, fn, observerInfo(fmt.Sprintf("o%d", i), "x::y", "", f))
	}
	for i := range 40 {
		meta := json.RawMessage(nil)
		if i%2 == 0 {
			meta = json.RawMessage(`{"tag":"x"}`)
		}
		call(t, c, CallRequest{FunctionID: fn, Payload: json.RawMessage(fmt.Sprintf(`{"k":%d,"n":%d}`, i%2, i%5)), Metadata: meta})
		if i%8 == 0 {
			fe.waitResult(fe.invoke(InvokeFunctionMessage{FunctionID: fn, Data: json.RawMessage(fmt.Sprintf(`{"k":%d,"n":%d}`, i%2, i%5))}))
		}
	}
	close(mismatches)
	for m := range mismatches {
		t.Errorf("Matches through the view disagrees with the bare call: %s", m)
	}
}

// A call handed to a hook of a function with observers carries a view, made on its
// first Observer.Matches; a call of a function with no observers carries none.
func TestAHookCallMakesItsViewOnFirstUse(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	type seen struct{ hooked, before, after bool }
	got := make(chan seen, 1)
	c.OnObserverDeliverFor("fx::lazy", func(_ context.Context, call *Call, obs []*Observer) (any, error) {
		s := seen{hooked: call.hook != nil}
		if s.hooked {
			s.before = call.hook.view != nil
			for _, o := range obs {
				o.Matches(call)
			}
			s.after = call.hook.view != nil
		}
		got <- s
		return call.Output, nil
	})
	registerHost(t, c, "fx::lazy", echoHandler)
	call(t, c, CallRequest{FunctionID: "fx::lazy", Payload: json.RawMessage(`{"k":1}`)})
	if s := <-got; s.hooked {
		t.Errorf("a call with no observers carries a view: %+v", s)
	}
	addObserver(t, fe, "fx::lazy", observerInfo("a", "x::y", "", `{"input.k":{"$eq":1}}`))
	call(t, c, CallRequest{FunctionID: "fx::lazy", Payload: json.RawMessage(`{"k":1}`)})
	if s := <-got; !s.hooked || s.before || !s.after {
		t.Errorf("hooked %v, view before Matches %v, after %v; want true, false, true", s.hooked, s.before, s.after)
	}
}

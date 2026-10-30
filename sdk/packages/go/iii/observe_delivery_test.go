package iii

import (
	"context"
	"encoding/json"
	"errors"
	"slices"
	"testing"
	"time"
)

// addObserver attaches an observer to fn and waits until the SDK accepted it.
func addObserver(t *testing.T, fe *fakeEngine, fn string, info ObserverInfo) {
	t.Helper()
	rev := fe.add(fn, info)
	if a := fe.waitAck(fn, info.ID, rev); a.Error != nil {
		t.Fatalf("%s rejected: %+v", info.ID, a.Error)
	}
}

// handlerAnswer is a deliver hook that delivers nothing and answers with the handler's
// output.
func handlerAnswer(_ context.Context, call *Call, _ []*Observer) (any, error) {
	return call.Output, nil
}

// cronMatch matches an observer of args { expression } to the ticks of its expression.
func cronMatch() *MatchHook {
	return MatchFunc(func(obs *Observer, call *Call) bool {
		var tick struct {
			Expression string `json:"expression"`
		}
		_ = json.Unmarshal(call.Input, &tick)
		return obs.Args["expression"] == tick.Expression
	})
}

// rowKeys matches a row-change observer of args { db, table?, ops? } to a change
// { db, table, op } by keys, with "*" for every table or operation.
func rowKeys() *MatchHook {
	rowKey := func(db, table, op string) string { return db + "|" + table + "|" + op }
	return MatchKeys(
		func(obs *Observer) []string {
			var args struct {
				DB    string   `json:"db"`
				Table string   `json:"table"`
				Ops   []string `json:"ops"`
			}
			_ = obs.DecodeArgs(&args)
			if args.Table == "" {
				args.Table = "*"
			}
			if len(args.Ops) == 0 {
				args.Ops = []string{"*"}
			}
			var keys []string
			for _, op := range args.Ops {
				keys = append(keys, rowKey(args.DB, args.Table, op))
			}
			return keys
		},
		func(call *Call) []string {
			var change struct{ DB, Table, Op string }
			_ = json.Unmarshal(call.Input, &change)
			var keys []string
			for _, table := range []string{change.Table, "*"} {
				for _, op := range []string{change.Op, "*"} {
					keys = append(keys, rowKey(change.DB, table, op))
				}
			}
			return keys
		},
	)
}

func rowChange(table, op string) json.RawMessage {
	data, _ := json.Marshal(map[string]string{"db": "primary", "table": table, "op": op})
	return data
}

// keyIndexOf is the key index of fn's installed match hook, or nil.
func keyIndexOf(c *Client, fn string) *keyIndex {
	f := c.functionObservers(fn)
	if f == nil {
		return nil
	}
	return f.state.Load().index
}

// A predicate match hook narrows iii's delivery to the observers it accepts, on local
// and routed calls, and their filters still apply. The handler returns its input, so
// the local item carries it once, with output_is_input.
func TestMatchFuncNarrowsTheDelivery(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	c.OnObserverMatchFor("cron::on-tick", cronMatch())
	registerHost(t, c, "cron::on-tick", Identity, RegisterFunctionOptions{Private: true})
	addObserver(t, fe, "cron::on-tick", observerInfo("daily", "x::y", `{"expression":"0 0 * * *"}`, ""))
	addObserver(t, fe, "cron::on-tick", observerInfo("hourly", "x::y", `{"expression":"0 * * * *"}`, ""))
	addObserver(t, fe, "cron::on-tick", observerInfo("hourly-late", "x::y", `{"expression":"0 * * * *"}`, `{"$and":[{"input.late":{"$eq":true}}]}`))
	tick := json.RawMessage(`{"expression":"0 * * * *","scheduled_time":"2026-11-01T10:00:00Z"}`)
	out, err := c.Call(context.Background(), CallRequest{FunctionID: "cron::on-tick", Payload: tick})
	if err != nil || string(out) != string(tick) {
		t.Fatalf("Call: %s %v", out, err)
	}
	item := fe.waitItems(1)[0]
	if !slices.Equal(item.Matched, []string{"hourly"}) || !item.OutputIsInput || item.Output != nil || string(item.Input) != string(tick) {
		t.Errorf("item %+v", item)
	}
	res := fe.waitResult(fe.invoke(InvokeFunctionMessage{FunctionID: "cron::on-tick", Data: tick}))
	if !slices.Equal(res.Observed, []string{"hourly"}) || string(res.Result) != string(tick) {
		t.Errorf("routed answer %+v", res)
	}
	time.Sleep(20 * time.Millisecond)
	if n := len(fe.reportItems()); n != 1 {
		t.Errorf("%d items, want 1 (the routed call rides its answer)", n)
	}
	if !c.IsObservedFor("cron::on-tick", &Call{Input: json.RawMessage(`{"expression":"* * * * *"}`)}) {
		t.Error("IsObservedFor reads the call on a function matched by a predicate")
	}
}

// A predicate that panics counts as no match for that observer, and is logged once.
func TestMatchPredicatePanicIsNoMatch(t *testing.T) {
	logs := captureLog(t)
	fe := newFakeEngine(t)
	c := fe.client()
	c.OnObserverMatchFor("pick::fn", MatchFunc(func(obs *Observer, _ *Call) bool {
		if obs.Args["boom"] == true {
			panic("bad args")
		}
		return true
	}))
	registerHost(t, c, "pick::fn", echoHandler)
	addObserver(t, fe, "pick::fn", observerInfo("fine", "x::y", "", ""))
	addObserver(t, fe, "pick::fn", observerInfo("boom", "x::y", `{"boom":true}`, ""))
	for range 2 {
		if _, err := c.Call(context.Background(), CallRequest{FunctionID: "pick::fn"}); err != nil {
			t.Fatal(err)
		}
	}
	for _, item := range fe.waitItems(2) {
		if !slices.Equal(item.Matched, []string{"fine"}) {
			t.Errorf("matched %v, want [fine]", item.Matched)
		}
	}
	if n := logs.count(`OnObserverMatch of "pick::fn" panicked`); n != 1 {
		t.Errorf("the panic logged %d times, want once:\n%s", n, logs.String())
	}
}

// Matching by keys files each accepted observer under its keys, unfiles it when it is
// removed or rejected, and gives a call the observers filed under its keys, each once,
// in key order. IsObservedFor is one lookup per call key.
func TestKeyIndexUnderAddAndRemove(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	c.OnObserverMatchFor("database::on-row-changed", rowKeys())
	ref := registerHost(t, c, "database::on-row-changed", Identity, RegisterFunctionOptions{Private: true})
	addObserver(t, fe, "database::on-row-changed", observerInfo("orders-insert", "x::y", `{"db":"primary","table":"orders","ops":["insert"]}`, ""))
	addObserver(t, fe, "database::on-row-changed", observerInfo("all", "x::y", `{"db":"primary"}`, ""))
	addObserver(t, fe, "database::on-row-changed", observerInfo("users", "x::y", `{"db":"primary","table":"users"}`, ""))

	idx := keyIndexOf(c, "database::on-row-changed")
	if idx == nil || idx.filed != 3 {
		t.Fatalf("index %v: want the three observers filed", idx)
	}
	observed := func(table, op string) bool { return ref.IsObservedFor(&Call{Input: rowChange(table, op)}) }
	if !observed("orders", "insert") || !observed("users", "update") || !observed("products", "delete") {
		t.Error("IsObservedFor misses a filed observer")
	}
	if c.IsObservedFor("database::on-row-changed", &Call{Input: json.RawMessage(`{"db":"replica","table":"orders","op":"insert"}`)}) {
		t.Error("IsObservedFor true for a database nobody observes")
	}

	if _, err := c.Call(context.Background(), CallRequest{FunctionID: "database::on-row-changed", Payload: rowChange("orders", "insert")}); err != nil {
		t.Fatal(err)
	}
	if item := fe.waitItems(1)[0]; !slices.Equal(item.Matched, []string{"orders-insert", "all"}) {
		t.Errorf("matched %v, want [orders-insert all]", item.Matched)
	}

	fe.remove("database::on-row-changed", "all")
	waitUntil(t, "all unfiled", func() bool { return !observed("products", "delete") })
	if !observed("users", "delete") || !observed("orders", "insert") || keyIndexOf(c, "database::on-row-changed").filed != 2 {
		t.Error("removing one observer unfiled another")
	}
	res := fe.waitResult(fe.invoke(InvokeFunctionMessage{FunctionID: "database::on-row-changed", Data: rowChange("users", "update")}))
	if !slices.Equal(res.Observed, []string{"users"}) {
		t.Errorf("routed observed %v, want [users]", res.Observed)
	}
	for _, obs := range c.Observers("database::on-row-changed") {
		if obs.ID == "orders-insert" {
			obs.Reject("UNKNOWN_TABLE", "orders was dropped")
		}
	}
	if observed("orders", "insert") || keyIndexOf(c, "database::on-row-changed").filed != 1 {
		t.Error("a rejected observer is still filed")
	}
	if _, err := c.Call(context.Background(), CallRequest{FunctionID: "database::on-row-changed", Payload: rowChange("orders", "insert")}); err != nil {
		t.Fatal(err)
	}
	time.Sleep(30 * time.Millisecond)
	if n := len(fe.reportItems()); n != 1 {
		t.Errorf("%d items: a call nobody is filed for was delivered", n)
	}
}

// A match hook set while observers are active is installed on the hook queue: a hook by
// keys gets its index built from them, a predicate replaces it, and nil restores every
// observer.
func TestMatchHookSetWithActiveObserversRebuildsTheIndex(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	ref := registerHost(t, c, "db::late", Identity)
	addObserver(t, fe, "db::late", observerInfo("users", "x::y", `{"db":"primary","table":"users"}`, ""))
	addObserver(t, fe, "db::late", observerInfo("orders", "x::y", `{"db":"primary","table":"orders"}`, ""))
	matched := func(payload json.RawMessage, want ...string) {
		t.Helper()
		mark := len(fe.reportItems())
		if _, err := c.Call(context.Background(), CallRequest{FunctionID: "db::late", Payload: payload}); err != nil {
			t.Fatal(err)
		}
		items := fe.waitItems(mark + 1)
		if got := items[mark].Matched; !slices.Equal(got, want) {
			t.Errorf("matched %v, want %v", got, want)
		}
	}

	if ref.OnObserverMatch(rowKeys()) != ref {
		t.Fatal("OnObserverMatch returns the handle")
	}
	waitUntil(t, "the index built", func() bool { idx := keyIndexOf(c, "db::late"); return idx != nil && idx.filed == 2 })
	matched(rowChange("orders", "update"), "orders")
	if ref.IsObservedFor(&Call{Input: rowChange("products", "update")}) {
		t.Error("IsObservedFor true for a table nobody observes")
	}

	ref.OnObserverMatch(MatchFunc(func(obs *Observer, _ *Call) bool { return obs.ID == "users" }))
	waitUntil(t, "the predicate installed", func() bool { return keyIndexOf(c, "db::late") == nil })
	matched(rowChange("orders", "update"), "users")

	ref.OnObserverMatch(nil)
	waitUntil(t, "the hook removed", func() bool { return c.functionObservers("db::late").state.Load().match == nil })
	matched(rowChange("orders", "update"), "users", "orders")
	if !ref.IsObservedFor(&Call{Input: rowChange("products", "update")}) {
		t.Error("IsObservedFor without a match hook is IsObserved")
	}
	c.mu.Lock()
	_, kept := c.functionHooks["db::late"]
	c.mu.Unlock()
	if kept || c.OnObserverMatchFor("db::late", &MatchHook{}) != c {
		t.Error("nil and the zero MatchHook remove the hook")
	}
}

// An observer whose keys panic is rejected with OBSERVE_REJECTED: when it is added (the
// removed hooks of the levels that resolved run), and when a hook by keys is set while
// it is active (late-rejected). A call whose keys panic matches nothing, logged once.
func TestKeysThatPanic(t *testing.T) {
	logs := captureLog(t)
	fe := newFakeEngine(t)
	c := fe.client()
	tableKeys := MatchKeys(
		func(obs *Observer) []string { return []string{obs.Args["table"].(string)} },
		func(call *Call) []string {
			var change map[string]string
			_ = json.Unmarshal(call.Input, &change)
			if change["table"] == "" {
				panic("a change without a table")
			}
			return []string{change["table"]}
		},
	)
	var l hookLog
	recordHooks(c, "db::keys", &l)
	c.OnObserverMatchFor("db::keys", tableKeys)
	registerHost(t, c, "db::keys", Identity)
	rev := fe.add("db::keys", observerInfo("no-table", "x::y", "", ""))
	if a := fe.waitAck("db::keys", "no-table", rev); a.Error == nil || a.Error.Code != "OBSERVE_REJECTED" {
		t.Fatalf("ack %+v, want OBSERVE_REJECTED", a)
	}
	waitUntil(t, "the removed hook of the resolved level", func() bool { _, r := l.snapshot(); return slices.Equal(r, []string{"no-table"}) })
	addObserver(t, fe, "db::keys", observerInfo("orders", "x::y", `{"table":"orders"}`, ""))
	if idx := keyIndexOf(c, "db::keys"); idx == nil || idx.filed != 1 || len(c.Observers("db::keys")) != 1 {
		t.Fatal("the rejected observer is active or filed")
	}

	for _, payload := range []string{`{}`, `{"op":"insert"}`} {
		if _, err := c.Call(context.Background(), CallRequest{FunctionID: "db::keys", Payload: json.RawMessage(payload)}); err != nil {
			t.Fatal(err)
		}
	}
	if c.IsObservedFor("db::keys", &Call{Input: json.RawMessage(`{}`)}) {
		t.Error("IsObservedFor true for a call whose keys panic")
	}
	if _, err := c.Call(context.Background(), CallRequest{FunctionID: "db::keys", Payload: json.RawMessage(`{"table":"orders"}`)}); err != nil {
		t.Fatal(err)
	}
	if items := fe.waitItems(1); len(items) != 1 || !slices.Equal(items[0].Matched, []string{"orders"}) {
		t.Errorf("items %+v, want the orders call only", items)
	}
	if n := logs.count(`OnObserverMatch of "db::keys" panicked`); n != 1 {
		t.Errorf("the panic logged %d times, want once", n)
	}

	registerHost(t, c, "db::rebuild", Identity)
	addObserver(t, fe, "db::rebuild", observerInfo("keyed", "x::y", `{"table":"users"}`, ""))
	addObserver(t, fe, "db::rebuild", observerInfo("bare", "x::y", "", ""))
	c.OnObserverMatchFor("db::rebuild", tableKeys)
	late := fe.waitFrames(MsgObserverAck, 1, func(d *DecodedMessage) bool {
		return d.ObserverAck.FunctionID == "db::rebuild" && d.ObserverAck.Error != nil
	})[0].ObserverAck
	if late.ID != "bare" || late.Error.Code != "OBSERVE_REJECTED" {
		t.Errorf("late rejection %+v", late)
	}
	waitUntil(t, "the index rebuilt", func() bool { idx := keyIndexOf(c, "db::rebuild"); return idx != nil && idx.filed == 1 })
	if obs := c.Observers("db::rebuild"); len(obs) != 1 || obs[0].ID != "keyed" {
		t.Errorf("active %v, want [keyed]", obs)
	}
}

// The deliver hook receives the observers the match hook matched, as they were when the
// call started, and its value is the caller's answer. iii delivers nothing for the
// function: no report item for a local call, no observed on a routed answer.
func TestDeliverHookReceivesTheMatchedObservers(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	pathOf := func(data json.RawMessage) string {
		var req struct{ Path string }
		_ = json.Unmarshal(data, &req)
		return req.Path
	}
	got := make(chan []string, 2)
	c.OnObserverMatchFor("route::fn", MatchFunc(func(obs *Observer, call *Call) bool { return obs.Args["path"] == pathOf(call.Input) }))
	c.OnObserverDeliverFor("route::fn", func(_ context.Context, _ *Call, observers []*Observer) (any, error) {
		var ids []string
		for _, obs := range observers {
			ids = append(ids, obs.ID)
		}
		got <- ids
		return map[string]int{"routes": len(observers)}, nil
	})
	registerHost(t, c, "route::fn", Identity)
	addObserver(t, fe, "route::fn", observerInfo("a", "x::y", `{"path":"/a"}`, ""))
	addObserver(t, fe, "route::fn", observerInfo("b", "x::y", `{"path":"/b"}`, ""))
	payload := json.RawMessage(`{"path":"/a"}`)
	if out, err := c.Call(context.Background(), CallRequest{FunctionID: "route::fn", Payload: payload}); err != nil || string(out) != `{"routes":1}` {
		t.Fatalf("local: %s %v", out, err)
	}
	if ids := <-got; !slices.Equal(ids, []string{"a"}) {
		t.Errorf("local: the hook got %v, want [a]", ids)
	}
	res := fe.waitResult(fe.invoke(InvokeFunctionMessage{FunctionID: "route::fn", Data: payload}))
	if string(res.Result) != `{"routes":1}` || len(res.Observed) != 0 {
		t.Errorf("routed answer %+v", res)
	}
	if ids := <-got; !slices.Equal(ids, []string{"a"}) {
		t.Errorf("routed: the hook got %v, want [a]", ids)
	}
	time.Sleep(30 * time.Millisecond)
	if n := len(fe.frames(MsgInvocationsObserved)); n != 0 {
		t.Errorf("%d report frames for a function that delivers its calls itself", n)
	}
}

// The observers a call is delivered to are those matched in the call state it started
// with: an observer filed under the call's key while the handler runs is not given to
// the deliver hook, and one removed meanwhile still is.
func TestDeliverHookGetsTheObserversOfTheCallStart(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	entered, release := make(chan struct{}, 1), make(chan struct{})
	got := make(chan []string, 1)
	c.OnObserverMatchFor("db::snap", rowKeys())
	c.OnObserverDeliverFor("db::snap", func(_ context.Context, call *Call, observers []*Observer) (any, error) {
		var ids []string
		for _, obs := range observers {
			ids = append(ids, obs.ID)
		}
		got <- ids
		return call.Output, nil
	})
	registerHost(t, c, "db::snap", func(_ context.Context, data json.RawMessage) (any, error) {
		entered <- struct{}{}
		<-release
		return data, nil
	})
	orders := `{"db":"primary","table":"orders"}`
	addObserver(t, fe, "db::snap", observerInfo("staying", "x::y", orders, ""))
	addObserver(t, fe, "db::snap", observerInfo("leaving", "x::y", orders, ""))
	done := make(chan error, 1)
	go func() {
		_, err := c.Call(context.Background(), CallRequest{FunctionID: "db::snap", Payload: rowChange("orders", "insert")})
		done <- err
	}()
	<-entered
	addObserver(t, fe, "db::snap", observerInfo("joining", "x::y", orders, ""))
	fe.remove("db::snap", "leaving")
	waitUntil(t, "leaving removed", func() bool { return len(c.Observers("db::snap")) == 2 })
	close(release)
	if ids := <-got; !slices.Equal(ids, []string{"staying", "leaving"}) {
		t.Errorf("the hook got %v, want [staying leaving]", ids)
	}
	if err := <-done; err != nil {
		t.Fatal(err)
	}
}

// Matched by keys, a call matches only the observers active when it started: one
// accepted while the handler runs is not delivered the call (decision 20).
func TestKeyMatchingUsesTheObserversOfTheCallStart(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	entered, release := make(chan struct{}, 1), make(chan struct{})
	c.OnObserverMatchFor("db::start", rowKeys())
	registerHost(t, c, "db::start", func(_ context.Context, data json.RawMessage) (any, error) {
		entered <- struct{}{}
		<-release
		return data, nil
	})
	orders := `{"db":"primary","table":"orders"}`
	addObserver(t, fe, "db::start", observerInfo("before", "x::y", orders, ""))
	id := fe.invoke(InvokeFunctionMessage{FunctionID: "db::start", Data: rowChange("orders", "insert")})
	<-entered
	addObserver(t, fe, "db::start", observerInfo("during", "x::y", orders, ""))
	close(release)
	if got := fe.waitResult(id).Observed; !slices.Equal(got, []string{"before"}) {
		t.Errorf("observed %v, want [before]", got)
	}
	res := fe.waitResult(fe.invoke(InvokeFunctionMessage{FunctionID: "db::start", Data: rowChange("orders", "insert")}))
	if !slices.Equal(res.Observed, []string{"before", "during"}) {
		t.Errorf("the next call observed %v, want [before during]", res.Observed)
	}
}

// Filing and unfiling make a new key index and leave the one a call holds as it was.
func TestKeyIndexIsCopyOnWrite(t *testing.T) {
	a, b := &Observer{ID: "a"}, &Observer{ID: "b"}
	ids := func(list []*Observer) []string {
		var out []string
		for _, obs := range list {
			out = append(out, obs.ID)
		}
		return out
	}
	empty := &keyIndex{}
	one := empty.with(a, []string{"orders|insert", "orders|*"})
	two := one.with(b, []string{"orders|*", "users|*"})
	back := two.without(a, []string{"orders|insert", "orders|*"})
	for _, tc := range []struct {
		name  string
		index *keyIndex
		keys  []string
		want  []string
		filed int
	}{
		{"empty", empty, []string{"orders|*"}, nil, 0},
		{"one", one, []string{"orders|insert", "orders|*", "users|*"}, []string{"a"}, 1},
		{"two", two, []string{"users|*", "orders|insert", "orders|*"}, []string{"b", "a"}, 2},
		{"back", back, []string{"orders|insert", "orders|*", "users|*"}, []string{"b"}, 1},
	} {
		if got := ids(tc.index.lookup(tc.keys)); !slices.Equal(got, tc.want) || tc.index.filed != tc.filed {
			t.Errorf("%s: lookup %v (want %v), %d filed (want %d)", tc.name, got, tc.want, tc.index.filed, tc.filed)
		}
	}
	if back.has([]string{"orders|insert"}) || !back.has([]string{"nothing", "users|*"}) {
		t.Error("has")
	}
	if back.without(a, []string{"orders|insert"}) != back || back.with(a, nil) != back {
		t.Error("unfiling an observer that is not filed, or filing one without keys, changed the index")
	}
}

// What the deliver hook returns is what the caller gets back, locally and routed: a
// value of its own, an error with its code, a panic as an error.
func TestDeliverHookAnswersTheCaller(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	c.OnObserverDeliverFor("route::fn", func(_ context.Context, call *Call, _ []*Observer) (any, error) {
		var req struct{ Path string }
		_ = json.Unmarshal(call.Input, &req)
		switch req.Path {
		case "/missing":
			return map[string]int{"status_code": 404}, nil
		case "/denied":
			return nil, &InvocationError{Code: "FORBIDDEN", Message: "denied"}
		case "/boom":
			panic("hook bug")
		}
		return call.Output, nil
	})
	registerHost(t, c, "route::fn", Identity)
	ctx := context.Background()
	for _, routed := range []bool{false, true} {
		answer := func(path string) (json.RawMessage, error) {
			payload, _ := json.Marshal(map[string]string{"path": path})
			if !routed {
				return c.Call(ctx, CallRequest{FunctionID: "route::fn", Payload: payload})
			}
			res := fe.waitResult(fe.invoke(InvokeFunctionMessage{FunctionID: "route::fn", Data: payload}))
			if res.Error != nil {
				return nil, newInvocationError(res.Error, res.FunctionID)
			}
			return res.Result, nil
		}
		if out, err := answer("/missing"); err != nil || string(out) != `{"status_code":404}` {
			t.Errorf("routed %v: /missing %s %v", routed, out, err)
		}
		var ie *InvocationError
		if _, err := answer("/denied"); !errors.As(err, &ie) || ie.Code != "FORBIDDEN" {
			t.Errorf("routed %v: /denied %v", routed, err)
		}
		if _, err := answer("/boom"); !errors.As(err, &ie) || ie.Code != "invocation_failed" {
			t.Errorf("routed %v: /boom %v", routed, err)
		}
		if out, err := answer("/ok"); err != nil || string(out) != `{"path":"/ok"}` {
			t.Errorf("routed %v: /ok %s %v", routed, out, err)
		}
	}
}

// A routed call delivered by iii: the answer lists the matched observers in observed
// (invocationresult-observed.json), and no report frame is sent.
func TestRoutedCallListsObservedOnTheAnswer(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	registerHost(t, c, "orders::create", func(context.Context, json.RawMessage) (any, error) {
		return json.RawMessage(`{"order_id":"ord_1002","total":1250}`), nil
	})
	addObserver(t, fe, "orders::create", observerInfo("8b2d4f6a-0c1e-4a3b-9d5f-7a9c1e3b5d70", "x::y", "", ""))
	addObserver(t, fe, "orders::create", observerInfo("9c3e5a7b-1d2f-4b4c-8e6a-8b0d2f4a6c81", "x::y", "", ""))
	var fixture InvocationResultMessage
	if err := json.Unmarshal(observeFixture(t, "invocationresult-observed.json"), &fixture); err != nil {
		t.Fatal(err)
	}
	mark := fe.mark()
	fe.invoke(InvokeFunctionMessage{
		InvocationID: &fixture.InvocationID,
		FunctionID:   "orders::create",
		Data:         json.RawMessage(`{"sku":"A-1","qty":2,"_caller_worker_id":"2b8d4f6a-1c3e-4a5b-9d7f-0e2c4a6b8d1f"}`),
		Traceparent:  fixture.Traceparent,
		Caller:       &Caller{WorkerID: "2b8d4f6a-1c3e-4a5b-9d7f-0e2c4a6b8d1f", WorkerName: "checkout", FunctionID: "checkout::confirm", InvocationID: "5e7a9c1b-2d4f-4b6a-8c0e-1f3a5c7e9b20"},
	})
	got, _ := MarshalMessage(fe.waitResult(fixture.InvocationID))
	assertJSONEqual(t, "invocationresult-observed.json", got, observeFixture(t, "invocationresult-observed.json"))
	time.Sleep(20 * time.Millisecond)
	if n := fe.countFrames(mark, MsgInvocationResult); n != 0 {
		t.Errorf("%d frames besides the answer", n)
	}
}

// A routed call whose handler failed: iii's delivery puts the on: error observer in the
// error answer's observed (invocationresult-observed-error.json). A deliver hook that
// rethrows the error answers with it and lists nothing, as does one that turns the error
// into a result; neither sends a report item.
func TestRoutedErrorAnswerCarriesObserved(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	failing := func(context.Context, json.RawMessage) (any, error) {
		return nil, &InvocationError{Code: "CARD_DECLINED", Message: "card declined by issuer"}
	}
	registerHost(t, c, "payments::charge", failing)
	c.OnObserverDeliverFor("payments::refund", func(_ context.Context, call *Call, _ []*Observer) (any, error) {
		return nil, newInvocationError(call.Error, call.FunctionID)
	})
	registerHost(t, c, "payments::refund", failing)
	c.OnObserverDeliverFor("payments::soft", func(_ context.Context, call *Call, _ []*Observer) (any, error) {
		return map[string]string{"declined": call.Error.Message}, nil
	})
	registerHost(t, c, "payments::soft", failing)
	onError := ObserverInfo{ID: "c81e2f4a-5b6c-4d7e-9f0a-1b2c3d4e5f60", Target: "alerts::payment-failed", Filters: json.RawMessage(`{}`), Args: json.RawMessage(`{}`),
		Capture: []Capture{CaptureInput, CaptureError, CaptureCaller}, On: OnError}
	for _, fn := range []string{"payments::charge", "payments::refund", "payments::soft"} {
		addObserver(t, fe, fn, onError)
	}
	var fixture InvocationResultMessage
	_ = json.Unmarshal(observeFixture(t, "invocationresult-observed-error.json"), &fixture)
	fe.invoke(InvokeFunctionMessage{InvocationID: &fixture.InvocationID, FunctionID: "payments::charge", Traceparent: fixture.Traceparent})
	got, _ := MarshalMessage(fe.waitResult(fixture.InvocationID))
	assertJSONEqual(t, "invocationresult-observed-error.json", got, observeFixture(t, "invocationresult-observed-error.json"))

	if res := fe.waitResult(fe.invoke(InvokeFunctionMessage{FunctionID: "payments::refund"})); res.Error == nil || res.Error.Code != "CARD_DECLINED" || len(res.Observed) != 0 {
		t.Errorf("rethrown by the hook: %+v", res)
	}
	if res := fe.waitResult(fe.invoke(InvokeFunctionMessage{FunctionID: "payments::soft"})); res.Error != nil || string(res.Result) != `{"declined":"card declined by issuer"}` || len(res.Observed) != 0 {
		t.Errorf("turned into a result by the hook: %+v", res)
	}
	time.Sleep(20 * time.Millisecond)
	if n := len(fe.frames(MsgInvocationsObserved)); n != 0 {
		t.Errorf("%d report frames for routed calls", n)
	}
}

// A routed call of a function with a deliver hook is answered with the hook's value, its
// trace context echoed and no observed, and nothing else is sent for it.
func TestRoutedDeliverHookAnswer(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	c.OnObserverDeliverFor("orders::submit", func(_ context.Context, call *Call, _ []*Observer) (any, error) {
		var placed struct {
			OrderID string `json:"order_id"`
		}
		if err := json.Unmarshal(call.Output, &placed); err != nil {
			return nil, err
		}
		return map[string]string{"order_id": placed.OrderID, "status": "accepted"}, nil
	})
	registerHost(t, c, "orders::submit", func(context.Context, json.RawMessage) (any, error) {
		return json.RawMessage(`{"order_id":"ord_1004","sku":"A-1","qty":2,"total":1250,"warehouse":"ams-2"}`), nil
	})
	addObserver(t, fe, "orders::submit", observerInfo("c6ccda45-4a35-461a-a9c4-a318eec7c36a", "x::y", "", ""))
	tp := "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"
	mark := fe.mark()
	res := fe.waitResult(fe.invoke(InvokeFunctionMessage{
		FunctionID:  "orders::submit",
		Data:        json.RawMessage(`{"sku":"A-1","qty":2,"_caller_worker_id":"2b8d4f6a-1c3e-4a5b-9d7f-0e2c4a6b8d1f"}`),
		Traceparent: &tp,
		Caller:      &Caller{WorkerID: "2b8d4f6a-1c3e-4a5b-9d7f-0e2c4a6b8d1f", WorkerName: "checkout", FunctionID: "checkout::confirm", InvocationID: "5e7a9c1b-2d4f-4b6a-8c0e-1f3a5c7e9b20"},
	}))
	if string(res.Result) != `{"order_id":"ord_1004","status":"accepted"}` || res.Error != nil || res.Observed != nil ||
		res.Traceparent == nil || *res.Traceparent != tp {
		t.Errorf("answer %+v", res)
	}
	time.Sleep(20 * time.Millisecond)
	if n := fe.countFrames(mark, MsgInvocationResult); n != 0 {
		t.Errorf("%d frames besides the answer", n)
	}
}

// Against an engine without observe.v2 the engine delivers routed calls: the answer
// carries no observed and nothing is reported for them.
func TestRoutedCallsAreNotDeliveredWithoutObserveV2(t *testing.T) {
	fe := newFakeEngine(t).v1Engine()
	c := fe.client()
	registerHost(t, c, "v1::fn", echoHandler)
	addObserver(t, fe, "v1::fn", observerInfo("o1", "x::y", "", ""))
	mark := fe.mark()
	res := fe.waitResult(fe.invoke(InvokeFunctionMessage{FunctionID: "v1::fn", Data: json.RawMessage(`{"a":1}`)}))
	if len(res.Observed) != 0 || string(res.Result) != `{"a":1}` {
		t.Errorf("answer %+v", res)
	}
	time.Sleep(20 * time.Millisecond)
	if n := fe.countFrames(mark, MsgInvocationResult); n != 0 {
		t.Errorf("%d frames besides the answer", n)
	}
}

// The deliver hook runs inside the function's invocation: a call it makes has that
// invocation as its caller.
func TestDeliverHookRunsInTheFunctionsInvocation(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	seen := make(chan Invocation, 1)
	c.OnObserverDeliverFor("ctx::fn", func(ctx context.Context, call *Call, _ []*Observer) (any, error) {
		inv, _ := InvocationFromContext(ctx)
		seen <- inv
		_, _ = c.Call(ctx, CallRequest{FunctionID: "audit::log", Action: VoidAction()})
		return call.Output, nil
	})
	registerHost(t, c, "ctx::fn", echoHandler)
	id := fe.invoke(InvokeFunctionMessage{FunctionID: "ctx::fn"})
	fe.waitResult(id)
	if inv := <-seen; inv.FunctionID != "ctx::fn" || inv.InvocationID != id.String() {
		t.Errorf("invocation %+v", inv)
	}
	sent := fe.waitFrames(MsgInvokeFunction, 1, func(d *DecodedMessage) bool { return d.InvokeFunction.FunctionID == "audit::log" })[0].InvokeFunction
	if sent.Caller == nil || sent.Caller.FunctionID != "ctx::fn" || sent.Caller.InvocationID != id.String() {
		t.Errorf("caller %+v", sent.Caller)
	}
}

// A call through an observer: an invokefunction to the observer's target with
// activation.observer_id, no namespace, the hook's invocation as caller; the target's
// result is the caller's. An engine refusal comes back with its code. A request that
// names a function or a namespace, or enqueues, is refused before anything is sent.
func TestCallThroughAnObserver(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client(WithNamespace("acme"))
	c.OnObserverDeliverFor("http::on-request", func(ctx context.Context, call *Call, observers []*Observer) (any, error) {
		for _, obs := range observers {
			if obs.Matches(call) {
				return c.Call(ctx, CallRequest{Observer: obs, Payload: call.Input})
			}
		}
		return map[string]int{"status_code": 404}, nil
	})
	registerHost(t, c, "http::on-request", Identity, RegisterFunctionOptions{Private: true})
	addObserver(t, fe, "http::on-request", observerInfo("route", "users::get", `{"method":"GET","path":"/users/:id"}`, `{"$and":[{"input.headers.x-tenant":{"$eq":"acme"}}]}`))
	fe.setOnFrame(func(msg map[string]json.RawMessage) {
		if messageType(msg) != string(MsgInvokeFunction) {
			return
		}
		inv := decodeRecorded(t, msg).InvokeFunction
		if inv.Activation == nil || inv.InvocationID == nil {
			return
		}
		res := &InvocationResultMessage{InvocationID: *inv.InvocationID, FunctionID: inv.FunctionID, Result: json.RawMessage(`{"status_code":200}`)}
		if string(inv.Data) == `{"headers":{"x-tenant":"acme"},"fail":true}` {
			res = &InvocationResultMessage{InvocationID: *inv.InvocationID, FunctionID: inv.FunctionID, Error: &ErrorBody{Code: "OVERFLOW", Message: "max_in_flight"}}
		}
		fe.send(res)
	})
	call := func(data string) *InvocationResultMessage {
		return fe.waitResult(fe.invoke(InvokeFunctionMessage{FunctionID: "http::on-request", Data: json.RawMessage(data),
			Caller: &Caller{WorkerID: "w", WorkerName: "edge", FunctionID: "edge::serve", InvocationID: "i0"}}))
	}
	id := fe.invoke(InvokeFunctionMessage{FunctionID: "http::on-request", Data: json.RawMessage(`{"headers":{"x-tenant":"acme"}}`)})
	if res := fe.waitResult(id); string(res.Result) != `{"status_code":200}` || res.Error != nil {
		t.Fatalf("routed through the observer: %+v", res)
	}
	sent := fe.waitFrames(MsgInvokeFunction, 1, func(d *DecodedMessage) bool { return d.InvokeFunction.Activation != nil })[0].InvokeFunction
	if sent.FunctionID != "users::get" || sent.Activation.ObserverID != "route" || sent.Activation.ActivationID != "" || sent.Namespace != "" {
		t.Errorf("frame %+v (the observer's target, no namespace)", sent)
	}
	if sent.Caller == nil || sent.Caller.FunctionID != "http::on-request" || sent.Caller.InvocationID != id.String() {
		t.Errorf("caller %+v, want the hook's invocation", sent.Caller)
	}
	if res := call(`{"headers":{"x-tenant":"other"}}`); string(res.Result) != `{"status_code":404}` {
		t.Errorf("a call the filters reject: %+v", res)
	}
	if res := call(`{"headers":{"x-tenant":"acme"},"fail":true}`); res.Error == nil || res.Error.Code != "OVERFLOW" {
		t.Errorf("an engine refusal: %+v", res)
	}

	obs := c.Observers("http::on-request")[0]
	ctx := context.Background()
	for name, req := range map[string]CallRequest{
		"function id": {Observer: obs, FunctionID: "users::get"},
		"namespace":   {Observer: obs, Namespace: "acme"},
		"enqueue":     {Observer: obs, Action: EnqueueAction("q")},
		"foreign":     {Observer: &Observer{ID: "route", Target: "users::get"}},
	} {
		if _, err := c.Call(ctx, req); err == nil {
			t.Errorf("%s: accepted", name)
		}
	}
}

// Reject takes the observer out of delivery at once and runs its removed hooks; the
// engine's observerremoved that follows runs none again.
func TestRejectStopsDeliveryAtOnce(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	var l hookLog
	recordHooks(c, "rej::fn", &l)
	registerHost(t, c, "rej::fn", echoHandler)
	addObserver(t, fe, "rej::fn", observerInfo("o1", "x::y", "", ""))
	obs := c.Observers("rej::fn")[0]
	obs.Reject("UNKNOWN_DB", "db removed by a config reload")
	ack := fe.waitFrames(MsgObserverAck, 1, func(d *DecodedMessage) bool { return d.ObserverAck.Error != nil })[0].ObserverAck
	if ack.ID != "o1" || ack.Error.Code != "UNKNOWN_DB" || ack.Revision != 1 {
		t.Errorf("late rejection %+v", ack)
	}
	if c.IsObserved("rej::fn") {
		t.Error("observed after Reject")
	}
	if _, err := c.Call(context.Background(), CallRequest{FunctionID: "rej::fn"}); err != nil {
		t.Fatal(err)
	}
	time.Sleep(30 * time.Millisecond)
	if n := len(fe.reportItems()); n != 0 {
		t.Errorf("%d items delivered after Reject", n)
	}
	waitUntil(t, "the removed hook", func() bool { _, r := l.snapshot(); return slices.Equal(r, []string{"o1"}) })
	fe.remove("rej::fn", "o1")
	// the queue runs in order: once o2 is acked, the removal of o1 was applied
	addObserver(t, fe, "rej::fn", observerInfo("o2", "x::y", "", ""))
	if _, removed := l.snapshot(); !slices.Equal(removed, []string{"o1"}) {
		t.Errorf("removed %v, want o1 once", removed)
	}
}

func TestIsObservedAndObserversFollowTheActiveSet(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	ref := registerHost(t, c, "cron::on-tick", Identity)
	if c.IsObserved("cron::on-tick") || ref.IsObserved() || len(c.Observers("cron::on-tick")) != 0 || c.IsObserved("unknown::fn") || c.Observers("unknown::fn") != nil {
		t.Fatal("observed before any observer")
	}
	if c.IsObservedFor("unknown::fn", &Call{}) || ref.IsObservedFor(&Call{}) {
		t.Fatal("IsObservedFor before any observer")
	}
	addObserver(t, fe, "cron::on-tick", observerInfo("o1", "x::y", `{"expression":"x"}`, ""))
	addObserver(t, fe, "cron::on-tick", observerInfo("o2", "x::y", `{"expression":"y"}`, ""))
	list := ref.Observers()
	if !c.IsObserved("cron::on-tick") || !ref.IsObservedFor(nil) || len(list) != 2 || list[0].ID != "o1" {
		t.Errorf("observers %v", list)
	}
	fe.remove("cron::on-tick", "o1")
	waitUntil(t, "one left", func() bool { return len(c.Observers("cron::on-tick")) == 1 })
	var args struct{ Expression string }
	if err := c.Observers("cron::on-tick")[0].DecodeArgs(&args); err != nil || args.Expression != "y" {
		t.Errorf("DecodeArgs: %+v %v", args, err)
	}
}

// Observer.Matches applies on and the filters to a call, a partial one included: an
// empty Status reads as error when Error is set.
func TestObserverMatchesACall(t *testing.T) {
	f := newFunctionObservers(New("ws://unused.invalid"), "pay::fn", true, false)
	onError, err := newObserver(f, ObserverInfo{ID: "e", Target: "x", On: OnError, Filters: json.RawMessage(`{"$and":[{"input.amount":{"$gte":500}}]}`)}, 1)
	if err != nil {
		t.Fatal(err)
	}
	big := json.RawMessage(`{"amount":750}`)
	failure := &ErrorBody{Code: "CARD_DECLINED", Message: "declined"}
	for _, tc := range []struct {
		call *Call
		want bool
	}{
		{&Call{Input: big, Error: failure}, true},
		{&Call{Input: big, Status: "error", Error: failure}, true},
		{&Call{Input: big, Status: "success", Output: json.RawMessage(`{}`)}, false},
		{&Call{Input: json.RawMessage(`{"amount":5}`), Error: failure}, false},
		{&Call{Error: failure}, false},
		{nil, false},
	} {
		if got := onError.Matches(tc.call); got != tc.want {
			t.Errorf("Matches(%+v) = %v, want %v", tc.call, got, tc.want)
		}
	}
	if (&Observer{}).Matches(&Call{}) {
		t.Error("an observer the SDK did not hand out matches")
	}
}

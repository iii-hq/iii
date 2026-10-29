package iii

import (
	"context"
	"encoding/json"
	"errors"
	"slices"
	"testing"
	"time"

	"github.com/google/uuid"
)

// addObserver attaches an observer to fn and waits until the SDK accepted it.
func addObserver(t *testing.T, fe *fakeEngine, fn string, info ObserverInfo) {
	t.Helper()
	rev := fe.add(fn, info)
	if a := fe.waitAck(fn, info.ID, rev); a.Error != nil {
		t.Fatalf("%s rejected: %+v", info.ID, a.Error)
	}
}

// deliverAll is a deliver hook that hands every call to next with every observer.
func deliverAll(ctx context.Context, call *Call, observers []*Observer, next Next) (any, error) {
	return next(ctx, call, observers)
}

// rowKeys files a row-change observer of args { db, table?, ops? } and delivers a
// change { db, table, op }, with "*" for every table or operation.
func rowKeys() DeliverHook {
	rowKey := func(db, table, op string) string { return db + "|" + table + "|" + op }
	return Keyed(
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

// A deliver hook chooses observers by their args and passes them to next; the caller
// gets what the hook returns (here next's: the handler's output). The handler returns
// its input, so the report item carries it once, with output_is_input.
func TestDeliverHookChoosesObserversWithNext(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	c.OnObserverDeliverFor("cron::on-tick", func(ctx context.Context, call *Call, observers []*Observer, next Next) (any, error) {
		var tick struct {
			Expression string `json:"expression"`
		}
		if err := json.Unmarshal(call.Input, &tick); err != nil {
			return nil, err
		}
		var chosen []*Observer
		for _, o := range observers {
			if o.Args["expression"] == tick.Expression {
				chosen = append(chosen, o)
			}
		}
		return next(ctx, call, chosen)
	})
	registerHost(t, c, "cron::on-tick", Identity, RegisterFunctionOptions{Private: true})
	addObserver(t, fe, "cron::on-tick", observerInfo("daily", "x::y", `{"expression":"0 0 * * *"}`, ""))
	addObserver(t, fe, "cron::on-tick", observerInfo("hourly", "x::y", `{"expression":"0 * * * *"}`, ""))
	tick := json.RawMessage(`{"expression":"0 * * * *","scheduled_time":"2026-11-01T10:00:00Z"}`)
	out, err := c.Call(context.Background(), CallRequest{FunctionID: "cron::on-tick", Payload: tick})
	if err != nil || string(out) != string(tick) {
		t.Fatalf("Call: %s %v", out, err)
	}
	item := fe.waitItems(1)[0]
	if !slices.Equal(item.Matched, []string{"hourly"}) || !item.OutputIsInput || item.Output != nil || string(item.Input) != string(tick) {
		t.Errorf("item %+v", item)
	}
	time.Sleep(20 * time.Millisecond)
	if n := len(fe.reportItems()); n != 1 {
		t.Errorf("%d items, want 1", n)
	}
}

// Keyed files each accepted observer under its keys, unfiles it when it is removed or
// rejected, and delivers a call to the observers filed under its keys, each once, in
// key order. IsObservedFor is one lookup per call key.
func TestKeyedIndexUnderAddAndRemove(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	c.OnObserverDeliverFor("database::on-row-changed", rowKeys())
	ref := registerHost(t, c, "database::on-row-changed", Identity, RegisterFunctionOptions{Private: true})
	addObserver(t, fe, "database::on-row-changed", observerInfo("orders-insert", "x::y", `{"db":"primary","table":"orders","ops":["insert"]}`, ""))
	addObserver(t, fe, "database::on-row-changed", observerInfo("all", "x::y", `{"db":"primary"}`, ""))
	addObserver(t, fe, "database::on-row-changed", observerInfo("users", "x::y", `{"db":"primary","table":"users"}`, ""))

	f := c.functionObservers("database::on-row-changed")
	idx := f.keyed.Load()
	if idx == nil {
		t.Fatal("no Keyed index")
	}
	idx.mu.RLock()
	filed := len(idx.filed)
	idx.mu.RUnlock()
	if filed != 3 {
		t.Errorf("%d observers filed, want 3", filed)
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
	if !observed("users", "delete") || !observed("orders", "insert") {
		t.Error("removing one observer unfiled another")
	}
	for _, obs := range c.Observers("database::on-row-changed") {
		if obs.ID == "orders-insert" {
			obs.Reject("UNKNOWN_TABLE", "orders was dropped")
		}
	}
	if observed("orders", "insert") {
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

// A hook made by Keyed is recognized and indexed; any other hook is not. A Keyed hook
// wrapped in another hook still delivers by its keys (scanning the observers), and
// IsObservedFor is then IsObserved.
func TestKeyedHookWrappedStillDelivers(t *testing.T) {
	if asKeyed(rowKeys()) == nil || asKeyed(deliverAll) != nil || asKeyed(nil) != nil {
		t.Fatal("asKeyed")
	}
	fe := newFakeEngine(t)
	c := fe.client()
	keyed := rowKeys()
	c.OnObserverDeliverFor("db::wrapped", func(ctx context.Context, call *Call, observers []*Observer, next Next) (any, error) {
		return keyed(ctx, call, observers, next)
	})
	registerHost(t, c, "db::wrapped", Identity)
	addObserver(t, fe, "db::wrapped", observerInfo("users", "x::y", `{"db":"primary","table":"users"}`, ""))
	addObserver(t, fe, "db::wrapped", observerInfo("orders", "x::y", `{"db":"primary","table":"orders"}`, ""))
	if c.functionObservers("db::wrapped").keyed.Load() != nil {
		t.Error("a wrapped hook was indexed")
	}
	if !c.IsObservedFor("db::wrapped", &Call{Input: rowChange("products", "insert")}) {
		t.Error("IsObservedFor of an unindexed function is IsObserved")
	}
	if _, err := c.Call(context.Background(), CallRequest{FunctionID: "db::wrapped", Payload: rowChange("orders", "update")}); err != nil {
		t.Fatal(err)
	}
	if item := fe.waitItems(1)[0]; !slices.Equal(item.Matched, []string{"orders"}) {
		t.Errorf("matched %v, want [orders]", item.Matched)
	}
}

// What the deliver hook returns is what the caller gets back, locally and routed: a
// value of its own, an error with its code, a panic as an error.
func TestDeliverHookAnswersTheCaller(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	c.OnObserverDeliverFor("route::fn", func(ctx context.Context, call *Call, observers []*Observer, next Next) (any, error) {
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
		return next(ctx, call, observers)
	})
	registerHost(t, c, "route::fn", Identity)
	ctx := context.Background()
	for _, routed := range []bool{false, true} {
		dispatch := DispatchAuto
		if routed {
			dispatch = DispatchRemote
		}
		answer := func(path string) (json.RawMessage, error) {
			payload, _ := json.Marshal(map[string]string{"path": path})
			if !routed {
				return c.Call(ctx, CallRequest{FunctionID: "route::fn", Payload: payload, Dispatch: dispatch})
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

// Each observer is delivered at most once per call, however many times next runs.
func TestNextDeliversEachObserverOncePerCall(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	c.OnObserverDeliverFor("twice::fn", func(ctx context.Context, call *Call, observers []*Observer, next Next) (any, error) {
		_, _ = next(ctx, call, observers)
		return next(ctx, call, append(observers, observers...))
	})
	registerHost(t, c, "twice::fn", echoHandler)
	addObserver(t, fe, "twice::fn", observerInfo("o1", "x::y", "", ""))
	addObserver(t, fe, "twice::fn", observerInfo("o2", "x::y", "", ""))
	if _, err := c.Call(context.Background(), CallRequest{FunctionID: "twice::fn"}); err != nil {
		t.Fatal(err)
	}
	if item := fe.waitItems(1)[0]; !slices.Equal(item.Matched, []string{"o1", "o2"}) {
		t.Errorf("matched %v", item.Matched)
	}
	id := fe.invoke(InvokeFunctionMessage{FunctionID: "twice::fn"})
	if res := fe.waitResult(id); !slices.Equal(res.Observed, []string{"o1", "o2"}) {
		t.Errorf("observed %v", res.Observed)
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

// A routed call whose handler failed: the error answer carries observed for the
// on: error observer (invocationresult-observed-error.json), also when a deliver hook
// returns the handler's error unchanged. A hook that turns the error into a result
// reports the match in an item instead, ahead of the answer.
func TestRoutedErrorAnswerCarriesObserved(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	failing := func(context.Context, json.RawMessage) (any, error) {
		return nil, &InvocationError{Code: "CARD_DECLINED", Message: "card declined by issuer"}
	}
	registerHost(t, c, "payments::charge", failing)
	c.OnObserverDeliverFor("payments::refund", deliverAll)
	registerHost(t, c, "payments::refund", failing)
	c.OnObserverDeliverFor("payments::soft", func(ctx context.Context, call *Call, observers []*Observer, next Next) (any, error) {
		_, err := next(ctx, call, observers)
		return map[string]string{"declined": err.Error()}, nil
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

	if res := fe.waitResult(fe.invoke(InvokeFunctionMessage{FunctionID: "payments::refund"})); res.Error == nil || !slices.Equal(res.Observed, []string{onError.ID}) {
		t.Errorf("rethrown unchanged: %+v", res)
	}
	id := fe.invoke(InvokeFunctionMessage{FunctionID: "payments::soft"})
	res := fe.waitResult(id)
	if res.Error != nil || len(res.Observed) != 0 {
		t.Errorf("turned into a result: %+v", res)
	}
	items := fe.waitItems(1)
	if items[0].InvocationID != id.String() || items[0].Status != "error" || !slices.Equal(items[0].Matched, []string{onError.ID}) {
		t.Errorf("item %+v", items[0])
	}
	itemAt := fe.frameIndex(func(d *DecodedMessage) bool { return d.Type == MsgInvocationsObserved })
	answerAt := fe.frameIndex(func(d *DecodedMessage) bool {
		return d.Type == MsgInvocationResult && d.InvocationResult.InvocationID == id
	})
	if itemAt > answerAt {
		t.Errorf("the item (%d) must precede the answer (%d)", itemAt, answerAt)
	}
}

// A deliver hook that returns something other than the handler's output: the matches
// go in an item carrying the handler's call, sent before the answer, which has no
// observed (invocationsobserved-routed.json, then invocationresult-hook-result.json).
func TestRoutedHookResultSendsTheItemBeforeTheAnswer(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	c.OnObserverDeliverFor("orders::submit", func(ctx context.Context, call *Call, observers []*Observer, next Next) (any, error) {
		if _, err := next(ctx, call, observers); err != nil {
			return nil, err
		}
		return map[string]string{"order_id": "ord_1004", "status": "accepted"}, nil
	})
	registerHost(t, c, "orders::submit", func(context.Context, json.RawMessage) (any, error) {
		return json.RawMessage(`{"order_id":"ord_1004","sku":"A-1","qty":2,"total":1250,"warehouse":"ams-2"}`), nil
	})
	addObserver(t, fe, "orders::submit", observerInfo("c6ccda45-4a35-461a-a9c4-a318eec7c36a", "x::y", "", ""))
	var want InvocationsObservedMessage
	_ = json.Unmarshal(observeFixture(t, "invocationsobserved-routed.json"), &want)
	var answer InvocationResultMessage
	_ = json.Unmarshal(observeFixture(t, "invocationresult-hook-result.json"), &answer)
	fe.invoke(InvokeFunctionMessage{
		InvocationID: &answer.InvocationID,
		FunctionID:   "orders::submit",
		Data:         json.RawMessage(`{"sku":"A-1","qty":2,"_caller_worker_id":"2b8d4f6a-1c3e-4a5b-9d7f-0e2c4a6b8d1f"}`),
		Traceparent:  answer.Traceparent,
		Caller:       &Caller{WorkerID: "2b8d4f6a-1c3e-4a5b-9d7f-0e2c4a6b8d1f", WorkerName: "checkout", FunctionID: "checkout::confirm", InvocationID: "5e7a9c1b-2d4f-4b6a-8c0e-1f3a5c7e9b20"},
	})
	res := fe.waitResult(answer.InvocationID)
	got, _ := MarshalMessage(res)
	assertJSONEqual(t, "invocationresult-hook-result.json", got, observeFixture(t, "invocationresult-hook-result.json"))
	items := fe.waitItems(1)
	items[0].StartedAtMs, items[0].DurationMs = want.Items[0].StartedAtMs, want.Items[0].DurationMs
	gotItems, _ := MarshalMessage(&InvocationsObservedMessage{Items: items})
	assertJSONEqual(t, "invocationsobserved-routed.json", gotItems, observeFixture(t, "invocationsobserved-routed.json"))
	itemAt := fe.frameIndex(func(d *DecodedMessage) bool { return d.Type == MsgInvocationsObserved })
	answerAt := fe.frameIndex(func(d *DecodedMessage) bool { return d.Type == MsgInvocationResult })
	if itemAt < 0 || itemAt > answerAt {
		t.Errorf("the item (%d) must precede the answer (%d)", itemAt, answerAt)
	}
}

// next given the unchanged call pools into observed; each next given a changed call
// sends its own item for the same invocation, carrying the call it was given
// (invocationsobserved-routed-items.json): items and observed coexist.
func TestRoutedChangedCallsSendItemsNextToObserved(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	c.OnObserverDeliverFor("orders::import", func(ctx context.Context, call *Call, observers []*Observer, next Next) (any, error) {
		out, err := next(ctx, call, observers)
		var batch struct {
			Orders []json.RawMessage `json:"orders"`
		}
		_ = json.Unmarshal(call.Input, &batch)
		for _, order := range batch.Orders {
			each := *call
			each.Input = order
			if _, err := next(ctx, &each, observers); err != nil {
				return nil, err
			}
		}
		return out, err
	})
	registerHost(t, c, "orders::import", func(context.Context, json.RawMessage) (any, error) {
		return json.RawMessage(`{"batch_id":"b_77","imported":2}`), nil
	})
	addObserver(t, fe, "orders::import", observerInfo("every-batch", "x::y", "", ""))
	addObserver(t, fe, "orders::import", observerInfo("517d76aa-3a9a-45fe-ab4f-d2534029ed41", "x::y", "", `{"$and":[{"input.warehouse":{"$eq":"ams-2"}}]}`))
	addObserver(t, fe, "orders::import", observerInfo("49d94cda-fe89-4e71-85c4-c203ec9fedce", "x::y", "", `{"$and":[{"input.warehouse":{"$eq":"fra-1"}}]}`))
	var want InvocationsObservedMessage
	_ = json.Unmarshal(observeFixture(t, "invocationsobserved-routed-items.json"), &want)
	id := uuid.MustParse(want.Items[0].InvocationID)
	tp := *want.Items[0].Traceparent
	fe.invoke(InvokeFunctionMessage{
		InvocationID: &id,
		FunctionID:   "orders::import",
		Data: json.RawMessage(`{"orders":[{"order_id":"ord_2001","warehouse":"ams-2","sku":"A-1","qty":2},` +
			`{"order_id":"ord_2002","warehouse":"fra-1","sku":"B-7","qty":1}]}`),
		Traceparent: &tp,
		Caller:      &Caller{WorkerID: "9f1c0d2e-3b4a-4c5d-8e6f-7a8b9c0d1e2f", WorkerName: "erp", FunctionID: "erp::sync", InvocationID: "dba2523f-7f0f-4883-a840-64dc90d532ea"},
	})
	res := fe.waitResult(id)
	if !slices.Equal(res.Observed, []string{"every-batch"}) || string(res.Result) != `{"batch_id":"b_77","imported":2}` {
		t.Errorf("answer %+v", res)
	}
	items := fe.waitItems(2)
	for i := range items {
		items[i].StartedAtMs, items[i].DurationMs = want.Items[i].StartedAtMs, want.Items[i].DurationMs
	}
	got, _ := MarshalMessage(&InvocationsObservedMessage{Items: items})
	assertJSONEqual(t, "invocationsobserved-routed-items.json", got, observeFixture(t, "invocationsobserved-routed-items.json"))
	if n := len(fe.frames(MsgInvocationsObserved)); n != 1 {
		t.Errorf("%d report frames, want the two items in one", n)
	}
}

// A next on a routed call after its hook settled delivers nothing and is counted as
// LATE_DELIVERY; on a local call it is reported on its own.
func TestLateNextCountsLateDelivery(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	late := make(chan func(), 2)
	c.OnObserverDeliverFor("late::fn", func(ctx context.Context, call *Call, observers []*Observer, next Next) (any, error) {
		late <- func() { _, _ = next(ctx, call, observers) }
		return "settled", nil
	})
	registerHost(t, c, "late::fn", echoHandler)
	addObserver(t, fe, "late::fn", observerInfo("o1", "x::y", "", ""))

	res := fe.waitResult(fe.invoke(InvokeFunctionMessage{FunctionID: "late::fn"}))
	if len(res.Observed) != 0 {
		t.Errorf("observed %v", res.Observed)
	}
	(<-late)()
	c.reporter.mu.Lock()
	drops := c.reporter.drops[dropKey{"late::fn", dropLateDelivery}]
	c.reporter.mu.Unlock()
	if drops != 1 {
		t.Errorf("LATE_DELIVERY %d, want 1", drops)
	}

	if _, err := c.Call(context.Background(), CallRequest{FunctionID: "late::fn"}); err != nil {
		t.Fatal(err)
	}
	(<-late)()
	if item := fe.waitItems(1)[0]; !slices.Equal(item.Matched, []string{"o1"}) {
		t.Errorf("late local item %+v", item)
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
	c.OnObserverDeliverFor("ctx::fn", func(ctx context.Context, call *Call, observers []*Observer, next Next) (any, error) {
		inv, _ := InvocationFromContext(ctx)
		seen <- inv
		_, _ = c.Call(ctx, CallRequest{FunctionID: "audit::log", Action: VoidAction()})
		return next(ctx, call, observers)
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
	c.OnObserverDeliverFor("http::on-request", func(ctx context.Context, call *Call, observers []*Observer, next Next) (any, error) {
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

// Reject takes the observer out of delivery at once; the removed hooks run when the
// engine's observerremoved arrives.
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
	if _, removed := l.snapshot(); len(removed) != 0 {
		t.Error("removed before observerremoved")
	}
	fe.remove("rej::fn", "o1")
	waitUntil(t, "removed", func() bool { _, r := l.snapshot(); return len(r) == 1 })
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

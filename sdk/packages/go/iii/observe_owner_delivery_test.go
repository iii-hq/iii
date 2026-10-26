package iii

import (
	"context"
	"encoding/json"
	"errors"
	"sync"
	"testing"
	"time"

	"github.com/google/uuid"
)

// cronOwner registers an owner-delivered function that indexes its observers by the
// "expression" arg, like the cron worker of sdk.md §1.
type cronOwner struct {
	mu    sync.Mutex
	index map[string][]*Observer
}

func newCronOwner(t *testing.T, fe *fakeEngine, c *Client, id string) *cronOwner {
	t.Helper()
	return newCronOwnerWith(t, fe, c, id, echoHandler)
}

func newCronOwnerWith(t *testing.T, fe *fakeEngine, c *Client, id string, handler Handler) *cronOwner {
	t.Helper()
	o := &cronOwner{index: map[string][]*Observer{}}
	registerOwnerWith(t, c, id, handler, &ObserveHooks{
		Delivery: DeliveryOwner,
		OnObserverAdded: func(_ context.Context, obs *Observer) error {
			expr, _ := obs.Args["expression"].(string)
			o.mu.Lock()
			o.index[expr] = append(o.index[expr], obs)
			o.mu.Unlock()
			return nil
		},
		OnObserverRemoved: func(_ context.Context, obs *Observer) {
			expr, _ := obs.Args["expression"].(string)
			o.mu.Lock()
			defer o.mu.Unlock()
			list := o.index[expr]
			for i, x := range list {
				if x == obs {
					o.index[expr] = append(list[:i:i], list[i+1:]...)
				}
			}
		},
	})
	return o
}

func (o *cronOwner) observers(expr string) []*Observer {
	o.mu.Lock()
	defer o.mu.Unlock()
	return append([]*Observer(nil), o.index[expr]...)
}

func addCronObserver(t *testing.T, fe *fakeEngine, fn, id, expr, filters string, capture ...Capture) {
	t.Helper()
	rev := fe.add(fn, observerInfo(id, "local::target", `{"expression":"`+expr+`"}`, filters, capture...))
	if a := fe.waitAck(fn, id, rev); a.Error != nil {
		t.Fatalf("%s rejected: %+v", id, a.Error)
	}
}

func TestEmitAppliesTheObserversFilters(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	owner := newCronOwner(t, fe, c, "cron::on-tick")
	addCronObserver(t, fe, "cron::on-tick", "nov", "* * *", `{"$and":[{"input.scheduled_time":{"$gte":"2026-11-01"}}]}`)
	obs := owner.observers("* * *")[0]
	mark := fe.mark()
	ok, err := obs.Emit(context.Background(), map[string]string{"scheduled_time": "2026-10-31"})
	if err != nil || ok {
		t.Fatalf("Emit rejected by filters: %v %v", ok, err)
	}
	time.Sleep(20 * time.Millisecond)
	if n := fe.countFrames(mark); n != 0 {
		t.Fatalf("%d frames sent for an event the filters reject", n)
	}
	if ok, err := obs.Emit(context.Background(), map[string]string{"scheduled_time": "2026-11-02"}); err != nil || !ok {
		t.Fatalf("Emit: %v %v", ok, err)
	}
	items := fe.waitItems(1)
	if items[0].FunctionID != "cron::on-tick" || items[0].Status != "success" || len(items[0].Matched) != 1 || items[0].Matched[0] != "nov" {
		t.Errorf("item %+v", items[0])
	}
	if items[0].Output != nil || items[0].Error != nil || string(items[0].Input) != `{"scheduled_time":"2026-11-02"}` {
		t.Errorf("item parts: input %s output %s", items[0].Input, items[0].Output)
	}
}

// sdk.md §7 Routing: two observers with different args only receive what the owner
// emits to them.
func TestEmitReachesOnlyTheObserversTheOwnerRoutesTo(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	owner := newCronOwner(t, fe, c, "cron::on-tick")
	addCronObserver(t, fe, "cron::on-tick", "daily", "0 0 * * *", "")
	addCronObserver(t, fe, "cron::on-tick", "hourly", "0 * * * *", "")
	for _, obs := range owner.observers("0 * * * *") {
		if _, err := obs.Emit(context.Background(), map[string]string{"expression": "0 * * * *"}); err != nil {
			t.Fatal(err)
		}
	}
	items := fe.waitItems(1)
	time.Sleep(20 * time.Millisecond)
	items = fe.reportItems()
	if len(items) != 1 || len(items[0].Matched) != 1 || items[0].Matched[0] != "hourly" {
		t.Errorf("items %+v", items)
	}
}

// sdk.md §1: an event's caller is the invocation it is emitted in: inside the observed
// function's own handler, that invocation (its id and caller); inside another handler,
// that handler; outside any handler, this worker only.
func TestEmitCallerRules(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	var owner *cronOwner
	emitAll := func(ctx context.Context, from string) error {
		for _, obs := range owner.observers("x") {
			if _, err := obs.Emit(ctx, map[string]string{"from": from}); err != nil {
				return err
			}
		}
		return nil
	}
	owner = newCronOwnerWith(t, fe, c, "cron::on-tick", func(ctx context.Context, _ json.RawMessage) (any, error) {
		return nil, emitAll(ctx, "cron::on-tick")
	})
	_ = c.RegisterFunction("cron::on-tick-shadow", func(ctx context.Context, _ json.RawMessage) (any, error) {
		return nil, emitAll(ctx, "cron::on-tick-shadow")
	})
	addCronObserver(t, fe, "cron::on-tick", "all", "x", "", CaptureInput, CaptureCaller, CaptureMetadata)
	obs := owner.observers("x")[0]
	ownID := uuid.New()
	tp := "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"
	fe.send(&InvokeFunctionMessage{
		InvocationID: &ownID, FunctionID: "cron::on-tick", Data: json.RawMessage(`{}`), Metadata: json.RawMessage(`{"run":1}`),
		Traceparent: &tp,
		Caller:      &Caller{WorkerID: "w-sched", WorkerName: "scheduler", FunctionID: "sched::tick", InvocationID: "inv-sched"},
	})
	_ = fe.waitItems(1)
	otherID := uuid.New()
	fe.send(&InvokeFunctionMessage{
		InvocationID: &otherID, FunctionID: "cron::on-tick-shadow", Data: json.RawMessage(`{}`),
		Caller: &Caller{WorkerID: "w-up", WorkerName: "upstream", FunctionID: "up::fn", InvocationID: "inv-up"},
	})
	_ = fe.waitItems(2)
	if _, err := obs.Emit(context.Background(), map[string]string{"from": "nowhere"}); err != nil {
		t.Fatal(err)
	}
	items := fe.waitItems(3)

	own, other, none := items[0], items[1], items[2]
	if own.InvocationID != ownID.String() || own.Caller == nil || own.Caller.FunctionID != "sched::tick" || own.Caller.WorkerName != "scheduler" {
		t.Errorf("own handler: invocation %s caller %+v", own.InvocationID, own.Caller)
	}
	if string(own.Metadata) != `{"run":1}` || own.Traceparent == nil || *own.Traceparent != tp {
		t.Errorf("own handler: metadata %s traceparent %v", own.Metadata, own.Traceparent)
	}
	if other.InvocationID == otherID.String() || other.Caller.FunctionID != "cron::on-tick-shadow" || other.Caller.InvocationID != otherID.String() ||
		other.Caller.WorkerID != "worker-1" || other.Caller.WorkerName != "observe-test" ||
		other.Caller.Parent == nil || other.Caller.Parent.FunctionID != "up::fn" || other.Caller.Parent.Parent != nil {
		t.Errorf("another handler: invocation %s caller %+v parent %+v", other.InvocationID, other.Caller, other.Caller.Parent)
	}
	if none.Caller == nil || none.Caller.FunctionID != "" || none.Caller.WorkerName != "observe-test" || none.Caller.WorkerID != "worker-1" || none.Metadata != nil {
		t.Errorf("outside any handler: caller %+v", none.Caller)
	}
}

// sdk.md §7: emits of one event to N observers in one flush are one report item, with
// the union of their captures.
func TestEmitOfOneEventToNObserversIsOneItem(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	owner := newCronOwner(t, fe, c, "db::on-row-changed")
	addCronObserver(t, fe, "db::on-row-changed", "a", "orders", "", CaptureInput)
	addCronObserver(t, fe, "db::on-row-changed", "b", "orders", "", CaptureCaller)
	addCronObserver(t, fe, "db::on-row-changed", "c", "orders", `{"$and":[{"input.op":{"$eq":"delete"}}]}`)
	event := map[string]any{"table": "orders", "op": "insert"}
	for _, obs := range owner.observers("orders") {
		if _, err := obs.Emit(context.Background(), event); err != nil {
			t.Fatal(err)
		}
	}
	items := fe.waitItems(1)
	time.Sleep(20 * time.Millisecond)
	items = fe.reportItems()
	if len(items) != 1 {
		t.Fatalf("%d items, want 1", len(items))
	}
	it := items[0]
	if len(it.Matched) != 2 || it.Matched[0] != "a" || it.Matched[1] != "b" || it.Input == nil || it.Caller == nil || it.Output != nil {
		t.Errorf("item %+v", it)
	}
}

func TestEmitAndCallRulesOnAutoAndRemovedObservers(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	fe.setObservers("auto::fn", 1, observerInfo("auto-obs", "x::y", "", ""))
	got := make(chan *Observer, 1)
	registerOwner(t, fe, c, "auto::fn", &ObserveHooks{OnObserverAdded: func(_ context.Context, obs *Observer) error { got <- obs; return nil }})
	autoObs := <-got
	var ie *InvocationError
	if _, err := autoObs.Emit(context.Background(), 1); !errors.As(err, &ie) || ie.Code != "NOT_OWNER_DELIVERED" {
		t.Errorf("Emit on auto: %v", err)
	}
	if _, err := autoObs.Call(context.Background(), 1); !errors.As(err, &ie) || ie.Code != "NOT_OWNER_DELIVERED" {
		t.Errorf("Call on auto: %v", err)
	}

	owner := newCronOwner(t, fe, c, "cron::on-tick")
	addCronObserver(t, fe, "cron::on-tick", "o1", "x", "")
	obs := owner.observers("x")[0]
	fe.remove("cron::on-tick", "o1")
	waitUntil(t, "removed", func() bool { return len(owner.observers("x")) == 0 })
	mark := fe.mark()
	if ok, err := obs.Emit(context.Background(), 1); ok || err != nil {
		t.Errorf("Emit after removal: %v %v (want false, nil)", ok, err)
	}
	if _, err := obs.Call(context.Background(), 1); !errors.As(err, &ie) || ie.Code != "OBSERVER_NOT_ACTIVE" {
		t.Errorf("Call after removal: %v", err)
	}
	time.Sleep(20 * time.Millisecond)
	if n := fe.countFrames(mark); n != 0 {
		t.Errorf("%d frames sent", n)
	}
}

func TestRejectStopsDeliveryAtOnce(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	owner := newCronOwner(t, fe, c, "cron::on-tick")
	addCronObserver(t, fe, "cron::on-tick", "o1", "x", "")
	obs := owner.observers("x")[0]
	obs.Reject("UNKNOWN_DB", "db removed by a config reload")
	ack := fe.waitFrames(MsgObserverAck, 1, func(d *DecodedMessage) bool { return d.ObserverAck.Error != nil })[0].ObserverAck
	if ack.ID != "o1" || ack.Error.Code != "UNKNOWN_DB" || ack.Revision != 1 {
		t.Errorf("late rejection %+v", ack)
	}
	if ok, _ := obs.Emit(context.Background(), 1); ok || c.IsObserved("cron::on-tick") {
		t.Error("delivered after Reject")
	}
	// OnObserverRemoved runs when the engine's observerremoved arrives
	if len(owner.observers("x")) != 1 {
		t.Error("removed before observerremoved")
	}
	fe.remove("cron::on-tick", "o1")
	waitUntil(t, "removed", func() bool { return len(owner.observers("x")) == 0 })
}

// sdk.md §1, protocol.md §3.5: obs.Call is an invokefunction to the target with the
// activation, resolved with the target's result; FILTERS_NOT_MATCHED sends nothing; an
// engine error code surfaces.
func TestCallGoesThroughTheEngineWithTheActivation(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client(WithNamespace("acme"))
	owner := newCronOwner(t, fe, c, "http::on-request")
	rev := fe.add("http::on-request", observerInfo("route", "users::get", `{"expression":"GET /users/:id"}`, `{"$and":[{"input.headers.x-tenant":{"$eq":"acme"}}]}`))
	fe.waitAck("http::on-request", "route", rev)
	obs := owner.observers("GET /users/:id")[0]

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
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	out, err := obs.Call(ctx, map[string]any{"headers": map[string]string{"x-tenant": "acme"}})
	if err != nil || string(out) != `{"status_code":200}` {
		t.Fatalf("Call: %s %v", out, err)
	}
	sent := fe.waitFrames(MsgInvokeFunction, 1, func(d *DecodedMessage) bool { return d.InvokeFunction.Activation != nil })[0].InvokeFunction
	if sent.FunctionID != "users::get" || sent.Activation.ObserverID != "route" || sent.Namespace != "" || sent.Caller != nil {
		t.Errorf("owner call frame %+v (no namespace, no caller outside a handler)", sent)
	}
	var ie *InvocationError
	mark := fe.mark()
	if _, err := obs.Call(ctx, map[string]any{"headers": map[string]string{"x-tenant": "other"}}); !errors.As(err, &ie) || ie.Code != "FILTERS_NOT_MATCHED" {
		t.Errorf("filters: %v", err)
	}
	time.Sleep(20 * time.Millisecond)
	if n := fe.countFrames(mark); n != 0 {
		t.Errorf("%d frames sent for a payload the filters reject", n)
	}
	if _, err := obs.Call(ctx, json.RawMessage(`{"headers":{"x-tenant":"acme"},"fail":true}`)); !errors.As(err, &ie) || ie.Code != "OVERFLOW" {
		t.Errorf("engine error: %v", err)
	}
	if !obs.Matches(ObserveDoc{Input: map[string]any{"headers": map[string]string{"x-tenant": "acme"}}}) || obs.Matches(ObserveDoc{}) {
		t.Error("Matches")
	}
}

func TestIsObservedAndObserversFollowTheActiveSet(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	owner := newCronOwner(t, fe, c, "cron::on-tick")
	if c.IsObserved("cron::on-tick") || len(c.Observers("cron::on-tick")) != 0 || c.IsObserved("unknown::fn") || c.Observers("unknown::fn") != nil {
		t.Fatal("observed before any observer")
	}
	addCronObserver(t, fe, "cron::on-tick", "o1", "x", "")
	addCronObserver(t, fe, "cron::on-tick", "o2", "y", "")
	list := c.Observers("cron::on-tick")
	if !c.IsObserved("cron::on-tick") || len(list) != 2 || list[0] != owner.observers("x")[0] {
		t.Errorf("observers %v", list)
	}
	fe.remove("cron::on-tick", "o1")
	waitUntil(t, "one left", func() bool { return len(c.Observers("cron::on-tick")) == 1 })
	var args struct{ Expression string }
	if err := c.Observers("cron::on-tick")[0].DecodeArgs(&args); err != nil || args.Expression != "y" {
		t.Errorf("DecodeArgs: %+v %v", args, err)
	}
}

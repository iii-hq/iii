package iii

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"slices"
	"sort"
	"strings"
	"sync"
	"testing"
	"time"
)

// The observe wire fixtures (sdk/fixtures/protocol/observe) are the wire contract:
// every file decodes into its message and encodes back to the same JSON value, and
// what this SDK builds equals them. The two engine-only files carry the 0.25 observe
// object, which no v2 SDK sends.

var observeFixturesDir = filepath.Join("..", "..", "..", "fixtures", "protocol", "observe")

// engineOnlyFixtures carry the 0.25 observe object on registerfunction: the SDK types
// do not have it, so they are not round-tripped here.
var engineOnlyFixtures = []string{"registerfunction-v1-owner.json", "registerfunction-v1-not-observable.json"}

func observeFixture(t *testing.T, name string) []byte {
	t.Helper()
	data, err := os.ReadFile(filepath.Join(observeFixturesDir, name))
	if err != nil {
		t.Fatalf("read fixture %s: %v", name, err)
	}
	return data
}

func observeFixtureFiles(t *testing.T) []string {
	t.Helper()
	files, err := filepath.Glob(filepath.Join(observeFixturesDir, "*.json"))
	if err != nil {
		t.Fatal(err)
	}
	var names []string
	for _, f := range files {
		names = append(names, filepath.Base(f))
	}
	sort.Strings(names)
	if len(names) < 43 {
		t.Fatalf("found %d observe fixtures in %s, expected the full set", len(names), observeFixturesDir)
	}
	return names
}

// assertJSONEqual compares two JSON documents as values (key order and whitespace do
// not matter; keys, values and absent keys do).
func assertJSONEqual(t *testing.T, what string, got, want []byte) {
	t.Helper()
	if !jsonEq(t, got, want) {
		t.Errorf("%s:\n got  %s\n want %s", what, compactJSON(got), compactJSON(want))
	}
}

func compactJSON(data []byte) string {
	var v any
	if json.Unmarshal(data, &v) != nil {
		return string(data)
	}
	out, _ := json.Marshal(v)
	return string(out)
}

func TestObserveFixturesRoundTrip(t *testing.T) {
	for _, name := range observeFixtureFiles(t) {
		if slices.Contains(engineOnlyFixtures, name) {
			continue
		}
		t.Run(name, func(t *testing.T) {
			data := observeFixture(t, name)
			dec, err := UnmarshalMessage(data)
			if err != nil {
				t.Fatalf("decode: %v", err)
			}
			wantType := strings.SplitN(strings.TrimSuffix(name, ".json"), "-", 2)[0]
			if string(dec.Type) != wantType {
				t.Fatalf("type %q, want %q", dec.Type, wantType)
			}
			msg := decodedVariant(t, dec)
			out, err := MarshalMessage(msg)
			if err != nil {
				t.Fatalf("encode: %v", err)
			}
			assertJSONEqual(t, name, out, data)
		})
	}
}

// The engine-only fixtures decode as registerfunction (the observe object is not
// read), and no registerfunction this SDK builds carries an observe key.
func TestNoRegisterFunctionCarriesTheObserveObject(t *testing.T) {
	for _, name := range engineOnlyFixtures {
		dec, err := UnmarshalMessage(observeFixture(t, name))
		if err != nil || dec.RegisterFunction == nil {
			t.Fatalf("%s: %v", name, err)
		}
		out, _ := MarshalMessage(dec.RegisterFunction)
		if strings.Contains(string(out), `"observe"`) {
			t.Errorf("%s re-encoded with an observe key: %s", name, out)
		}
	}
	fe := newFakeEngine(t)
	c := fe.client()
	closed := false
	c.OnObserverDeliverFor("hooked::fn", handlerAnswer)
	for id, opts := range map[string]RegisterFunctionOptions{
		"plain::fn":   {},
		"args::fn":    {ArgsFormat: json.RawMessage(`{"type":"object"}`)},
		"private::fn": {Private: true, Standby: true},
		"closed::fn":  {Observable: &closed},
		"hooked::fn":  {},
	} {
		if _, err := c.RegisterFunction(id, echoHandler, opts); err != nil {
			t.Fatal(err)
		}
	}
	for _, d := range fe.waitFrames(MsgRegisterFunction, 5, nil) {
		out, _ := MarshalMessage(d.RegisterFunction)
		if strings.Contains(string(out), `"observe"`) || strings.Contains(string(out), "standby") {
			t.Errorf("registerfunction %s", out)
		}
	}
}

// decodedVariant returns the one populated variant of a DecodedMessage.
func decodedVariant(t *testing.T, d *DecodedMessage) any {
	t.Helper()
	for _, v := range []any{
		d.Observe, d.Unobserve, d.ObserverAck, d.InvocationsObserved, d.LocalInvocationStats,
		d.FunctionRegistered, d.ObserverAdded, d.ObserverRemoved, d.ObserveResult, d.ObserverStateChanged,
		d.RegisterFunction, d.InvokeFunction, d.InvocationResult, d.WorkerRegistered, d.RegistrationRejected,
	} {
		switch x := v.(type) {
		case *ObserveMessage:
			if x != nil {
				return x
			}
		case *UnobserveMessage:
			if x != nil {
				return x
			}
		case *ObserverAckMessage:
			if x != nil {
				return x
			}
		case *InvocationsObservedMessage:
			if x != nil {
				return x
			}
		case *LocalInvocationStatsMessage:
			if x != nil {
				return x
			}
		case *FunctionRegisteredMessage:
			if x != nil {
				return x
			}
		case *ObserverAddedMessage:
			if x != nil {
				return x
			}
		case *ObserverRemovedMessage:
			if x != nil {
				return x
			}
		case *ObserveResultMessage:
			if x != nil {
				return x
			}
		case *ObserverStateChangedMessage:
			if x != nil {
				return x
			}
		case *RegisterFunctionMessage:
			if x != nil {
				return x
			}
		case *InvokeFunctionMessage:
			if x != nil {
				return x
			}
		case *InvocationResultMessage:
			if x != nil {
				return x
			}
		case *WorkerRegisteredMessage:
			if x != nil {
				return x
			}
		case *RegistrationRejectedMessage:
			if x != nil {
				return x
			}
		}
	}
	t.Fatalf("no decoded variant for %q", d.Type)
	return nil
}

func TestObserveFixturesBuiltBySDK(t *testing.T) {
	c := New("ws://unused.invalid")
	t.Cleanup(func() { _ = c.Close() })

	t.Run("observe.json", func(t *testing.T) {
		h, err := c.Observe(Source{
			FunctionID: "cron::on-tick",
			Namespace:  "default",
			Args:       Args{"expression": "0 0 9 * * Mon-Fri"},
			Filters:    Filters{"input.scheduled_time": Op{"$gte": "2026-11-01T00:00:00Z"}},
			Capture:    []Capture{CaptureInput},
			On:         OnSuccess,
			Deliver:    DeliverEnvelope,
		}, "local::cron-handler", ObserverOptions{
			ID:              "5b0c3e1a-8f2d-4c6b-9a7e-1d2f3a4b5c6d",
			Metadata:        json.RawMessage(`{"__binding":"weekday-digest"}`),
			MaxFires:        1,
			ExpiresIn:       600 * time.Second,
			MaxInFlight:     64,
			TargetNamespace: "default",
		})
		if err != nil {
			t.Fatal(err)
		}
		assertJSONEqual(t, "observe.json", h.frame, observeFixture(t, "observe.json"))
	})
	t.Run("observe-minimal.json (the shorthand)", func(t *testing.T) {
		h, err := c.ObserveFunction("orders::archive", "search::reindex-order",
			ObserverOptions{ID: "d94f3a5b-6c7d-4e8f-a01b-2c3d4e5f6a7b"})
		if err != nil {
			t.Fatal(err)
		}
		assertJSONEqual(t, "observe-minimal.json", h.frame, observeFixture(t, "observe-minimal.json"))
	})
	t.Run("observe-on-error.json", func(t *testing.T) {
		h, err := c.Observe(Source{
			FunctionID: "payments::charge",
			On:         OnError,
			Filters:    Filters{"input.amount": Op{"$gte": 500}},
			Capture:    []Capture{CaptureInput, CaptureError, CaptureCaller},
		}, "alerts::payment-failed", ObserverOptions{
			ID:          "c81e2f4a-5b6c-4d7e-9f0a-1b2c3d4e5f60",
			Metadata:    json.RawMessage(`{"team":"payments-oncall"}`),
			MaxInFlight: 4,
		})
		if err != nil {
			t.Fatal(err)
		}
		assertJSONEqual(t, "observe-on-error.json", h.frame, observeFixture(t, "observe-on-error.json"))
	})
	t.Run("capture: [] is sent", func(t *testing.T) {
		h, err := c.Observe(Source{FunctionID: "a::b", Capture: []Capture{}}, "c::d")
		if err != nil {
			t.Fatal(err)
		}
		var msg map[string]map[string]json.RawMessage
		_ = json.Unmarshal(h.frame, &msg)
		if string(msg["source"]["capture"]) != "[]" {
			t.Errorf("capture = %s, want []", msg["source"]["capture"])
		}
	})
	t.Run("unobserve.json", func(t *testing.T) {
		out, err := MarshalMessage(&UnobserveMessage{ID: "5b0c3e1a-8f2d-4c6b-9a7e-1d2f3a4b5c6d"})
		if err != nil {
			t.Fatal(err)
		}
		assertJSONEqual(t, "unobserve.json", out, observeFixture(t, "unobserve.json"))
	})
	t.Run("invokefunction-workers-register.json (features)", func(t *testing.T) {
		var fixture struct {
			Data struct {
				Features []string `json:"features"`
			} `json:"data"`
		}
		_ = json.Unmarshal(observeFixture(t, "invokefunction-workers-register.json"), &fixture)
		data, _ := json.Marshal(workerMetadata{Features: sdkFeatures})
		var got struct {
			Features []string `json:"features"`
		}
		_ = json.Unmarshal(data, &got)
		if !slices.Equal(got.Features, fixture.Data.Features) {
			t.Errorf("features %v, want %v", got.Features, fixture.Data.Features)
		}
	})
	formatOf := func(t *testing.T, name, field string) json.RawMessage {
		var fixture map[string]json.RawMessage
		_ = json.Unmarshal(observeFixture(t, name), &fixture)
		return fixture[field]
	}
	description := func(t *testing.T, name string) string {
		var s string
		_ = json.Unmarshal(formatOf(t, name, "description"), &s)
		return s
	}
	for _, tc := range []struct {
		name, id string
		opts     func(t *testing.T, name string) RegisterFunctionOptions
	}{
		{"registerfunction.json", "orders::archive", func(t *testing.T, name string) RegisterFunctionOptions {
			return RegisterFunctionOptions{Description: description(t, name), RequestFormat: formatOf(t, name, "request_format"), ResponseFormat: formatOf(t, name, "response_format")}
		}},
		{"registerfunction-private.json", "cron::on-tick", func(t *testing.T, name string) RegisterFunctionOptions {
			return RegisterFunctionOptions{Description: description(t, name), RequestFormat: formatOf(t, name, "request_format"),
				ResponseFormat: formatOf(t, name, "response_format"), ArgsFormat: formatOf(t, name, "args_format"), Private: true, Standby: true}
		}},
		{"registerfunction-not-observable.json", "vault::read-secret", func(t *testing.T, name string) RegisterFunctionOptions {
			closed := false
			return RegisterFunctionOptions{Description: description(t, name), RequestFormat: formatOf(t, name, "request_format"), Observable: &closed}
		}},
	} {
		t.Run(tc.name, func(t *testing.T) {
			msg, err := registerFunctionMessage("RegisterFunction", tc.id, tc.opts(t, tc.name))
			if err != nil {
				t.Fatal(err)
			}
			out, err := MarshalMessage(msg)
			if err != nil {
				t.Fatal(err)
			}
			assertJSONEqual(t, tc.name, out, observeFixture(t, tc.name))
		})
	}
}

// The report items and the stats this SDK builds equal invocationsobserved.json,
// invocationsobserved-event-source.json and localinvocationstats.json: an item's
// caller carries the function, invocation and parent of the call's context, never the
// top-level worker (the engine stamps it), and no caller outside any handler.
func TestObserveFixturesReportsBuiltBySDK(t *testing.T) {
	var fixture InvocationsObservedMessage
	if err := json.Unmarshal(observeFixture(t, "invocationsobserved.json"), &fixture); err != nil {
		t.Fatal(err)
	}
	all := []Capture{CaptureInput, CaptureOutput, CaptureError, CaptureCaller, CaptureMetadata}
	tp := "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"
	checkout := &Caller{WorkerID: "2b8d4f6a-1c3e-4a5b-9d7f-0e2c4a6b8d1f", WorkerName: "checkout", FunctionID: "checkout::confirm", InvocationID: "5e7a9c1b-2d4f-4b6a-8c0e-1f3a5c7e9b20"}
	errBody, _ := json.Marshal(&ErrorBody{Code: "CARD_DECLINED", Message: "card declined by issuer"})
	host := &fakeReportHost{online: true, status: reportReady}
	r := newReporter(host, defaultReporterLimits)
	for i, b := range []struct {
		init     reportInit
		captures []Capture
	}{
		{reportInit{
			functionID: "payments::charge", namespace: "default", invocationID: "0d4b6f81-3a5c-4e7d-9b1f-2c4e6a8b0d13",
			matched: []string{"c81e2f4a-5b6c-4d7e-9f0a-1b2c3d4e5f60"}, status: "error",
			values: reportValues{
				input:   json.RawMessage(`{"customer_id":"cus_8812","amount":1250,"currency":"EUR"}`),
				errBody: errBody,
				caller:  reportCaller(checkout),
			},
			startedAtMs: 1760350000123, durationMs: 12.75, trace: traceContext{traceparent: &tp},
		}, []Capture{CaptureInput, CaptureError, CaptureCaller}},
		{reportInit{
			functionID: "orders::archive", namespace: "default", invocationID: "8a0c2e4f-6b8d-4f1a-9c3e-5a7c9e1b3d46",
			matched: []string{"d94f3a5b-6c7d-4e8f-a01b-2c3d4e5f6a7b"}, status: "success",
			values: reportValues{
				input:  json.RawMessage(`{"order_id":"ord_1001"}`),
				output: json.RawMessage(`null`),
				// a local call outside any handler: this worker only
				caller: reportCaller(&Caller{WorkerID: "7c1e0b52-93d4-4f0e-8a3b-6d2f1c9e4a70", WorkerName: "billing"}),
			},
			startedAtMs: 1760350000180, durationMs: 0.42,
		}, []Capture{CaptureInput, CaptureOutput, CaptureCaller}},
		{reportInit{
			functionID: "orders::create", namespace: "default", invocationID: "4b6d8f0a-2c4e-4a6c-8e0a-2c4e6a8c0e1f",
			matched: []string{"8b2d4f6a-0c1e-4a3b-9d5f-7a9c1e3b5d70"}, status: "success",
			values: reportValues{
				input:    json.RawMessage(`{"sku":"A-1","qty":2}`),
				output:   json.RawMessage(`{"order_id":"ord_1003","total":1250}`),
				caller:   reportCaller(checkout),
				metadata: json.RawMessage(`{"session_id":"s_41"}`),
			},
			startedAtMs: 1760350000150, durationMs: 3.25, trace: traceContext{traceparent: &tp},
		}, all},
	} {
		item := r.build(b.init, b.captures)
		want, _ := json.Marshal(fixture.Items[i])
		assertJSONEqual(t, "invocationsobserved.json item", item.json, want)
	}

	// an event source: the handler returned its input, so the item carries it once
	var source InvocationsObservedMessage
	_ = json.Unmarshal(observeFixture(t, "invocationsobserved-event-source.json"), &source)
	want := source.Items[0]
	inv := &invocation{functionID: "database::execute", invocationID: "4a6c8e0b-2d4f-4a6c-9e1b-3d5f7a9c1e3b",
		caller: &Caller{WorkerID: "7c1e0b52-93d4-4f0e-8a3b-6d2f1c9e4a70", WorkerName: "billing", FunctionID: "billing::close-invoice", InvocationID: "77e1c3a5-b7d9-4f1b-9d3f-5b7d9f1b3d5f"}}
	local := &Caller{WorkerID: "database-worker", WorkerName: "database", FunctionID: inv.functionID, InvocationID: inv.invocationID, Parent: stripCaller(inv.caller)}
	event := want.Input
	item := r.build(reportInit{
		functionID: want.FunctionID, namespace: "default", invocationID: want.InvocationID, matched: want.Matched, status: "success",
		values:      reportValues{input: event, output: event, caller: reportCaller(local)},
		startedAtMs: want.StartedAtMs, durationMs: want.DurationMs, trace: traceContext{traceparent: want.Traceparent, baggage: want.Baggage},
	}, []Capture{CaptureInput, CaptureOutput, CaptureCaller})
	wantJSON, _ := json.Marshal(want)
	assertJSONEqual(t, "invocationsobserved-event-source.json item", item.json, wantJSON)
	// to an engine without observe.v2 it carries the output
	if err := item.render(false); err != nil || item.wire.OutputIsInput || string(item.wire.Output) != string(event) {
		t.Errorf("without observe.v2: %s %v", item.json, err)
	}

	// localinvocationstats.json
	stats := newReporter(host, defaultReporterLimits)
	now := time.Now()
	stats.windowStart = now.Add(-10 * time.Second)
	stats.stats["database::on-row-changed"] = &functionStats{count: 48211, durationMsSum: 190.2}
	stats.stats["database::query"] = &functionStats{count: 37, errors: 2, durationMsSum: 15.5}
	stats.statsOrder = []string{"database::on-row-changed", "database::query"}
	stats.dropLocked("database::on-row-changed", dropBufferFull, 12)
	// a drop is sent with its reason as counted; the fixture's second row has a reason
	// of the engine's contract this SDK does not count
	stats.dropLocked("database::execute", "LATE_DELIVERY", 1)
	if !stats.sendStats(now) {
		t.Fatal("stats not sent")
	}
	assertJSONEqual(t, "localinvocationstats.json", host.sentFrames()[0], observeFixture(t, "localinvocationstats.json"))
}

// A call through an observer builds invokefunction-observer-call.json from inside the
// http::on-request invocation (the caller the SDK asserts, observe_depth 0 omitted, no
// namespace, activation.observer_id only), and invokefunction-observer-call-void.json
// outside any handler (no invocation_id, no caller).
func TestObserveFixtureObserverCallBuiltBySDK(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client(WithNamespace("acme"))
	host := newFunctionObservers(c, "http::on-request", true, true)
	route, err := newObserver(host, ObserverInfo{ID: "b6c4e2a0-9f1d-4b3a-8e7c-6d5f4e3a2b1c", Target: "users::get"}, 1)
	if err != nil {
		t.Fatal(err)
	}
	stock, err := newObserver(host, ObserverInfo{ID: "868c8f79-e34f-4390-8ca6-7904b54ba84a", Target: "storefront::sync-stock"}, 1)
	if err != nil {
		t.Fatal(err)
	}

	var fixture map[string]json.RawMessage
	_ = json.Unmarshal(observeFixture(t, "invokefunction-observer-call.json"), &fixture)
	var tp string
	_ = json.Unmarshal(fixture["traceparent"], &tp)
	inv := &invocation{functionID: "http::on-request", invocationID: "9b1d3f5a-7c9e-4b1d-8f3a-5c7e9b1d3f5a"}
	ctx := injectTraceContext(withInvocation(context.Background(), inv), traceContext{traceparent: &tp})
	ctx, cancel := context.WithTimeout(ctx, 200*time.Millisecond)
	defer cancel()
	_, _ = c.Call(ctx, CallRequest{Observer: route, Payload: fixture["data"]}) // the fake engine does not answer

	sent := fe.waitFrames(MsgInvokeFunction, 1, func(d *DecodedMessage) bool {
		return d.InvokeFunction.Activation != nil && d.InvokeFunction.FunctionID == "users::get"
	})[0].InvokeFunction
	got, _ := MarshalMessage(sent)
	var gotMap map[string]json.RawMessage
	_ = json.Unmarshal(got, &gotMap)
	gotMap["invocation_id"] = fixture["invocation_id"] // random
	a, _ := json.Marshal(gotMap)
	b, _ := json.Marshal(fixture)
	assertJSONEqual(t, "invokefunction-observer-call.json", a, b)

	var void map[string]json.RawMessage
	_ = json.Unmarshal(observeFixture(t, "invokefunction-observer-call-void.json"), &void)
	if _, err := c.Call(context.Background(), CallRequest{Observer: stock, Payload: void["data"], Action: VoidAction()}); err != nil {
		t.Fatal(err)
	}
	sent = fe.waitFrames(MsgInvokeFunction, 1, func(d *DecodedMessage) bool {
		return d.InvokeFunction.Activation != nil && d.InvokeFunction.FunctionID == "storefront::sync-stock"
	})[0].InvokeFunction
	got, _ = MarshalMessage(sent)
	assertJSONEqual(t, "invokefunction-observer-call-void.json", got, observeFixture(t, "invokefunction-observer-call-void.json"))
}

func TestObserveFixturesLenientDecode(t *testing.T) {
	// null for optional fields, unknown enum values kept
	dec, err := UnmarshalMessage([]byte(`{"type":"functionregistered","id":"f","namespace":"default","revision":3,"local_dispatch":true,
		"observers":[{"id":"o1","args":null,"filters":null,"target":"t::x","capture":null,"on":"someday"}]}`))
	if err != nil {
		t.Fatal(err)
	}
	info := dec.FunctionRegistered.Observers[0]
	if info.On != "someday" {
		t.Errorf("decoded %+v", info)
	}
	f := newFunctionObservers(New("ws://unused.invalid"), "f", true, false)
	obs, err := newObserver(f, info, 3)
	if err != nil {
		t.Fatalf("null filters compile as {}: %v", err)
	}
	if len(obs.Capture) != 3 || obs.On != "someday" || obs.Args == nil {
		t.Errorf("defaults: capture %v on %q args %v", obs.Capture, obs.On, obs.Args)
	}
	if _, err := newObserver(f, ObserverInfo{ID: "o2", Target: "t", Filters: json.RawMessage(`[1]`)}, 3); err == nil {
		t.Error("a present non-object filters must be INVALID_FILTER")
	}

	dec, err = UnmarshalMessage([]byte(`{"type":"observerstatechanged","id":"x","state":"hibernating","reason":null,"fires":2}`))
	if err != nil || dec.ObserverStateChanged.State != "hibernating" || toObserverState("hibernating") != ObserverPending {
		t.Errorf("unknown state: %v %+v", err, dec.ObserverStateChanged)
	}

	// the defaults left out are read when sent explicitly, and null reads as absent
	dec, err = UnmarshalMessage([]byte(`{"type":"registerfunction","id":"f","private":false,"observable":true}`))
	if err != nil || dec.RegisterFunction.Private || !*dec.RegisterFunction.Observable {
		t.Errorf("registerfunction defaults: %v %+v", err, dec.RegisterFunction)
	}
	dec, err = UnmarshalMessage([]byte(`{"type":"invocationresult","invocation_id":"3c5e7a9b-1d3f-4b5d-9f1a-3c5e7a9b1d3f","function_id":"f","result":null,"observed":null}`))
	if err != nil || dec.InvocationResult.Observed != nil || string(dec.InvocationResult.Result) != "null" {
		t.Errorf("invocationresult: %v %+v", err, dec.InvocationResult)
	}
	dec, err = UnmarshalMessage([]byte(`{"type":"invocationsobserved","items":[{"function_id":"f","invocation_id":"i","matched":[],"status":"success",` +
		`"started_at_ms":1,"duration_ms":0.5,"observe_depth":0,"output_is_input":null}]}`))
	if err != nil || dec.InvocationsObserved.Items[0].OutputIsInput {
		t.Errorf("output_is_input null: %v %+v", err, dec.InvocationsObserved)
	}

	// malformed observe fields on an invokefunction are absent, the frame still decodes
	dec, err = UnmarshalMessage([]byte(`{"type":"invokefunction","invocation_id":"3c5e7a9b-1d3f-4b5d-9f1a-3c5e7a9b1d3f","function_id":"f","data":{},
		"caller":"nope","observe_depth":-1,"activation":{"observer_id":7}}`))
	if err != nil {
		t.Fatal(err)
	}
	inv := dec.InvokeFunction
	if inv.Caller != nil || inv.ObserveDepth != 0 || inv.Activation != nil {
		t.Errorf("lenient decode: %+v", inv)
	}
	// a parent inside the parent is dropped (one level only)
	dec, _ = UnmarshalMessage([]byte(`{"type":"invokefunction","function_id":"f","data":{},
		"caller":{"worker_id":"w","worker_name":5,"parent":{"function_id":"p","parent":{"function_id":"pp"}}},"observe_depth":2,
		"activation":{"observer_id":"o","activation_id":"a"}}`))
	inv = dec.InvokeFunction
	if inv.Caller.WorkerID != "w" || inv.Caller.WorkerName != "" || inv.Caller.Parent.FunctionID != "p" || inv.Caller.Parent.Parent != nil {
		t.Errorf("caller %+v parent %+v", inv.Caller, inv.Caller.Parent)
	}
	if inv.ObserveDepth != 2 || inv.Activation.ObserverID != "o" || inv.Activation.ActivationID != "a" {
		t.Errorf("depth %d activation %+v", inv.ObserveDepth, inv.Activation)
	}
}

// fakeReportHost is a reporterHost recording what the reporter sends.
type fakeReportHost struct {
	mu     sync.Mutex
	queued int
	online bool
	// v1: the engine lacks observe.v2
	v1       bool
	status   reportStatus
	statuses map[string]reportStatus
	frames   [][]byte
}

func (h *fakeReportHost) reportOnline() (uint64, bool, bool) {
	h.mu.Lock()
	defer h.mu.Unlock()
	return 1, h.online, h.online && !h.v1
}

func (h *fakeReportHost) reportStatus(fn string, _ uint64) reportStatus {
	h.mu.Lock()
	defer h.mu.Unlock()
	if st, ok := h.statuses[fn]; ok {
		return st
	}
	return h.status
}

func (h *fakeReportHost) reportQueued() int {
	h.mu.Lock()
	defer h.mu.Unlock()
	return h.queued
}

func (h *fakeReportHost) sendReport(_ uint64, frame []byte) bool {
	h.mu.Lock()
	defer h.mu.Unlock()
	h.frames = append(h.frames, append([]byte(nil), frame...))
	return true
}

func (h *fakeReportHost) sentFrames() [][]byte {
	h.mu.Lock()
	defer h.mu.Unlock()
	return append([][]byte(nil), h.frames...)
}

func (h *fakeReportHost) set(online bool, status reportStatus) {
	h.mu.Lock()
	h.online, h.status = online, status
	h.mu.Unlock()
}

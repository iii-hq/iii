package iii

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"sync"
	"testing"
	"time"
)

// The observe wire fixtures (sdk/fixtures/protocol/observe, CONTRACT D5) are the wire
// contract: every file decodes into its message and encodes back to the same JSON
// value, and what this SDK builds equals them.

var observeFixturesDir = filepath.Join("..", "..", "..", "fixtures", "protocol", "observe")

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
	if len(names) < 28 {
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

// decodedVariant returns the one populated variant of a DecodedMessage.
func decodedVariant(t *testing.T, d *DecodedMessage) any {
	t.Helper()
	for _, v := range []any{
		d.UnregisterTriggerType, d.Observe, d.Unobserve, d.ObserverAck, d.InvocationsObserved,
		d.LocalInvocationStats, d.FunctionRegistered, d.ObserverAdded, d.ObserverRemoved,
		d.ObserveResult, d.ObserverStateChanged, d.RegisterFunction, d.InvokeFunction, d.WorkerRegistered,
	} {
		switch x := v.(type) {
		case *UnregisterTriggerTypeMessage:
			if x != nil {
				return x
			}
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
		case *WorkerRegisteredMessage:
			if x != nil {
				return x
			}
		}
	}
	t.Fatalf("no decoded variant for %q", d.Type)
	return nil
}

// handleFrame is the observe frame a handle sends.
func handleFrame(t *testing.T, h *ObserverHandle) []byte {
	t.Helper()
	return h.frame
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
		assertJSONEqual(t, "observe.json", handleFrame(t, h), observeFixture(t, "observe.json"))
	})
	t.Run("observe-minimal.json", func(t *testing.T) {
		h, err := c.Observe(Source{FunctionID: "orders::archive"}, "search::reindex-order",
			ObserverOptions{ID: "d94f3a5b-6c7d-4e8f-a01b-2c3d4e5f6a7b"})
		if err != nil {
			t.Fatal(err)
		}
		assertJSONEqual(t, "observe-minimal.json", handleFrame(t, h), observeFixture(t, "observe-minimal.json"))
	})
	t.Run("observe-auto.json", func(t *testing.T) {
		h, err := c.Observe(Source{
			FunctionID: "payments::charge",
			On:         OnError,
			Filters:    Filters{"input.amount": Op{"$gte": 500}},
			Capture:    []Capture{CaptureInput, CaptureError, CaptureCaller},
		}, "alerts::payment-failed", ObserverOptions{ID: "c81e2f4a-5b6c-4d7e-9f0a-1b2c3d4e5f60", MaxInFlight: 4})
		if err != nil {
			t.Fatal(err)
		}
		assertJSONEqual(t, "observe-auto.json", handleFrame(t, h), observeFixture(t, "observe-auto.json"))
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
	t.Run("unregistertriggertype.json", func(t *testing.T) {
		out, err := MarshalMessage(&UnregisterTriggerTypeMessage{ID: "webhook"})
		if err != nil {
			t.Fatal(err)
		}
		assertJSONEqual(t, "unregistertriggertype.json", out, observeFixture(t, "unregistertriggertype.json"))
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
		if strings.Join(got.Features, ",") != strings.Join(fixture.Data.Features, ",") {
			t.Errorf("features %v, want %v", got.Features, fixture.Data.Features)
		}
	})
	t.Run("registerfunction-owner.json (observe descriptor)", func(t *testing.T) {
		var fixture struct {
			Observe json.RawMessage `json:"observe"`
		}
		_ = json.Unmarshal(observeFixture(t, "registerfunction-owner.json"), &fixture)
		allow := true
		d, err := observeDescriptor("cron::on-tick", &ObserveHooks{
			Delivery:          DeliveryOwner,
			ArgsFormat:        json.RawMessage(`{"type":"object","additionalProperties":false,"required":["expression"],"properties":{"expression":{"type":"string"}}}`),
			LegacyTriggerType: "cron",
			Allow:             &allow,
			OnObserverAdded:   func(context.Context, *Observer) error { return nil },
		})
		if err != nil {
			t.Fatal(err)
		}
		out, _ := json.Marshal(d)
		assertJSONEqual(t, "registerfunction-owner.json observe", out, fixture.Observe)
	})
	t.Run("registerfunction-not-observable.json (observe descriptor)", func(t *testing.T) {
		var fixture struct {
			Observe json.RawMessage `json:"observe"`
		}
		_ = json.Unmarshal(observeFixture(t, "registerfunction-not-observable.json"), &fixture)
		allow := false
		d, err := observeDescriptor("vault::read-secret", &ObserveHooks{Delivery: DeliveryAuto, Allow: &allow})
		if err != nil {
			t.Fatal(err)
		}
		out, _ := json.Marshal(d)
		assertJSONEqual(t, "registerfunction-not-observable.json observe", out, fixture.Observe)
	})
	t.Run("registerfunction.json (no descriptor)", func(t *testing.T) {
		d, err := observeDescriptor("orders::archive", nil)
		if err != nil || d != nil {
			t.Fatalf("descriptor %v, %v: want none", d, err)
		}
		hooksOnly, err := observeDescriptor("orders::archive", &ObserveHooks{OnObserverRemoved: func(context.Context, *Observer) {}})
		if err != nil || hooksOnly != nil {
			t.Fatalf("callbacks never travel: %v, %v", hooksOnly, err)
		}
	})
}

// The report items and the stats this SDK builds equal invocationsobserved.json and
// localinvocationstats.json.
func TestObserveFixturesReportsBuiltBySDK(t *testing.T) {
	var fixture InvocationsObservedMessage
	if err := json.Unmarshal(observeFixture(t, "invocationsobserved.json"), &fixture); err != nil {
		t.Fatal(err)
	}
	all := []Capture{CaptureInput, CaptureOutput, CaptureError, CaptureCaller, CaptureMetadata}
	tp := "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"
	bg := "iii.function.id=database::execute"
	type built struct {
		init     reportInit
		captures []Capture
	}
	errBody, _ := json.Marshal(&ErrorBody{Code: "CARD_DECLINED", Message: "card declined by issuer"})
	cases := []built{
		{reportInit{
			functionID: "payments::charge", namespace: "default", invocationID: "0d4b6f81-3a5c-4e7d-9b1f-2c4e6a8b0d13",
			matched: []string{"c81e2f4a-5b6c-4d7e-9f0a-1b2c3d4e5f60"}, status: "error",
			values: reportValues{
				input:    json.RawMessage(`{"customer_id":"cus_8812","amount":1250,"currency":"EUR"}`),
				errBody:  errBody,
				caller:   &Caller{WorkerID: "2b8d4f6a-1c3e-4a5b-9d7f-0e2c4a6b8d1f", WorkerName: "checkout", FunctionID: "checkout::confirm", InvocationID: "5e7a9c1b-2d4f-4b6a-8c0e-1f3a5c7e9b20"},
				metadata: json.RawMessage(`{"session_id":"s_41"}`),
			},
			startedAtMs: 1760350000123, durationMs: 12.75, trace: traceContext{traceparent: &tp},
		}, all},
		{reportInit{
			functionID: "orders::archive", namespace: "default", invocationID: "8a0c2e4f-6b8d-4f1a-9c3e-5a7c9e1b3d46",
			matched: []string{"d94f3a5b-6c7d-4e8f-a01b-2c3d4e5f6a7b"}, status: "success",
			values: reportValues{
				input:  json.RawMessage(`{"order_id":"ord_1001"}`),
				output: json.RawMessage(`null`),
				caller: &Caller{WorkerID: "7c1e0b52-93d4-4f0e-8a3b-6d2f1c9e4a70", WorkerName: "billing"},
			},
			startedAtMs: 1760350000180, durationMs: 0.42, observeDepth: 1,
		}, []Capture{CaptureInput, CaptureOutput, CaptureCaller}},
	}
	for i, b := range cases {
		item := newReportItem(b.init)
		item.capture(b.captures)
		if err := item.render(); err != nil {
			t.Fatal(err)
		}
		want, _ := json.Marshal(fixture.Items[i])
		assertJSONEqual(t, "invocationsobserved.json item", item.json, want)
	}

	// the third item: one emitted event coalesced for two observers
	host := &fakeReportHost{online: true, status: reportReady}
	r := newReporter(host, defaultReporterLimits)
	event := json.RawMessage(`{"db":"primary","schema":"public","table":"invoices","op":"update","affected_rows":1,"returning":[{"id":77,"status":"paid","amount":1250}],"truncated":false,"at":1760350000201}`)
	ctx := &invocation{functionID: "database::execute"}
	init := reportInit{
		functionID: "database::on-row-changed", namespace: "default", invocationID: "c3f5a7b9-0d2e-4f4a-8b6c-7d9e1f3a5b7c",
		status: "success",
		values: reportValues{input: event, caller: &Caller{WorkerName: "database", FunctionID: "database::execute", InvocationID: "a1b3c5d7-e9f1-4a3b-8c5d-7e9f1a3b5c7d",
			Parent: &Caller{WorkerID: "7c1e0b52-93d4-4f0e-8a3b-6d2f1c9e4a70", WorkerName: "billing", FunctionID: "billing::close-invoice", InvocationID: "77e1c3a5-b7d9-4f1b-9d3f-5b7d9f1b3d5f"}}},
		startedAtMs: 1760350000201, durationMs: 0.004, trace: traceContext{traceparent: &tp, baggage: &bg},
	}
	r.pushEmit(init, []Capture{CaptureInput}, "e2a4c6b8-1d3f-4a5b-8c7d-9e0f1a2b3c4d", event, ctx)
	r.pushEmit(init, []Capture{CaptureCaller}, "f3b5d7c9-2e4a-4b6c-9d8e-0f1a2b3c4d5e", event, ctx)
	if r.size() != 1 {
		t.Fatalf("%d items, want one coalesced item", r.size())
	}
	want, _ := json.Marshal(fixture.Items[2])
	assertJSONEqual(t, "invocationsobserved.json coalesced item", r.items[0].json, want)

	// localinvocationstats.json
	stats := newReporter(host, defaultReporterLimits)
	now := time.Now()
	stats.windowStart = now.Add(-10 * time.Second)
	stats.stats["database::on-row-changed"] = &functionStats{count: 48211, durationMsSum: 190.2}
	stats.stats["orders::archive"] = &functionStats{count: 37, errors: 2, durationMsSum: 15.5}
	stats.statsOrder = []string{"database::on-row-changed", "orders::archive"}
	stats.dropLocked("database::on-row-changed", dropBufferFull, 12)
	if !stats.sendStats(now) {
		t.Fatal("stats not sent")
	}
	assertJSONEqual(t, "localinvocationstats.json", host.sentFrames()[0], observeFixture(t, "localinvocationstats.json"))
}

// obs.Call builds invokefunction-owner-call.json: the activation, the caller the SDK
// asserts (the handler it runs in), observe_depth 0 omitted, and no namespace.
func TestObserveFixtureOwnerCallBuiltBySDK(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client(WithNamespace("acme"))
	_ = c
	var fixture map[string]json.RawMessage
	_ = json.Unmarshal(observeFixture(t, "invokefunction-owner-call.json"), &fixture)

	owner := newFunctionObservers(c, "http::on-request", &ObserveHooks{Delivery: DeliveryOwner}, true)
	obs, err := newObserver(owner, ObserverInfo{ID: "4f2a9b1c-7d3e-4f6a-8b5c-2e1d0f9a8b7c", Target: "users::get"}, 1)
	if err != nil {
		t.Fatal(err)
	}
	owner.known[obs.ID] = obs
	owner.activateLocked(obs)

	inv := &invocation{functionID: "http::on-request", invocationID: "9b1d3f5a-7c9e-4b1d-8f3a-5c7e9b1d3f5a"}
	ctx, cancel := context.WithTimeout(withInvocation(context.Background(), inv), 200*time.Millisecond)
	defer cancel()
	var payload any
	_ = json.Unmarshal(fixture["data"], &payload)
	_, _ = obs.Call(ctx, payload) // the fake engine does not answer

	sent := fe.waitFrames(MsgInvokeFunction, 1, func(d *DecodedMessage) bool { return d.InvokeFunction.Activation != nil })[0].InvokeFunction
	got, _ := MarshalMessage(sent)
	var gotMap map[string]json.RawMessage
	_ = json.Unmarshal(got, &gotMap)
	// the invocation id is random; this SDK sends no traceparent yet (otel.go)
	gotMap["invocation_id"] = fixture["invocation_id"]
	delete(fixture, "traceparent")
	a, _ := json.Marshal(gotMap)
	b, _ := json.Marshal(fixture)
	assertJSONEqual(t, "invokefunction-owner-call.json", a, b)
}

func TestObserveFixturesLenientDecode(t *testing.T) {
	// null for optional fields, unknown enum values kept
	dec, err := UnmarshalMessage([]byte(`{"type":"functionregistered","id":"f","namespace":"default","revision":3,"local_dispatch":true,
		"observers":[{"id":"o1","args":null,"filters":null,"target":"t::x","capture":null,"on":"someday","legacy":null}]}`))
	if err != nil {
		t.Fatal(err)
	}
	info := dec.FunctionRegistered.Observers[0]
	if info.On != "someday" || info.Legacy != nil {
		t.Errorf("decoded %+v", info)
	}
	f := newFunctionObservers(New("ws://unused.invalid"), "f", nil, true)
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
	mu       sync.Mutex
	queued   int
	online   bool
	status   reportStatus
	statuses map[string]reportStatus
	frames   [][]byte
}

func (h *fakeReportHost) reportOnline() (uint64, bool) {
	h.mu.Lock()
	defer h.mu.Unlock()
	return 1, h.online
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

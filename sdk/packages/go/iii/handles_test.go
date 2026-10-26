package iii

import (
	"context"
	"encoding/json"
	"fmt"
	"testing"
	"time"

	"github.com/google/uuid"
)

// fastReconnect reconnects within milliseconds, so a test can drop the connection and
// watch the replay.
func fastReconnect() Option {
	return WithReconnectConfig(ReconnectConfig{
		InitialDelay:      10 * time.Millisecond,
		MaxDelay:          50 * time.Millisecond,
		BackoffMultiplier: 2,
		MaxRetries:        -1,
	})
}

// reconnectAndWaitReplay drops the connection and waits for the next one's replay to
// finish: engine::workers::register is the last frame onConnect sends. Only frames of the
// new connection count (and are returned), so a register frame of the dropped connection
// that lands late cannot end the wait before the client has reconnected.
func reconnectAndWaitReplay(t *testing.T, m *mockEngine) []map[string]json.RawMessage {
	t.Helper()
	next := m.connectionCount()
	m.closeActiveConnection()
	got := m.waitForFrom(next, func(msgs []map[string]json.RawMessage) bool {
		return indexOfFunctionID(msgs, FnRegisterWorker) >= 0
	}, 3*time.Second)
	if indexOfFunctionID(got, FnRegisterWorker) < 0 {
		t.Fatal("no replay after reconnect")
	}
	return got
}

// sentinel registers a throwaway function and waits for its frame. Frames leave through
// one channel in order, so a frame sent before the sentinel has arrived when it returns:
// counting after it proves a frame was not sent.
func sentinel(t *testing.T, m *mockEngine, c *Client, id string) []map[string]json.RawMessage {
	t.Helper()
	if _, err := c.RegisterFunction(id, noopHandler); err != nil {
		t.Fatal(err)
	}
	waitFrame(t, m, MsgRegisterFunction, id)
	return m.receivedMessages()
}

// triggerFrame waits for the registertrigger frame whose function_id is functionID.
func triggerFrame(t *testing.T, m *mockEngine, functionID string) map[string]json.RawMessage {
	t.Helper()
	match := func(msg map[string]json.RawMessage) bool {
		return messageType(msg) == string(MsgRegisterTrigger) && stringField(msg, "function_id") == functionID
	}
	got := m.waitFor(func(msgs []map[string]json.RawMessage) bool { return firstWhere(msgs, match) != nil }, 3*time.Second)
	frame := firstWhere(got, match)
	if frame == nil {
		t.Fatalf("no registertrigger frame for function %q", functionID)
	}
	return frame
}

func mustBeUUID(t *testing.T, id string) {
	t.Helper()
	if _, err := uuid.Parse(id); err != nil {
		t.Errorf("id %q is not a UUID: %v", id, err)
	}
}

// V2: the registertrigger frame of RegisterTrigger, as Node's registerTrigger sends it:
// a generated UUID id, the worker's namespace when Namespace is empty, trigger_namespace
// passed through, config and metadata as given.
func TestRegisterTriggerFrameWithGeneratedID(t *testing.T) {
	m := newMockEngine(t)
	c := connectClient(t, m, WithNamespace("orders"))
	tr, err := c.RegisterTrigger(RegisterTriggerInput{
		Type:             "http",
		FunctionID:       "orders::create",
		Config:           json.RawMessage(`{"api_path":"/orders","http_method":"POST"}`),
		Metadata:         json.RawMessage(`{"team":"shop"}`),
		TriggerNamespace: "infra",
	})
	if err != nil {
		t.Fatalf("RegisterTrigger: %v", err)
	}
	mustBeUUID(t, tr.ID)
	want := fmt.Sprintf(`{"config":{"api_path":"/orders","http_method":"POST"},"function_id":"orders::create",`+
		`"id":%q,"metadata":{"team":"shop"},"namespace":"orders","trigger_namespace":"infra",`+
		`"trigger_type":"http","type":"registertrigger"}`, tr.ID)
	if got := frameJSON(t, waitFrame(t, m, MsgRegisterTrigger, tr.ID)); got != want {
		t.Errorf("registertrigger frame\n got %s\nwant %s", got, want)
	}

	other, err := c.RegisterTrigger(RegisterTriggerInput{Type: "http", FunctionID: "orders::create"})
	if err != nil {
		t.Fatal(err)
	}
	if other.ID == tr.ID {
		t.Error("two registrations got the same generated id")
	}
}

// V2: an explicit ID and Namespace are sent as given, nil Config is {} and an empty
// TriggerNamespace is omitted.
func TestRegisterTriggerFrameWithExplicitIDAndNamespace(t *testing.T) {
	m := newMockEngine(t)
	c := connectClient(t, m, WithNamespace("orders"))
	tr, err := c.RegisterTrigger(RegisterTriggerInput{ID: "t-fixed", Type: "cron", FunctionID: "jobs::tick", Namespace: "billing"})
	if err != nil {
		t.Fatalf("RegisterTrigger: %v", err)
	}
	if tr.ID != "t-fixed" {
		t.Errorf("ID = %q, want t-fixed", tr.ID)
	}
	const want = `{"config":{},"function_id":"jobs::tick","id":"t-fixed","namespace":"billing","trigger_type":"cron","type":"registertrigger"}`
	if got := frameJSON(t, waitFrame(t, m, MsgRegisterTrigger, "t-fixed")); got != want {
		t.Errorf("registertrigger frame\n got %s\nwant %s", got, want)
	}
}

// V2: without a worker namespace no namespace is sent; a blank one is refused.
func TestRegisterTriggerFrameWithoutNamespace(t *testing.T) {
	t.Setenv("III_NAMESPACE", "")
	m := newMockEngine(t)
	c := connectClient(t, m)
	tr, err := c.RegisterTrigger(RegisterTriggerInput{Type: "http", FunctionID: "svc::fn"})
	if err != nil {
		t.Fatal(err)
	}
	want := fmt.Sprintf(`{"config":{},"function_id":"svc::fn","id":%q,"trigger_type":"http","type":"registertrigger"}`, tr.ID)
	if got := frameJSON(t, waitFrame(t, m, MsgRegisterTrigger, tr.ID)); got != want {
		t.Errorf("registertrigger frame\n got %s\nwant %s", got, want)
	}
	if _, err := c.RegisterTrigger(RegisterTriggerInput{Type: "http", FunctionID: "svc::fn", Namespace: "  "}); err == nil {
		t.Error("blank Namespace accepted")
	}
	if _, err := c.RegisterTrigger(RegisterTriggerInput{Type: "http", FunctionID: "svc::fn", Config: json.RawMessage(`{`)}); err == nil {
		t.Error("invalid Config accepted")
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	if len(c.triggers) != 1 {
		t.Errorf("%d triggers registered, want 1", len(c.triggers))
	}
}

// V1: FunctionRef.Unregister sends unregisterfunction, clears the registry, the replay
// leaves the function out, a second call does nothing (Node's stale-ref check).
func TestFunctionRefUnregister(t *testing.T) {
	m := newMockEngine(t)
	c := connectClient(t, m, fastReconnect())
	if _, err := c.RegisterFunction("keep::fn", noopHandler); err != nil {
		t.Fatal(err)
	}
	ref, err := c.RegisterFunction("drop::fn", noopHandler)
	if err != nil {
		t.Fatal(err)
	}
	if ref.ID != "drop::fn" {
		t.Errorf("ref.ID = %q", ref.ID)
	}
	waitFrame(t, m, MsgRegisterFunction, "drop::fn")

	if err := ref.Unregister(); err != nil {
		t.Fatalf("Unregister: %v", err)
	}
	const want = `{"id":"drop::fn","type":"unregisterfunction"}`
	if got := frameJSON(t, waitFrame(t, m, MsgUnregisterFunction, "drop::fn")); got != want {
		t.Errorf("unregisterfunction frame\n got %s\nwant %s", got, want)
	}
	c.mu.Lock()
	_, still := c.functions["drop::fn"]
	c.mu.Unlock()
	if still {
		t.Error("function still in the registry")
	}

	if err := ref.Unregister(); err != nil {
		t.Errorf("second Unregister: %v, want nil", err)
	}
	if n := countRegister(sentinel(t, m, c, "sentinel::a"), string(MsgUnregisterFunction), "drop::fn"); n != 1 {
		t.Errorf("%d unregisterfunction frames, want 1 (second Unregister is a no-op)", n)
	}

	got := reconnectAndWaitReplay(t, m)
	if n := countRegister(got, string(MsgRegisterFunction), "keep::fn"); n != 1 {
		t.Errorf("keep::fn replayed %d times, want 1", n)
	}
	if n := countRegister(got, string(MsgRegisterFunction), "drop::fn"); n != 0 {
		t.Errorf("drop::fn replayed %d times after Unregister, want 0", n)
	}
}

// V1: a handle whose id was registered again no longer controls it.
func TestFunctionRefStaleHandle(t *testing.T) {
	m := newMockEngine(t)
	c := connectClient(t, m)
	first, _ := c.RegisterFunction("dup::fn", noopHandler)
	second, err := c.RegisterFunction("dup::fn", noopHandler)
	if err != nil {
		t.Fatal(err)
	}
	if err := first.Unregister(); err != nil {
		t.Fatalf("stale Unregister: %v", err)
	}
	if n := countRegister(sentinel(t, m, c, "sentinel::b"), string(MsgUnregisterFunction), "dup::fn"); n != 0 {
		t.Errorf("stale handle sent %d unregisterfunction frames, want 0", n)
	}
	if c.functionObservers("dup::fn") == nil {
		t.Fatal("stale handle removed the newer registration")
	}
	if first.IsObserved() || len(first.Observers()) != 0 {
		t.Error("stale handle reports observers")
	}
	if err := second.Unregister(); err != nil {
		t.Fatal(err)
	}
	waitFrame(t, m, MsgUnregisterFunction, "dup::fn")
}

// V1: unregistering while disconnected sends nothing (it is not buffered) and the first
// connection never registers the function.
func TestFunctionRefUnregisterWhileDisconnected(t *testing.T) {
	m := newMockEngine(t)
	c := New(m.url)
	t.Cleanup(func() { _ = c.Close() })
	ref, err := c.RegisterFunction("early::fn", noopHandler)
	if err != nil {
		t.Fatal(err)
	}
	if err := ref.Unregister(); err != nil {
		t.Fatalf("Unregister: %v", err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	if err := c.Connect(ctx); err != nil {
		t.Fatal(err)
	}
	got := m.waitFor(func(msgs []map[string]json.RawMessage) bool {
		return indexOfFunctionID(msgs, FnRegisterWorker) >= 0
	}, 3*time.Second)
	if n := countRegister(got, string(MsgRegisterFunction), "early::fn"); n != 0 {
		t.Errorf("early::fn registered %d times, want 0", n)
	}
	if n := countType(got, string(MsgUnregisterFunction)); n != 0 {
		t.Errorf("%d unregisterfunction frames sent, want 0 (dropped while disconnected)", n)
	}
}

// V1: Unregister clears the function's observe state.
func TestFunctionRefUnregisterClearsObserveState(t *testing.T) {
	c := New("ws://127.0.0.1:0")
	ref, err := c.RegisterFunction("obs::fn", noopHandler, RegisterFunctionOptions{Observe: &ObserveHooks{Delivery: DeliveryOwner}})
	if err != nil {
		t.Fatal(err)
	}
	obs := c.functionObservers("obs::fn")
	if obs == nil {
		t.Fatal("no observe state after RegisterFunction")
	}
	c.mu.Lock()
	c.hooksInactiveWarned["obs::fn"] = true
	c.mu.Unlock()

	if err := ref.Unregister(); err != nil {
		t.Fatal(err)
	}
	if !obs.isDisposed() {
		t.Error("observe state not disposed")
	}
	if c.functionObservers("obs::fn") != nil {
		t.Error("function still has observe state")
	}
	if st := c.reportStatus("obs::fn", c.currentEpoch()); st != reportGone {
		t.Errorf("report status = %v, want reportGone (buffered reports dropped)", st)
	}
	if _, ok := c.localTarget("obs::fn", ""); ok {
		t.Error("unregistered function is still a local dispatch target")
	}
	c.mu.Lock()
	warned := c.hooksInactiveWarned["obs::fn"]
	c.mu.Unlock()
	if warned {
		t.Error("hooks-inactive warning state kept")
	}
}

// V1: the handles of RegisterHTTPFunction and RegisterFunctionTyped unregister too.
func TestFunctionRefFromHTTPAndTypedRegistration(t *testing.T) {
	m := newMockEngine(t)
	c := connectClient(t, m)
	httpRef, err := c.RegisterHTTPFunction("ext::fn", HTTPInvocationRef{URL: "https://example.com"})
	if err != nil {
		t.Fatal(err)
	}
	typedRef, err := RegisterFunctionTyped(c, "typed::fn", func(context.Context, struct{}) (struct{}, error) { return struct{}{}, nil })
	if err != nil {
		t.Fatal(err)
	}
	for _, ref := range []*FunctionRef{httpRef, typedRef} {
		if err := ref.Unregister(); err != nil {
			t.Fatalf("%s: %v", ref.ID, err)
		}
		waitFrame(t, m, MsgUnregisterFunction, ref.ID)
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	if len(c.functions) != 0 {
		t.Errorf("%d functions left, want 0", len(c.functions))
	}
}

// V2: Trigger.Unregister sends the trigger's id and type, clears the registry, the
// replay leaves it out, and a second call does nothing.
func TestTriggerUnregister(t *testing.T) {
	m := newMockEngine(t)
	c := connectClient(t, m, fastReconnect())
	keep, _ := c.RegisterTrigger(RegisterTriggerInput{Type: "http", FunctionID: "svc::fn", Config: json.RawMessage(`{"api_path":"/keep"}`)})
	drop, err := c.RegisterTrigger(RegisterTriggerInput{ID: "t-drop", Type: "cron", FunctionID: "svc::fn", Config: json.RawMessage(`{"expression":"* * * * * *"}`)})
	if err != nil {
		t.Fatal(err)
	}
	waitFrame(t, m, MsgRegisterTrigger, "t-drop")

	if err := drop.Unregister(); err != nil {
		t.Fatalf("Unregister: %v", err)
	}
	const want = `{"id":"t-drop","trigger_type":"cron","type":"unregistertrigger"}`
	if got := frameJSON(t, waitFrame(t, m, MsgUnregisterTrigger, "t-drop")); got != want {
		t.Errorf("unregistertrigger frame\n got %s\nwant %s", got, want)
	}
	if err := drop.Unregister(); err != nil {
		t.Errorf("second Unregister: %v, want nil", err)
	}
	if n := countRegister(sentinel(t, m, c, "sentinel::c"), string(MsgUnregisterTrigger), "t-drop"); n != 1 {
		t.Errorf("%d unregistertrigger frames, want 1", n)
	}

	got := reconnectAndWaitReplay(t, m)
	if n := countRegister(got, string(MsgRegisterTrigger), keep.ID); n != 1 {
		t.Errorf("kept trigger replayed %d times, want 1", n)
	}
	if n := countRegister(got, string(MsgRegisterTrigger), "t-drop"); n != 0 {
		t.Errorf("t-drop replayed %d times after Unregister, want 0", n)
	}

	// A handle whose id was registered again no longer controls it.
	old, _ := c.RegisterTrigger(RegisterTriggerInput{ID: "t-dup", Type: "http", FunctionID: "svc::fn"})
	newer, _ := c.RegisterTrigger(RegisterTriggerInput{ID: "t-dup", Type: "http", FunctionID: "svc::other"})
	if err := old.Unregister(); err != nil {
		t.Fatal(err)
	}
	if n := countRegister(sentinel(t, m, c, "sentinel::d"), string(MsgUnregisterTrigger), "t-dup"); n != 0 {
		t.Errorf("stale handle sent %d unregistertrigger frames, want 0", n)
	}
	if err := newer.Unregister(); err != nil {
		t.Fatal(err)
	}
	waitFrame(t, m, MsgUnregisterTrigger, "t-dup")
}

// V3: the registertriggertype frame and the trigger and function TriggerTypeRef binds,
// as Node's TriggerTypeRef sends them (the worker's namespace on the trigger).
func TestTriggerTypeRefRegistersTriggersAndFunctions(t *testing.T) {
	m := newMockEngine(t)
	c := connectClient(t, m, WithNamespace("orders"), fastReconnect())
	handler := &stubTriggerHandler{registered: make(chan TriggerConfig, 1), unregistered: make(chan TriggerConfig, 1)}
	ref, err := c.RegisterTriggerType(RegisterTriggerTypeInput{ID: "webhook", Description: "Webhooks"}, handler)
	if err != nil {
		t.Fatal(err)
	}
	if ref.ID != "webhook" {
		t.Errorf("ref.ID = %q", ref.ID)
	}
	if got, want := frameJSON(t, waitFrame(t, m, MsgRegisterTriggerType, "webhook")), `{"description":"Webhooks","id":"webhook","type":"registertriggertype"}`; got != want {
		t.Errorf("registertriggertype frame\n got %s\nwant %s", got, want)
	}

	tr, err := ref.RegisterTrigger("hooks::in", json.RawMessage(`{"path":"/a"}`), json.RawMessage(`{"source":"gh"}`))
	if err != nil {
		t.Fatalf("RegisterTrigger: %v", err)
	}
	mustBeUUID(t, tr.ID)
	want := fmt.Sprintf(`{"config":{"path":"/a"},"function_id":"hooks::in","id":%q,"metadata":{"source":"gh"},`+
		`"namespace":"orders","trigger_type":"webhook","type":"registertrigger"}`, tr.ID)
	if got := frameJSON(t, waitFrame(t, m, MsgRegisterTrigger, tr.ID)); got != want {
		t.Errorf("TriggerTypeRef.RegisterTrigger frame\n got %s\nwant %s", got, want)
	}

	fn, err := ref.RegisterFunction("hooks::handle", noopHandler, json.RawMessage(`{"path":"/b"}`), RegisterFunctionOptions{Description: "Handles hooks"})
	if err != nil {
		t.Fatalf("RegisterFunction: %v", err)
	}
	if got, want := frameJSON(t, waitFrame(t, m, MsgRegisterFunction, "hooks::handle")), `{"description":"Handles hooks","id":"hooks::handle","type":"registerfunction"}`; got != want {
		t.Errorf("registerfunction frame\n got %s\nwant %s", got, want)
	}
	bound := triggerFrame(t, m, "hooks::handle")
	boundID := stringField(bound, "id")
	mustBeUUID(t, boundID)
	want = fmt.Sprintf(`{"config":{"path":"/b"},"function_id":"hooks::handle","id":%q,"namespace":"orders",`+
		`"trigger_type":"webhook","type":"registertrigger"}`, boundID)
	if got := frameJSON(t, bound); got != want {
		t.Errorf("TriggerTypeRef.RegisterFunction trigger frame\n got %s\nwant %s", got, want)
	}
	msgs := m.receivedMessages()
	fnIdx := firstIndexWhere(msgs, func(msg map[string]json.RawMessage) bool {
		return messageType(msg) == string(MsgRegisterFunction) && messageID(msg) == "hooks::handle"
	})
	trIdx := firstIndexWhere(msgs, func(msg map[string]json.RawMessage) bool {
		return messageType(msg) == string(MsgRegisterTrigger) && stringField(msg, "id") == boundID
	})
	if fnIdx < 0 || trIdx < 0 || fnIdx > trIdx {
		t.Errorf("the function (frame %d) must be registered before its trigger (frame %d)", fnIdx, trIdx)
	}

	// As in the Node SDK, the FunctionRef removes the function only: the trigger stays
	// bound and is replayed.
	if err := fn.Unregister(); err != nil {
		t.Fatal(err)
	}
	waitFrame(t, m, MsgUnregisterFunction, "hooks::handle")
	got := reconnectAndWaitReplay(t, m)
	if n := countRegister(got, string(MsgRegisterFunction), "hooks::handle"); n != 0 {
		t.Errorf("hooks::handle replayed %d times after Unregister, want 0", n)
	}
	if n := countRegister(got, string(MsgRegisterTrigger), boundID); n != 1 {
		t.Errorf("the bound trigger replayed %d times, want 1 (Node keeps it)", n)
	}

	if _, err := ref.RegisterTrigger("hooks::in", nil, json.RawMessage(`{}`), json.RawMessage(`{}`)); err == nil {
		t.Error("two metadata values accepted")
	}
	if _, err := ref.RegisterFunction("hooks::bad", noopHandler, json.RawMessage(`{`)); err == nil {
		t.Error("invalid config accepted")
	}
	if c.functionObservers("hooks::bad") != nil {
		t.Error("a failed RegisterFunction left the function registered")
	}
}

// V3: TriggerTypeRef.Unregister sends unregistertriggertype, clears the registry, the
// replay leaves it out, and a second call does nothing; a stale handle does nothing.
func TestTriggerTypeRefUnregister(t *testing.T) {
	m := newMockEngine(t)
	c := connectClient(t, m, fastReconnect())
	handler := &stubTriggerHandler{registered: make(chan TriggerConfig, 1), unregistered: make(chan TriggerConfig, 1)}
	_, _ = c.RegisterTriggerType(RegisterTriggerTypeInput{ID: "tt-keep", Description: "kept"}, handler)
	drop, err := c.RegisterTriggerType(RegisterTriggerTypeInput{ID: "tt-drop", Description: "dropped"}, handler)
	if err != nil {
		t.Fatal(err)
	}
	waitFrame(t, m, MsgRegisterTriggerType, "tt-drop")

	if err := drop.Unregister(); err != nil {
		t.Fatalf("Unregister: %v", err)
	}
	const want = `{"id":"tt-drop","type":"unregistertriggertype"}`
	if got := frameJSON(t, waitFrame(t, m, MsgUnregisterTriggerType, "tt-drop")); got != want {
		t.Errorf("unregistertriggertype frame\n got %s\nwant %s", got, want)
	}
	if err := drop.Unregister(); err != nil {
		t.Errorf("second Unregister: %v, want nil", err)
	}
	if n := countRegister(sentinel(t, m, c, "sentinel::e"), string(MsgUnregisterTriggerType), "tt-drop"); n != 1 {
		t.Errorf("%d unregistertriggertype frames, want 1", n)
	}

	got := reconnectAndWaitReplay(t, m)
	if n := countRegister(got, string(MsgRegisterTriggerType), "tt-keep"); n != 1 {
		t.Errorf("tt-keep replayed %d times, want 1", n)
	}
	if n := countRegister(got, string(MsgRegisterTriggerType), "tt-drop"); n != 0 {
		t.Errorf("tt-drop replayed %d times after Unregister, want 0", n)
	}

	old, _ := c.RegisterTriggerType(RegisterTriggerTypeInput{ID: "tt-dup"}, handler)
	newer, _ := c.RegisterTriggerType(RegisterTriggerTypeInput{ID: "tt-dup", Description: "newer"}, handler)
	if err := old.Unregister(); err != nil {
		t.Fatal(err)
	}
	if n := countRegister(sentinel(t, m, c, "sentinel::f"), string(MsgUnregisterTriggerType), "tt-dup"); n != 0 {
		t.Errorf("stale handle sent %d unregistertriggertype frames, want 0", n)
	}
	if err := newer.Unregister(); err != nil {
		t.Fatal(err)
	}
	waitFrame(t, m, MsgUnregisterTriggerType, "tt-dup")

	// Client.UnregisterTriggerType still removes by id.
	_, _ = c.RegisterTriggerType(RegisterTriggerTypeInput{ID: "tt-by-id"}, handler)
	if err := c.UnregisterTriggerType("tt-by-id"); err != nil {
		t.Fatal(err)
	}
	waitFrame(t, m, MsgUnregisterTriggerType, "tt-by-id")
	if err := c.UnregisterTriggerType("tt-by-id"); err == nil {
		t.Error("unregistering an unknown trigger type returned nil")
	}
}

// Handles the client did not return are refused.
func TestForeignHandles(t *testing.T) {
	var nilFn *FunctionRef
	var nilTrigger *Trigger
	var nilType *TriggerTypeRef
	for name, unregister := range map[string]func() error{
		"zero FunctionRef":    (&FunctionRef{ID: "x"}).Unregister,
		"nil FunctionRef":     nilFn.Unregister,
		"zero Trigger":        (&Trigger{ID: "x"}).Unregister,
		"nil Trigger":         nilTrigger.Unregister,
		"zero TriggerTypeRef": (&TriggerTypeRef{ID: "x"}).Unregister,
		"nil TriggerTypeRef":  nilType.Unregister,
	} {
		if err := unregister(); err == nil {
			t.Errorf("%s: Unregister returned nil", name)
		}
	}
	if _, err := (&TriggerTypeRef{ID: "x"}).RegisterTrigger("fn", nil); err == nil {
		t.Error("zero TriggerTypeRef registered a trigger")
	}
	if _, err := (&TriggerTypeRef{ID: "x"}).RegisterFunction("fn", noopHandler, nil); err == nil {
		t.Error("zero TriggerTypeRef registered a function")
	}
}

func firstIndexWhere(msgs []map[string]json.RawMessage, pred func(map[string]json.RawMessage) bool) int {
	for i, m := range msgs {
		if pred(m) {
			return i
		}
	}
	return -1
}

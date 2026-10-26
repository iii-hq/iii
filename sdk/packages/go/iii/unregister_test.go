package iii

import (
	"context"
	"encoding/json"
	"testing"
	"time"
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

// A4: UnregisterFunction sends unregisterfunction and the reconnect replay leaves the
// function out, while the other registrations are replayed.
func TestUnregisterFunctionIsNotReplayed(t *testing.T) {
	m := newMockEngine(t)
	c := connectClient(t, m, fastReconnect())
	for _, id := range []string{"keep::fn", "drop::fn"} {
		if err := c.RegisterFunction(id, noopHandler); err != nil {
			t.Fatal(err)
		}
		waitFrame(t, m, MsgRegisterFunction, id)
	}

	if err := c.UnregisterFunction("drop::fn"); err != nil {
		t.Fatalf("UnregisterFunction: %v", err)
	}
	const want = `{"id":"drop::fn","type":"unregisterfunction"}`
	if got := frameJSON(t, waitFrame(t, m, MsgUnregisterFunction, "drop::fn")); got != want {
		t.Errorf("unregisterfunction frame\n got %s\nwant %s", got, want)
	}

	got := reconnectAndWaitReplay(t, m)
	if n := countRegister(got, string(MsgRegisterFunction), "keep::fn"); n != 1 {
		t.Errorf("keep::fn replayed %d times, want 1", n)
	}
	if n := countRegister(got, string(MsgRegisterFunction), "drop::fn"); n != 0 {
		t.Errorf("drop::fn replayed %d times after UnregisterFunction, want 0", n)
	}

	if err := c.UnregisterFunction("drop::fn"); err == nil {
		t.Error("unregistering an unknown function returned nil")
	}
}

// A4: unregistering while disconnected sends nothing (it is not buffered) and the first
// connection never registers the function.
func TestUnregisterFunctionWhileDisconnected(t *testing.T) {
	m := newMockEngine(t)
	c := New(m.url)
	t.Cleanup(func() { _ = c.Close() })
	if err := c.RegisterFunction("early::fn", noopHandler); err != nil {
		t.Fatal(err)
	}
	if err := c.UnregisterFunction("early::fn"); err != nil {
		t.Fatalf("UnregisterFunction: %v", err)
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

// A4: UnregisterFunction clears the function's observe state.
func TestUnregisterFunctionClearsObserveState(t *testing.T) {
	c := New("ws://127.0.0.1:0")
	if err := c.RegisterFunction("obs::fn", noopHandler, RegisterFunctionOptions{Observe: &ObserveHooks{Delivery: DeliveryOwner}}); err != nil {
		t.Fatal(err)
	}
	obs := c.functionObservers("obs::fn")
	if obs == nil {
		t.Fatal("no observe state after RegisterFunction")
	}
	c.mu.Lock()
	c.hooksInactiveWarned["obs::fn"] = true
	c.mu.Unlock()

	if err := c.UnregisterFunction("obs::fn"); err != nil {
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

// A4: UnregisterTrigger sends the trigger's id and type, and the replay leaves it out.
func TestUnregisterTriggerIsNotReplayed(t *testing.T) {
	m := newMockEngine(t)
	c := connectClient(t, m, fastReconnect())
	_ = c.RegisterTrigger("t-keep", "http", "svc::fn", json.RawMessage(`{"api_path":"/keep"}`))
	_ = c.RegisterTrigger("t-drop", "cron", "svc::fn", json.RawMessage(`{"expression":"* * * * * *"}`))
	waitFrame(t, m, MsgRegisterTrigger, "t-drop")

	if err := c.UnregisterTrigger("t-drop"); err != nil {
		t.Fatalf("UnregisterTrigger: %v", err)
	}
	const want = `{"id":"t-drop","trigger_type":"cron","type":"unregistertrigger"}`
	if got := frameJSON(t, waitFrame(t, m, MsgUnregisterTrigger, "t-drop")); got != want {
		t.Errorf("unregistertrigger frame\n got %s\nwant %s", got, want)
	}

	got := reconnectAndWaitReplay(t, m)
	if n := countRegister(got, string(MsgRegisterTrigger), "t-keep"); n != 1 {
		t.Errorf("t-keep replayed %d times, want 1", n)
	}
	if n := countRegister(got, string(MsgRegisterTrigger), "t-drop"); n != 0 {
		t.Errorf("t-drop replayed %d times after UnregisterTrigger, want 0", n)
	}
	if err := c.UnregisterTrigger("t-drop"); err == nil {
		t.Error("unregistering an unknown trigger returned nil")
	}
}

// A4: UnregisterTriggerType sends unregistertriggertype and the replay leaves it out.
func TestUnregisterTriggerTypeIsNotReplayed(t *testing.T) {
	m := newMockEngine(t)
	c := connectClient(t, m, fastReconnect())
	handler := &stubTriggerHandler{registered: make(chan TriggerConfig, 1), unregistered: make(chan TriggerConfig, 1)}
	_ = c.RegisterTriggerType("tt-keep", "kept", handler)
	_ = c.RegisterTriggerType("tt-drop", "dropped", handler)
	waitFrame(t, m, MsgRegisterTriggerType, "tt-drop")

	if err := c.UnregisterTriggerType("tt-drop"); err != nil {
		t.Fatalf("UnregisterTriggerType: %v", err)
	}
	const want = `{"id":"tt-drop","type":"unregistertriggertype"}`
	if got := frameJSON(t, waitFrame(t, m, MsgUnregisterTriggerType, "tt-drop")); got != want {
		t.Errorf("unregistertriggertype frame\n got %s\nwant %s", got, want)
	}

	got := reconnectAndWaitReplay(t, m)
	if n := countRegister(got, string(MsgRegisterTriggerType), "tt-keep"); n != 1 {
		t.Errorf("tt-keep replayed %d times, want 1", n)
	}
	if n := countRegister(got, string(MsgRegisterTriggerType), "tt-drop"); n != 0 {
		t.Errorf("tt-drop replayed %d times after UnregisterTriggerType, want 0", n)
	}
	if err := c.UnregisterTriggerType("tt-drop"); err == nil {
		t.Error("unregistering an unknown trigger type returned nil")
	}
}

package iii

import (
	"context"
	"encoding/json"
	"strings"
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

// V1: a handle whose id was unregistered and registered again no longer controls it.
func TestFunctionRefStaleHandle(t *testing.T) {
	m := newMockEngine(t)
	c := connectClient(t, m)
	first, err := c.RegisterFunction("dup::fn", noopHandler)
	if err != nil {
		t.Fatal(err)
	}
	if err := first.Unregister(); err != nil {
		t.Fatal(err)
	}
	waitFrame(t, m, MsgUnregisterFunction, "dup::fn")
	second, err := c.RegisterFunction("dup::fn", noopHandler)
	if err != nil {
		t.Fatalf("registering again after Unregister: %v", err)
	}
	if err := first.Unregister(); err != nil {
		t.Fatalf("stale Unregister: %v", err)
	}
	if n := countRegister(sentinel(t, m, c, "sentinel::b"), string(MsgUnregisterFunction), "dup::fn"); n != 1 {
		t.Errorf("%d unregisterfunction frames, want 1 (the stale handle sends none)", n)
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
	if n := countRegister(sentinel(t, m, c, "sentinel::b2"), string(MsgUnregisterFunction), "dup::fn"); n != 2 {
		t.Errorf("%d unregisterfunction frames, want 2", n)
	}
}

// A function id is registered once per client, as in the Node, Python and Rust SDKs:
// a second registration of the same id fails, by any of the registration methods, and
// leaves the first untouched; an empty id is refused.
func TestRegisterFunctionRefusesDuplicateAndEmptyIDs(t *testing.T) {
	m := newMockEngine(t)
	c := connectClient(t, m)
	first, err := c.RegisterFunction("once::fn", noopHandler)
	if err != nil {
		t.Fatal(err)
	}
	waitFrame(t, m, MsgRegisterFunction, "once::fn")
	observe := c.functionObservers("once::fn")

	typed := func(context.Context, struct{}) (struct{}, error) { return struct{}{}, nil }
	for name, register := range map[string]func() (*FunctionRef, error){
		"RegisterFunction": func() (*FunctionRef, error) { return c.RegisterFunction("once::fn", noopHandler) },
		"RegisterHTTPFunction": func() (*FunctionRef, error) {
			return c.RegisterHTTPFunction("once::fn", HTTPInvocationRef{URL: "https://x"})
		},
		"RegisterFunctionTyped": func() (*FunctionRef, error) {
			return RegisterFunctionTyped(c, "once::fn", typed)
		},
	} {
		ref, err := register()
		if ref != nil || err == nil || !strings.Contains(err.Error(), `"once::fn"): function id already registered`) {
			t.Errorf("%s: ref %v, error %v, want function id already registered", name, ref, err)
		}
	}
	for _, id := range []string{"", "   "} {
		if _, err := c.RegisterFunction(id, noopHandler); err == nil || !strings.Contains(err.Error(), "id is required") {
			t.Errorf("RegisterFunction(%q): error %v, want id is required", id, err)
		}
		if _, err := c.RegisterHTTPFunction(id, HTTPInvocationRef{URL: "https://x"}); err == nil || !strings.Contains(err.Error(), "id is required") {
			t.Errorf("RegisterHTTPFunction(%q): error %v, want id is required", id, err)
		}
	}
	msgs := sentinel(t, m, c, "sentinel::dup")
	if n := countRegister(msgs, string(MsgRegisterFunction), "once::fn"); n != 1 {
		t.Errorf("once::fn registered %d times on the wire, want 1", n)
	}
	if c.functionObservers("once::fn") != observe {
		t.Error("a refused registration replaced the first one")
	}
	if err := first.Unregister(); err != nil {
		t.Fatal(err)
	}
	if _, err := c.RegisterFunction("once::fn", noopHandler); err != nil {
		t.Errorf("registering again after Unregister: %v", err)
	}
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

// V1: Unregister clears the function's observe state, and keeps the hooks set for its
// id: a later registration of the id gets them.
func TestFunctionRefUnregisterClearsObserveState(t *testing.T) {
	c := New("ws://127.0.0.1:0")
	ref, err := c.RegisterFunction("obs::fn", noopHandler, RegisterFunctionOptions{Private: true})
	if err != nil {
		t.Fatal(err)
	}
	ref.OnObserverAdded(func(context.Context, *Observer) error { return nil }).
		OnObserverMatch(MatchFunc(func(*Observer, *Call) bool { return true })).
		OnObserverDeliver(handlerAnswer)
	obs := c.functionObservers("obs::fn")
	if obs == nil {
		t.Fatal("no observe state after RegisterFunction")
	}
	c.mu.Lock()
	c.unsupportedWarned["obs::fn"] = true
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
	if _, _, ok := c.localTarget("obs::fn", ""); ok {
		t.Error("unregistered function is still a local dispatch target")
	}
	c.mu.Lock()
	warned := c.unsupportedWarned["obs::fn"]
	hooks := c.functionHooks["obs::fn"]
	c.mu.Unlock()
	if warned {
		t.Error("UNSUPPORTED warning state kept")
	}
	if hooks == nil || hooks.added == nil || hooks.match == nil || hooks.deliver == nil {
		t.Fatal("the hooks set for the id were dropped by Unregister")
	}
	if _, err := c.RegisterFunction("obs::fn", noopHandler); err != nil {
		t.Fatal(err)
	}
	if st := c.functionObservers("obs::fn").state.Load(); st.deliver == nil || st.match == nil {
		t.Error("a registration of the id again does not get its deliver and match hooks")
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

// Handles the client did not return are refused, and their hook methods do nothing.
func TestForeignHandles(t *testing.T) {
	var nilFn *FunctionRef
	for name, unregister := range map[string]func() error{
		"zero FunctionRef": (&FunctionRef{ID: "x"}).Unregister,
		"nil FunctionRef":  nilFn.Unregister,
	} {
		if err := unregister(); err == nil {
			t.Errorf("%s: Unregister returned nil", name)
		}
	}
	zero := &FunctionRef{ID: "x"}
	if zero.OnObserverAdded(func(context.Context, *Observer) error { return nil }) != zero ||
		nilFn.OnObserverRemoved(func(context.Context, *Observer) {}) != nil ||
		zero.OnObserverMatch(nil) != zero || zero.OnObserverDeliver(nil) != zero || zero.IsObservedFor(&Call{}) || zero.IsObserved() {
		t.Error("a foreign handle's methods must return it and report nothing observed")
	}
}

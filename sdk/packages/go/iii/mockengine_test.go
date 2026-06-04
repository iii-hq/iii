package iii

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/coder/websocket"
)

// mockEngine is an in-process WebSocket stand-in for the iii engine, used by the
// client tests so they can assert on the exact frames the SDK emits without depending
// on a running engine. It is a Go port of the Rust SDK's MockEngine
// (sdk/packages/rust/iii/tests/common/mock_engine.rs), extended with an outbound
// scripting hook (OnReceive) because the Go client tests drive both directions of the
// protocol: the engine sends workerregistered on connect, invokefunction to exercise a
// worker's handler, and invocationresult / triggerregistrationresult acks for outbound
// Trigger calls.
//
// Every received text frame is decoded as JSON and appended to a buffer observable via
// the WaitFor* helpers. Concurrency: the buffer and the active-connection handle are
// guarded by mu; OnReceive runs on the read goroutine, so a handler that wants to reply
// uses the conn passed to it (do not block it indefinitely).
type mockEngine struct {
	server *httptest.Server
	url    string

	mu       sync.Mutex
	received []map[string]json.RawMessage
	active   *websocket.Conn // most-recent accepted connection, for forced close

	// onReceive, if set, is called for every decoded inbound frame with the
	// connection it arrived on, letting a test script the engine's reply. It runs
	// while holding no locks.
	onReceive func(conn *websocket.Conn, msg map[string]json.RawMessage)

	// updated under mu; broadcast lets WaitFor wake on every new frame.
	gotMessage *sync.Cond
}

// newMockEngine starts a mock engine on an ephemeral loopback port and returns it with
// the buffer empty. Call Close (via t.Cleanup) to shut it down.
func newMockEngine(t *testing.T) *mockEngine {
	t.Helper()
	m := &mockEngine{}
	m.gotMessage = sync.NewCond(&m.mu)

	m.server = httptest.NewServer(http.HandlerFunc(m.handle))
	// httptest serves http://; the SDK dials ws://. Same host:port, different scheme.
	m.url = "ws" + strings.TrimPrefix(m.server.URL, "http")

	t.Cleanup(m.close)
	return m
}

// handle accepts a worker WebSocket and pumps inbound frames into the buffer. It
// mirrors the Rust mock's accept loop: decode each text frame as a JSON object, record
// it, notify waiters, and (unlike the Rust mock) hand it to onReceive so the test can
// reply on the same connection.
func (m *mockEngine) handle(w http.ResponseWriter, r *http.Request) {
	conn, err := websocket.Accept(w, r, nil)
	if err != nil {
		return
	}
	// Read frames until the worker disconnects. Use a background-derived context: the
	// httptest request context is cancelled when the handler returns, which is exactly
	// what we want to bound reads, so we tie reads to r.Context().
	ctx := r.Context()

	m.mu.Lock()
	m.active = conn
	m.mu.Unlock()

	for {
		typ, data, err := conn.Read(ctx)
		if err != nil {
			return
		}
		if typ != websocket.MessageText {
			continue
		}
		var msg map[string]json.RawMessage
		if err := json.Unmarshal(data, &msg); err != nil {
			continue // ignore non-JSON frames, as the Rust mock does
		}

		m.mu.Lock()
		m.received = append(m.received, msg)
		m.gotMessage.Broadcast()
		hook := m.onReceive
		m.mu.Unlock()

		if hook != nil {
			hook(conn, msg)
		}
	}
}

// send marshals a message variant (a *Message struct from protocol.go) and writes it to
// conn as a text frame. Test helpers use it from inside onReceive to script replies.
func (m *mockEngine) send(ctx context.Context, conn *websocket.Conn, msg any) error {
	frame, err := MarshalMessage(msg)
	if err != nil {
		return err
	}
	return conn.Write(ctx, websocket.MessageText, frame)
}

// sendRaw writes a pre-encoded frame, for tests that want to send a hand-crafted or
// deliberately malformed payload the typed MarshalMessage would not produce.
func (m *mockEngine) sendRaw(ctx context.Context, conn *websocket.Conn, frame []byte) error {
	return conn.Write(ctx, websocket.MessageText, frame)
}

// receivedMessages returns a snapshot copy of every frame received so far.
func (m *mockEngine) receivedMessages() []map[string]json.RawMessage {
	m.mu.Lock()
	defer m.mu.Unlock()
	out := make([]map[string]json.RawMessage, len(m.received))
	copy(out, m.received)
	return out
}

// closeActiveConnection force-closes the current worker connection so a test can
// observe the SDK reconnect. The mock keeps listening and accepts the next handshake on
// the same URL — mirroring close_active_connection in the Rust mock.
func (m *mockEngine) closeActiveConnection() {
	m.mu.Lock()
	conn := m.active
	m.active = nil
	m.mu.Unlock()
	if conn != nil {
		// CloseNow drops the TCP connection immediately without a closing handshake,
		// which is what the SDK sees when the engine goes away abruptly — and what its
		// reconnect loop must handle. A graceful Close would block waiting for the peer
		// to echo the close frame, which a dropped worker never does.
		_ = conn.CloseNow()
	}
}

// clear empties the recorded-frames buffer, e.g. between asserting the initial
// registration set and triggering a reconnect. Mirrors clear() in the Rust mock.
func (m *mockEngine) clear() {
	m.mu.Lock()
	m.received = nil
	m.mu.Unlock()
}

// waitFor blocks until predicate holds on the recorded frames or timeout elapses,
// returning the final snapshot. Port of wait_for in the Rust mock, built on a cond var
// so it wakes on each new frame instead of polling.
func (m *mockEngine) waitFor(predicate func([]map[string]json.RawMessage) bool, timeout time.Duration) []map[string]json.RawMessage {
	deadline := time.Now().Add(timeout)

	// A watcher goroutine broadcasts at the deadline so a waiting Wait() unblocks even
	// if no further frame arrives.
	done := make(chan struct{})
	defer close(done)
	go func() {
		t := time.NewTimer(time.Until(deadline))
		defer t.Stop()
		select {
		case <-t.C:
			m.mu.Lock()
			m.gotMessage.Broadcast()
			m.mu.Unlock()
		case <-done:
		}
	}()

	m.mu.Lock()
	defer m.mu.Unlock()
	for {
		if predicate(m.received) || time.Now().After(deadline) {
			out := make([]map[string]json.RawMessage, len(m.received))
			copy(out, m.received)
			return out
		}
		m.gotMessage.Wait()
	}
}

// waitForCount waits until at least count frames have been received. Mirrors
// wait_for_count in the Rust mock.
func (m *mockEngine) waitForCount(count int, timeout time.Duration) []map[string]json.RawMessage {
	return m.waitFor(func(msgs []map[string]json.RawMessage) bool {
		return len(msgs) >= count
	}, timeout)
}

func (m *mockEngine) close() {
	m.closeActiveConnection()
	m.server.Close()
}

// --- frame inspection helpers, ports of the free functions in the Rust mock ---

// messageType returns the "type" tag of a recorded frame, or "" if absent.
func messageType(msg map[string]json.RawMessage) string {
	return stringField(msg, "type")
}

// messageID returns the "id" field of a recorded frame, or "" if absent.
func messageID(msg map[string]json.RawMessage) string {
	return stringField(msg, "id")
}

// stringField extracts a top-level string field from a recorded frame.
func stringField(msg map[string]json.RawMessage, key string) string {
	raw, ok := msg[key]
	if !ok {
		return ""
	}
	var s string
	if err := json.Unmarshal(raw, &s); err != nil {
		return ""
	}
	return s
}

// countType counts recorded frames with the given "type", regardless of id. Port of
// count_type in the Rust mock.
func countType(msgs []map[string]json.RawMessage, msgType string) int {
	n := 0
	for _, m := range msgs {
		if messageType(m) == msgType {
			n++
		}
	}
	return n
}

// countRegister counts recorded frames matching both "type" and "id". Port of
// count_register in the Rust mock; used to assert a registration was (re)sent exactly
// once / once-per-connection.
func countRegister(msgs []map[string]json.RawMessage, msgType, id string) int {
	n := 0
	for _, m := range msgs {
		if messageType(m) == msgType && messageID(m) == id {
			n++
		}
	}
	return n
}

// firstOfType returns the first recorded frame with the given "type", or nil.
func firstOfType(msgs []map[string]json.RawMessage, msgType string) map[string]json.RawMessage {
	for _, m := range msgs {
		if messageType(m) == msgType {
			return m
		}
	}
	return nil
}

// dialMock is a minimal raw WebSocket client used only by this file's self-tests to
// exercise the harness before client.go exists. The real SDK will dial with its own
// connection management; here we just need a socket that speaks the protocol.
func dialMock(t *testing.T, ctx context.Context, url string) *websocket.Conn {
	t.Helper()
	// Use a client with keep-alives disabled. WebSocket hijacks the underlying TCP
	// connection, so a pooled/keep-alive transport can hand a later dial a stale
	// connection from a closed socket — which manifests as the second dial in a
	// reconnect test hanging until the context deadline.
	httpClient := &http.Client{
		Transport: &http.Transport{DisableKeepAlives: true},
	}
	conn, _, err := websocket.Dial(ctx, url, &websocket.DialOptions{HTTPClient: httpClient})
	if err != nil {
		t.Fatalf("dial mock engine: %v", err)
	}
	t.Cleanup(func() { _ = conn.CloseNow() })
	return conn
}

// TestMockEngineRecordsAndScriptsReplies is a self-test of the harness: it proves the
// mock records inbound frames, that WaitFor wakes on them, the inspection helpers read
// the right fields, and the OnReceive hook can script a reply the client reads back.
// This commit ships the harness ahead of the client (iii-hq/iii#1719), so it must be
// demonstrably correct on its own.
func TestMockEngineRecordsAndScriptsReplies(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	m := newMockEngine(t)

	// Script the engine: when it receives a registerfunction, ack the worker by
	// sending back a workerregistered frame, the way the real engine greets a worker.
	m.onReceive = func(conn *websocket.Conn, msg map[string]json.RawMessage) {
		if messageType(msg) == string(MsgRegisterFunction) {
			_ = m.send(ctx, conn, &WorkerRegisteredMessage{WorkerID: "w-self-test"})
		}
	}

	conn := dialMock(t, ctx, m.url)

	// Send two registrations the worker would send on connect.
	for _, id := range []string{"hello::greet", "hello::bye"} {
		frame, err := MarshalMessage(&RegisterFunctionMessage{ID: id})
		if err != nil {
			t.Fatalf("marshal: %v", err)
		}
		if err := m.sendRaw(ctx, conn, frame); err != nil {
			t.Fatalf("write: %v", err)
		}
	}

	// WaitFor must wake once both registrations are recorded.
	got := m.waitForCount(2, 2*time.Second)
	if len(got) != 2 {
		t.Fatalf("recorded %d frames, want 2", len(got))
	}
	if countType(got, string(MsgRegisterFunction)) != 2 {
		t.Errorf("countType registerfunction = %d, want 2", countType(got, string(MsgRegisterFunction)))
	}
	if countRegister(got, string(MsgRegisterFunction), "hello::greet") != 1 {
		t.Errorf("registerfunction hello::greet not recorded exactly once")
	}
	if f := firstOfType(got, string(MsgRegisterFunction)); messageID(f) != "hello::greet" {
		t.Errorf("firstOfType id = %q, want hello::greet", messageID(f))
	}

	// The scripted reply must arrive back on the same socket.
	_, data, err := conn.Read(ctx)
	if err != nil {
		t.Fatalf("read scripted reply: %v", err)
	}
	dec, err := UnmarshalMessage(data)
	if err != nil {
		t.Fatalf("decode reply: %v", err)
	}
	if dec.WorkerRegistered == nil || dec.WorkerRegistered.WorkerID != "w-self-test" {
		t.Fatalf("expected workerregistered reply, got %q", dec.Type)
	}
}

// TestMockEngineReconnect proves closeActiveConnection drops the worker's socket while
// the listener keeps accepting, so the client tests can assert reconnect + re-send.
func TestMockEngineReconnect(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	m := newMockEngine(t)

	conn1 := dialMock(t, ctx, m.url)
	if err := m.sendRaw(ctx, conn1, mustMarshal(t, &PingMessage{})); err != nil {
		t.Fatalf("write on conn1: %v", err)
	}
	m.waitForCount(1, 2*time.Second)

	// Force-close, then confirm the same URL still accepts a fresh connection and
	// records on the (cleared) buffer.
	m.closeActiveConnection()
	if _, _, err := conn1.Read(ctx); err == nil {
		t.Error("expected conn1 to be closed by the mock")
	}
	m.clear()

	conn2 := dialMock(t, ctx, m.url)
	if err := m.sendRaw(ctx, conn2, mustMarshal(t, &PingMessage{})); err != nil {
		t.Fatalf("write on conn2: %v", err)
	}
	m.waitForCount(1, 2*time.Second)
	got := m.receivedMessages()
	if countType(got, string(MsgPing)) != 1 {
		t.Errorf("after reconnect: ping count = %d, want 1", countType(got, string(MsgPing)))
	}
}

func mustMarshal(t *testing.T, msg any) []byte {
	t.Helper()
	frame, err := MarshalMessage(msg)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	return frame
}

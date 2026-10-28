package telemetry

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"net"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/coder/websocket"
	"go.opentelemetry.io/otel/sdk/trace/tracetest"
	"go.opentelemetry.io/otel/trace"
)

// fakeOtelEngine is the engine's /otel endpoint: it records the handshake and every
// binary frame. hold, when set, blocks handshakes until it is closed.
type fakeOtelEngine struct {
	srv  *httptest.Server
	url  string
	hold chan struct{}

	mu       sync.Mutex
	cond     *sync.Cond
	frames   [][]byte
	paths    []string
	headers  []http.Header
	closed   int
	statuses []websocket.StatusCode
}

func newFakeOtelEngine(t *testing.T, hold bool) *fakeOtelEngine {
	t.Helper()
	f := &fakeOtelEngine{}
	f.cond = sync.NewCond(&f.mu)
	if hold {
		f.hold = make(chan struct{})
	}
	f.srv = httptest.NewServer(http.HandlerFunc(f.handle))
	f.url = "ws" + strings.TrimPrefix(f.srv.URL, "http")
	t.Cleanup(func() {
		f.release()
		f.srv.Close()
	})
	return f
}

func (f *fakeOtelEngine) release() {
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.hold != nil {
		close(f.hold)
		f.hold = nil
	}
}

func (f *fakeOtelEngine) handle(w http.ResponseWriter, r *http.Request) {
	f.mu.Lock()
	hold := f.hold
	f.paths = append(f.paths, r.URL.Path)
	f.headers = append(f.headers, r.Header.Clone())
	f.cond.Broadcast()
	f.mu.Unlock()
	if hold != nil {
		<-hold
	}
	conn, err := websocket.Accept(w, r, nil)
	if err != nil {
		return
	}
	conn.SetReadLimit(64 << 20)
	for {
		typ, data, err := conn.Read(context.Background())
		if err != nil {
			f.mu.Lock()
			f.closed++
			f.statuses = append(f.statuses, websocket.CloseStatus(err))
			f.cond.Broadcast()
			f.mu.Unlock()
			return
		}
		if typ == websocket.MessageBinary {
			f.mu.Lock()
			f.frames = append(f.frames, data)
			f.cond.Broadcast()
			f.mu.Unlock()
		}
	}
}

// waitFor blocks until cond holds (under mu) or the timeout passes.
func (f *fakeOtelEngine) waitFor(t *testing.T, what string, cond func() bool) {
	t.Helper()
	deadline := time.Now().Add(5 * time.Second)
	go func() {
		time.Sleep(5 * time.Second)
		f.cond.Broadcast()
	}()
	f.mu.Lock()
	defer f.mu.Unlock()
	for !cond() {
		if time.Now().After(deadline) {
			t.Fatalf("timed out waiting for %s", what)
		}
		f.cond.Wait()
	}
}

func (f *fakeOtelEngine) snapshot() [][]byte {
	f.mu.Lock()
	defer f.mu.Unlock()
	return append([][]byte(nil), f.frames...)
}

func fastReconnect(maxRetries int) Reconnect {
	return Reconnect{InitialDelay: time.Millisecond, MaxDelay: 5 * time.Millisecond, BackoffMultiplier: 1, MaxRetries: maxRetries}
}

func waitState(t *testing.T, c *Connection, want ConnState) {
	t.Helper()
	deadline := time.Now().Add(5 * time.Second)
	for c.State() != want {
		if time.Now().After(deadline) {
			t.Fatalf("state %v, want %v", c.State(), want)
		}
		time.Sleep(time.Millisecond)
	}
}

// Frames sent before the socket opens are queued and flushed in order on connect, with
// the signal prefix, over a binary frame, to the /otel path with the handshake headers.
func TestConnectionQueuesUntilConnected(t *testing.T) {
	f := newFakeOtelEngine(t, true)
	endpoint, _ := AppendOtelPath(f.url)
	c := NewConnection(endpoint, http.Header{"Authorization": {"Bearer tok"}}, fastReconnect(-1))
	defer func() { _ = c.Shutdown(context.Background()) }()

	f.waitFor(t, "handshake", func() bool { return len(f.paths) == 1 })
	if c.State() != StateConnecting {
		t.Fatalf("state %v while the handshake is held", c.State())
	}
	for _, body := range []string{"one", "two", "three"} {
		if err := c.Send(PrefixLogs, []byte(body)); err != nil {
			t.Fatal(err)
		}
	}
	f.release()
	f.waitFor(t, "3 frames", func() bool { return len(f.frames) == 3 })
	for i, want := range []string{"LOGSone", "LOGStwo", "LOGSthree"} {
		if got := string(f.snapshot()[i]); got != want {
			t.Errorf("frame %d = %q, want %q", i, got, want)
		}
	}
	if f.paths[0] != "/otel" || f.headers[0].Get("Authorization") != "Bearer tok" {
		t.Errorf("handshake path %q headers %v", f.paths[0], f.headers[0])
	}
}

func TestConnectionPendingQueueIsBounded(t *testing.T) {
	f := newFakeOtelEngine(t, true)
	c := NewConnection(f.url, nil, fastReconnect(-1))
	defer func() { _ = c.Shutdown(context.Background()) }()
	f.waitFor(t, "handshake", func() bool { return len(f.paths) == 1 })
	silence(t)
	for i := 0; i < MaxPendingMessages+5; i++ {
		_ = c.Send(PrefixTraces, []byte{byte(i)})
	}
	c.mu.Lock()
	n, first := len(c.pending), c.pending[0]
	c.mu.Unlock()
	if n != MaxPendingMessages || first[len(PrefixTraces)] != 5 {
		t.Errorf("pending %d, oldest kept %d; want %d and the 5 oldest dropped", n, first[len(PrefixTraces)], MaxPendingMessages)
	}
}

// silence mutes the connection's log lines for the test.
func silence(t *testing.T) {
	old := logf
	logf = func(string, ...any) {}
	t.Cleanup(func() { logf = old })
}

func closedPortURL(t *testing.T) string {
	t.Helper()
	l, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	addr := l.Addr().String()
	_ = l.Close()
	return "ws://" + addr + "/otel"
}

// After MaxRetries the connection fails: callbacks run, sends and exports are refused.
func TestConnectionFailsAfterMaxRetries(t *testing.T) {
	silence(t)
	c := NewConnection(closedPortURL(t), nil, fastReconnect(1))
	defer func() { _ = c.Shutdown(context.Background()) }()
	failed := make(chan struct{})
	var once sync.Once
	c.OnFailed(func() { once.Do(func() { close(failed) }) })
	select {
	case <-failed:
	case <-time.After(5 * time.Second):
		t.Fatal("OnFailed not called")
	}
	if c.State() != StateFailed {
		t.Errorf("state %v", c.State())
	}
	if err := c.Send(PrefixTraces, nil); !errors.Is(err, ErrConnectionFailed) {
		t.Errorf("Send after failure = %v", err)
	}
	exp := NewSpanExporter(c)
	stub := tracetest.SpanStub{Name: "s", SpanContext: trace.NewSpanContext(trace.SpanContextConfig{TraceID: fixtureTraceID, SpanID: fixtureSpanID})}
	if err := exp.ExportSpans(context.Background(), tracetest.SpanStubs{stub}.Snapshots()); err == nil || !strings.Contains(err.Error(), "connection failed: dropping spans") {
		t.Errorf("export after failure = %v", err)
	}
	// OnFailed registered after the failure runs at once.
	ran := false
	c.OnFailed(func() { ran = true })
	if !ran {
		t.Error("late OnFailed not run")
	}
}

// BeginShutdown without an open socket drops what is queued and refuses new frames.
func TestBeginShutdownWhileDisconnected(t *testing.T) {
	f := newFakeOtelEngine(t, true)
	c := NewConnection(f.url, nil, fastReconnect(-1))
	f.waitFor(t, "handshake", func() bool { return len(f.paths) == 1 })
	exp := NewLogExporter(c)
	if err := exp.core.export([]byte("queued")); err != nil {
		t.Fatal(err)
	}
	_ = c.Send(PrefixLogs, []byte("queued"))
	c.BeginShutdown()
	if err := c.Send(PrefixLogs, nil); !errors.Is(err, ErrConnectionShutdown) {
		t.Errorf("Send after BeginShutdown = %v", err)
	}
	if err := exp.core.export([]byte("x")); err == nil || !strings.Contains(err.Error(), "shut down: dropping logs") {
		t.Errorf("export after BeginShutdown = %v", err)
	}
	exp.core.mu.Lock()
	queued := len(exp.core.pending)
	exp.core.mu.Unlock()
	c.mu.Lock()
	pending := len(c.pending)
	c.mu.Unlock()
	if queued != 0 || pending != 0 {
		t.Errorf("exporter queue %d, connection queue %d after BeginShutdown", queued, pending)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	f.release()
	if err := c.Shutdown(ctx); err != nil {
		t.Errorf("Shutdown: %v", err)
	}
}

// An exporter queues while the socket is not open and flushes on connect.
func TestExporterFlushesQueueOnConnect(t *testing.T) {
	f := newFakeOtelEngine(t, true)
	c := NewConnection(f.url, nil, fastReconnect(-1))
	defer func() { _ = c.Shutdown(context.Background()) }()
	f.waitFor(t, "handshake", func() bool { return len(f.paths) == 1 })
	exp := NewSpanExporter(c)
	stub := tracetest.SpanStub{Name: "queued", SpanContext: trace.NewSpanContext(trace.SpanContextConfig{TraceID: fixtureTraceID, SpanID: fixtureSpanID})}
	if err := exp.ExportSpans(context.Background(), tracetest.SpanStubs{stub}.Snapshots()); err != nil {
		t.Fatal(err)
	}
	f.release()
	f.waitFor(t, "queued span frame", func() bool { return len(f.frames) == 1 })
	frame := f.snapshot()[0]
	if !bytes.HasPrefix(frame, []byte("OTLP{\"resourceSpans\":")) || !bytes.Contains(frame, []byte(`"name":"queued"`)) {
		t.Errorf("frame %s", frame)
	}
}

// Shutdown closes the socket with a normal closure and stops the goroutine.
func TestConnectionShutdownClosesSocket(t *testing.T) {
	f := newFakeOtelEngine(t, false)
	c := NewConnection(f.url, nil, fastReconnect(-1))
	waitState(t, c, StateConnected)
	if err := c.Send(PrefixMetrics, []byte("{}")); err != nil {
		t.Fatal(err)
	}
	if err := c.Shutdown(context.Background()); err != nil {
		t.Fatal(err)
	}
	f.waitFor(t, "close", func() bool { return f.closed == 1 })
	if f.statuses[0] != websocket.StatusNormalClosure || string(f.snapshot()[0]) != "MTRC{}" {
		t.Errorf("close status %v frames %q", f.statuses[0], f.snapshot())
	}
	select {
	case <-c.done:
	default:
		t.Error("connection goroutine still running")
	}
	if c.State() != StateDisconnected || !c.IsShuttingDown() {
		t.Errorf("state %v shuttingDown %v", c.State(), c.IsShuttingDown())
	}
}

// The connection reconnects after the engine drops it.
func TestConnectionReconnects(t *testing.T) {
	silence(t)
	accepted := make(chan *websocket.Conn, 2)
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		conn, err := websocket.Accept(w, r, nil)
		if err != nil {
			return
		}
		accepted <- conn
		_, _, _ = conn.Read(context.Background())
	}))
	defer srv.Close()
	c := NewConnection("ws"+strings.TrimPrefix(srv.URL, "http"), nil, fastReconnect(-1))
	defer func() { _ = c.Shutdown(context.Background()) }()
	connected := make(chan struct{}, 1)
	c.OnConnected(func() {
		select {
		case connected <- struct{}{}:
		default:
		}
	})

	waitConnected := func() *websocket.Conn {
		t.Helper()
		timer := time.NewTimer(5 * time.Second)
		defer timer.Stop()
		var conn *websocket.Conn
		// Client-side StateConnected does not mean the server handler has
		// published its socket yet. Synchronize with both sides explicitly.
		select {
		case conn = <-accepted:
		case <-timer.C:
			t.Fatal("timed out waiting for the server to accept a connection")
		}
		select {
		case <-connected:
		case <-timer.C:
			t.Fatal("timed out waiting for OnConnected")
		}
		return conn
	}

	first := waitConnected()
	_ = first.CloseNow()
	_ = waitConnected()
}

// engineRefusal is the text frame the engine's /otel handler sends before closing a
// connection it refuses (Engine::handle_otel, RBAC rejection).
const engineRefusal = `{"type":"error","error":{"code":"FORBIDDEN","message":"telemetry refused for this worker"}}`

// captureLogs records the connection's log lines for the test.
func captureLogs(t *testing.T) func() []string {
	t.Helper()
	var mu sync.Mutex
	var lines []string
	old := logf
	logf = func(format string, args ...any) {
		mu.Lock()
		lines = append(lines, fmt.Sprintf(format, args...))
		mu.Unlock()
	}
	t.Cleanup(func() { logf = old })
	return func() []string {
		mu.Lock()
		defer mu.Unlock()
		return append([]string(nil), lines...)
	}
}

// Review round 3, finding D: a text frame from the peer is logged and discarded, and does
// not stall Shutdown. coder/websocket's CloseRead answered it with a Close that waited up
// to 15s for its own goroutine.
func TestConnectionUnexpectedTextFrame(t *testing.T) {
	logs := captureLogs(t)
	sent := make(chan struct{}, 1)
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		conn, err := websocket.Accept(w, r, nil)
		if err != nil {
			return
		}
		_ = conn.Write(context.Background(), websocket.MessageText, []byte(engineRefusal))
		sent <- struct{}{}
		// Keep reading, so the close handshake is answered.
		for {
			if _, _, err := conn.Read(context.Background()); err != nil {
				return
			}
		}
	}))
	defer srv.Close()

	c := NewConnection("ws"+strings.TrimPrefix(srv.URL, "http")+"/otel", nil, fastReconnect(-1))
	t.Cleanup(func() {
		ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
		defer cancel()
		_ = c.Shutdown(ctx) // idempotent; stops the reconnect loop if the test fails early
	})
	// fastReconnect(-1) retries forever: a connection that never lands must fail the
	// test, not block it until the suite timeout.
	select {
	case <-sent:
	case <-time.After(5 * time.Second):
		t.Fatalf("the fake /otel endpoint never got the connection (state %v)", c.State())
	}
	deadline := time.Now().Add(3 * time.Second)
	for !strings.Contains(strings.Join(logs(), "\n"), "telemetry refused for this worker") {
		if time.Now().After(deadline) {
			t.Fatalf("the engine's text frame was not logged: %q", logs())
		}
		time.Sleep(5 * time.Millisecond)
	}
	if c.State() != StateConnected {
		t.Errorf("state %v after a text frame, want connected", c.State())
	}

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	start := time.Now()
	if err := c.Shutdown(ctx); err != nil {
		t.Fatalf("Shutdown: %v", err)
	}
	if d := time.Since(start); d > 2*time.Second {
		t.Fatalf("Shutdown took %v after a text frame from the peer", d)
	}
}

// Finding D: Shutdown is bounded by its ctx even when the peer never answers the close
// handshake (it does not read): cancelling lifeCtx ends it, where CloseNow waited.
func TestConnectionShutdownBoundedByContext(t *testing.T) {
	silence(t)
	stop := make(chan struct{})
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		conn, err := websocket.Accept(w, r, nil)
		if err != nil {
			return
		}
		_ = conn.Write(context.Background(), websocket.MessageText, []byte(engineRefusal))
		<-stop // never reads: the close handshake is never answered
		_ = conn.CloseNow()
	}))
	defer srv.Close()
	defer close(stop)

	c := NewConnection("ws"+strings.TrimPrefix(srv.URL, "http"), nil, fastReconnect(-1))
	waitState(t, c, StateConnected)
	time.Sleep(20 * time.Millisecond) // let the text frame arrive

	ctx, cancel := context.WithTimeout(context.Background(), 300*time.Millisecond)
	defer cancel()
	start := time.Now()
	_ = c.Shutdown(ctx)
	if d := time.Since(start); d > 2*time.Second {
		t.Fatalf("Shutdown with a 300ms ctx took %v", d)
	}
}

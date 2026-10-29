package iii

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/coder/websocket"
)

// V4: CallRequest.Payload is the invocation's data; the deprecated Data is used only
// when Payload is nil; neither is sent as {}.
func TestCallRequestPayload(t *testing.T) {
	m := newMockEngine(t)
	c := connectClient(t, m)
	ctx := context.Background()
	for _, tc := range []struct {
		fn   string
		req  CallRequest
		want string
	}{
		{"p::payload", CallRequest{Payload: json.RawMessage(`{"a":1}`)}, `{"a":1}`},
		{"p::data", CallRequest{Data: json.RawMessage(`{"b":2}`)}, `{"b":2}`},
		{"p::both", CallRequest{Payload: json.RawMessage(`{"p":true}`), Data: json.RawMessage(`{"d":true}`)}, `{"p":true}`},
		{"p::none", CallRequest{}, `{}`},
	} {
		req := tc.req
		req.FunctionID = tc.fn
		req.Action = VoidAction()
		if _, err := c.Call(ctx, req); err != nil {
			t.Fatalf("%s: %v", tc.fn, err)
		}
		got := m.waitFor(func(msgs []map[string]json.RawMessage) bool { return indexOfFunctionID(msgs, tc.fn) >= 0 }, 3*time.Second)
		i := indexOfFunctionID(got, tc.fn)
		if i < 0 {
			t.Fatalf("%s: no invokefunction frame", tc.fn)
		}
		if data := sortedJSON(t, got[i]["data"]); data != tc.want {
			t.Errorf("%s: data = %s, want %s", tc.fn, data, tc.want)
		}
	}
}

// V5, V7: ConnectionState (and its deprecated alias State) and WorkerID, the engine's
// id from WorkerRegistered, empty before the first one and kept after a disconnect.
func TestConnectionStateAndWorkerID(t *testing.T) {
	m := newMockEngine(t)
	var ack atomic.Bool
	ack.Store(true)
	greetWorker(m, &ack)
	c := New(m.url)
	t.Cleanup(func() { _ = c.Close() })
	if c.ConnectionState() != StateDisconnected || c.WorkerID() != "" {
		t.Fatalf("before Connect: state %q, worker id %q", c.ConnectionState(), c.WorkerID())
	}
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	if err := c.Connect(ctx); err != nil {
		t.Fatal(err)
	}
	if err := c.WaitUntilRegistered(ctx); err != nil {
		t.Fatal(err)
	}
	if c.ConnectionState() != StateConnected || c.State() != c.ConnectionState() {
		t.Errorf("connected: ConnectionState %q, State %q", c.ConnectionState(), c.State())
	}
	if got := c.WorkerID(); got != "w-1" {
		t.Errorf("WorkerID = %q, want w-1", got)
	}
	_ = c.Close()
	if c.ConnectionState() != StateDisconnected {
		t.Errorf("after Close: ConnectionState %q", c.ConnectionState())
	}
	if got := c.WorkerID(); got != "w-1" {
		t.Errorf("after Close: WorkerID = %q, want w-1 (kept)", got)
	}
}

// V6: Shutdown cancels pending calls, closes the socket with a normal closure, and is
// idempotent (with Close too).
func TestShutdownIsGraceful(t *testing.T) {
	m := newMockEngine(t)
	c := connectClient(t, m) // the engine never replies
	workerRegisterFrame(t, m)
	pending := make(chan error, 1)
	go func() {
		_, err := c.Call(context.Background(), CallRequest{FunctionID: "svc::silent", Timeout: time.Minute})
		pending <- err
	}()
	m.waitFor(func(msgs []map[string]json.RawMessage) bool { return indexOfFunctionID(msgs, "svc::silent") >= 0 }, 3*time.Second)

	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	if err := c.Shutdown(ctx); err != nil {
		t.Fatalf("Shutdown: %v", err)
	}
	select {
	case err := <-pending:
		if !errors.Is(err, ErrNotConnected) {
			t.Errorf("pending Call error = %v, want ErrNotConnected", err)
		}
	case <-time.After(3 * time.Second):
		t.Fatal("pending Call not cancelled by Shutdown")
	}
	if c.ConnectionState() != StateDisconnected {
		t.Errorf("state = %q, want disconnected", c.ConnectionState())
	}
	if status, ended := m.closeStatusOf(0, 3*time.Second); !ended || status != websocket.StatusNormalClosure {
		t.Errorf("close status = %v (ended %v), want StatusNormalClosure", status, ended)
	}
	if err := c.Shutdown(ctx); err != nil {
		t.Errorf("second Shutdown: %v, want nil", err)
	}
	if err := c.Close(); err != nil {
		t.Errorf("Close after Shutdown: %v", err)
	}
	if _, err := c.Call(context.Background(), CallRequest{FunctionID: "svc::late", Timeout: time.Second}); !errors.Is(err, ErrNotConnected) {
		t.Errorf("Call after Shutdown: %v, want ErrNotConnected", err)
	}
}

// V6: Close drops the socket without a closing handshake.
func TestCloseDropsTheSocket(t *testing.T) {
	m := newMockEngine(t)
	c := connectClient(t, m)
	workerRegisterFrame(t, m)
	_ = c.Close()
	if status, ended := m.closeStatusOf(0, 3*time.Second); !ended || status == websocket.StatusNormalClosure {
		t.Errorf("close status = %v (ended %v), want no close frame", status, ended)
	}
}

// V6: a ctx that is already done cuts the shutdown short: ctx.Err() is returned and the
// client is shut down anyway; a client never started shuts down with nil.
func TestShutdownWithDoneContext(t *testing.T) {
	m := newMockEngine(t)
	c := connectClient(t, m)
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if err := c.Shutdown(ctx); !errors.Is(err, context.Canceled) {
		t.Errorf("Shutdown(done ctx) = %v, want context.Canceled", err)
	}
	if c.ConnectionState() != StateDisconnected {
		t.Errorf("state = %q, want disconnected", c.ConnectionState())
	}
	if _, ended := m.closeStatusOf(0, 3*time.Second); !ended {
		t.Error("socket still open after Shutdown")
	}

	idle := New("ws://127.0.0.1:0")
	if err := idle.Shutdown(context.Background()); err != nil {
		t.Errorf("Shutdown of a client never started: %v", err)
	}
}

// silentEngine accepts worker connections and never reads from them, so a close
// handshake is never answered (a hung or partitioned engine).
func silentEngine(t *testing.T) string {
	t.Helper()
	stop := make(chan struct{})
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		conn, err := websocket.Accept(w, r, nil)
		if err != nil {
			return
		}
		<-stop // never reads
		_ = conn.CloseNow()
	}))
	t.Cleanup(func() {
		close(stop)
		srv.Close()
	})
	return "ws" + strings.TrimPrefix(srv.URL, "http")
}

func connectSilent(t *testing.T) *Client {
	t.Helper()
	c := New(silentEngine(t))
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	if err := c.Connect(ctx); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = c.Close() })
	return c
}

// V6 (review round 6, finding 1): an engine that never answers the close handshake
// cannot hold Shutdown past its ctx (coder/websocket would wait its own 5s).
func TestShutdownIsBoundedWhenTheEngineIgnoresTheCloseHandshake(t *testing.T) {
	c := connectSilent(t)
	ctx, cancel := context.WithTimeout(context.Background(), 300*time.Millisecond)
	defer cancel()
	start := time.Now()
	err := c.Shutdown(ctx)
	elapsed := time.Since(start)
	if !errors.Is(err, context.DeadlineExceeded) {
		t.Errorf("Shutdown = %v, want context.DeadlineExceeded", err)
	}
	if elapsed > time.Second {
		t.Errorf("Shutdown with a 300ms ctx took %v", elapsed)
	}
	if c.ConnectionState() != StateDisconnected {
		t.Errorf("state = %q, want disconnected", c.ConnectionState())
	}
}

// V6 (review round 6, finding 2): a Close called while Shutdown waits on an engine that
// does not answer cuts the Shutdown short instead of waiting for it.
func TestCloseCutsARunningShutdownShort(t *testing.T) {
	c := connectSilent(t)
	shutdownErr := make(chan error, 1)
	go func() { shutdownErr <- c.Shutdown(context.Background()) }()
	time.Sleep(200 * time.Millisecond) // Shutdown is now waiting for the close handshake

	start := time.Now()
	if err := c.Close(); err != nil {
		t.Fatalf("Close: %v", err)
	}
	if d := time.Since(start); d > time.Second {
		t.Errorf("Close during Shutdown took %v", d)
	}
	select {
	case err := <-shutdownErr:
		if !errors.Is(err, context.Canceled) {
			t.Errorf("Shutdown = %v, want context.Canceled (cut short by Close)", err)
		}
	case <-time.After(time.Second):
		t.Fatal("Shutdown still running after Close returned")
	}
}

// V6: Shutdown is bounded by ctx when the writer does not drain.
func TestShutdownIsBoundedWhenTheWriterIsWedged(t *testing.T) {
	c, _ := wedgedClient(t)
	c.reporter.push(testItem("w::fn", 1), []Capture{CaptureInput})
	c.reporter.recordLocal("w::fn", true, 1)
	ctx, cancel := context.WithTimeout(context.Background(), 100*time.Millisecond)
	defer cancel()
	start := time.Now()
	done := make(chan error, 1)
	go func() { done <- c.Shutdown(ctx) }()
	select {
	case err := <-done:
		if !errors.Is(err, context.DeadlineExceeded) {
			t.Errorf("Shutdown = %v, want context.DeadlineExceeded", err)
		}
	case <-time.After(3 * time.Second):
		t.Fatal("Shutdown hangs behind a wedged writer")
	}
	if d := time.Since(start); d > time.Second {
		t.Errorf("Shutdown took %s", d)
	}
}

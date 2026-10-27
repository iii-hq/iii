package iii

import (
	"context"
	"errors"
	"slices"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/coder/websocket"
)

// A5: WithInvocationTimeout bounds a Trigger whose request has no Timeout.
func TestWithInvocationTimeoutBoundsTrigger(t *testing.T) {
	m := newMockEngine(t)
	c := connectClient(t, m, WithInvocationTimeout(150*time.Millisecond)) // the engine never replies

	start := time.Now()
	_, err := c.Trigger(context.Background(), TriggerRequest{FunctionID: "svc::silent"})
	elapsed := time.Since(start)
	if !errors.Is(err, ErrTimeout) {
		t.Fatalf("error = %v, want ErrTimeout", err)
	}
	if elapsed < 150*time.Millisecond || elapsed > 5*time.Second {
		t.Errorf("timed out after %v, want about 150ms", elapsed)
	}
}

// A5: an explicit TriggerRequest.Timeout wins over WithInvocationTimeout.
func TestTriggerTimeoutOverridesInvocationTimeout(t *testing.T) {
	m := newMockEngine(t)
	c := connectClient(t, m, WithInvocationTimeout(time.Minute))

	start := time.Now()
	_, err := c.Trigger(context.Background(), TriggerRequest{FunctionID: "svc::silent", Timeout: 100 * time.Millisecond})
	if !errors.Is(err, ErrTimeout) {
		t.Fatalf("error = %v, want ErrTimeout", err)
	}
	if elapsed := time.Since(start); elapsed > 5*time.Second {
		t.Errorf("timed out after %v, want about 100ms", elapsed)
	}
}

// A5: the default is DefaultInvocationTimeout, and a non-positive option keeps it.
func TestInvocationTimeoutDefault(t *testing.T) {
	for name, opts := range map[string][]Option{
		"none":     nil,
		"zero":     {WithInvocationTimeout(0)},
		"negative": {WithInvocationTimeout(-time.Second)},
	} {
		c := New("ws://127.0.0.1:0", opts...)
		if got := c.triggerTimeout(0); got != DefaultInvocationTimeout {
			t.Errorf("%s: timeout = %v, want %v", name, got, DefaultInvocationTimeout)
		}
	}
	c := New("ws://127.0.0.1:0", WithInvocationTimeout(2*time.Second))
	if got := c.triggerTimeout(0); got != 2*time.Second {
		t.Errorf("timeout = %v, want 2s", got)
	}
	if got := c.triggerTimeout(time.Second); got != time.Second {
		t.Errorf("explicit timeout = %v, want 1s", got)
	}
}

// stateRecorder collects the states a connection-state listener receives.
type stateRecorder struct {
	mu     sync.Mutex
	states []ConnectionState
	added  chan struct{}
}

func newStateRecorder() *stateRecorder { return &stateRecorder{added: make(chan struct{}, 64)} }

func (r *stateRecorder) record(s ConnectionState) {
	r.mu.Lock()
	r.states = append(r.states, s)
	r.mu.Unlock()
	r.added <- struct{}{}
}

func (r *stateRecorder) snapshot() []ConnectionState {
	r.mu.Lock()
	defer r.mu.Unlock()
	return slices.Clone(r.states)
}

// waitFor waits until the recorded states end with want (or the timeout) and returns
// the whole sequence.
func (r *stateRecorder) waitFor(t *testing.T, want []ConnectionState) []ConnectionState {
	t.Helper()
	deadline := time.After(3 * time.Second)
	for {
		if got := r.snapshot(); len(got) >= len(want) && slices.Equal(got[len(got)-len(want):], want) {
			return got
		}
		select {
		case <-r.added:
		case <-deadline:
			got := r.snapshot()
			t.Fatalf("states = %v, want them to end with %v", got, want)
			return got
		}
	}
}

// A6: listeners get the current state, then every change in order, across a reconnect;
// a listener may call the client; unsubscribe stops the delivery.
func TestOnConnectionStateChangeOrderAndUnsubscribe(t *testing.T) {
	m := newMockEngine(t)
	c := New(m.url, fastReconnect())
	t.Cleanup(func() { _ = c.Close() })

	first := newStateRecorder()
	var calledState atomic.Bool
	unsubscribeFirst := c.OnConnectionStateChange(func(s ConnectionState) {
		_ = c.State() // must not deadlock: listeners run outside the client's locks
		calledState.Store(true)
		first.record(s)
	})
	first.waitFor(t, []ConnectionState{StateDisconnected})

	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	if err := c.Connect(ctx); err != nil {
		t.Fatal(err)
	}
	first.waitFor(t, []ConnectionState{StateDisconnected, StateConnecting, StateConnected})

	// A listener added later starts from the current state.
	second := newStateRecorder()
	unsubscribeSecond := c.OnConnectionStateChange(second.record)
	second.waitFor(t, []ConnectionState{StateConnected})

	m.closeActiveConnection()
	got := first.waitFor(t, []ConnectionState{StateDisconnected, StateReconnecting, StateConnected})
	want := []ConnectionState{StateDisconnected, StateConnecting, StateConnected, StateDisconnected, StateReconnecting, StateConnected}
	if !slices.Equal(got, want) {
		t.Errorf("first listener states = %v, want %v", got, want)
	}
	if got, want := second.waitFor(t, []ConnectionState{StateConnected}), []ConnectionState{StateConnected, StateDisconnected, StateReconnecting, StateConnected}; !slices.Equal(got, want) {
		t.Errorf("second listener states = %v, want %v", got, want)
	}
	if !calledState.Load() {
		t.Error("listener never ran")
	}

	// After unsubscribing, the first listener gets nothing more; the second still does.
	unsubscribeFirst()
	unsubscribeFirst() // idempotent
	before := len(first.snapshot())
	_ = c.Close()
	second.waitFor(t, []ConnectionState{StateDisconnected})
	time.Sleep(50 * time.Millisecond)
	if after := first.snapshot(); len(after) != before {
		t.Errorf("unsubscribed listener got %v after unsubscribe", after[before:])
	}
	unsubscribeSecond()
}

// A6: a panicking listener is contained and the others keep receiving.
func TestOnConnectionStateChangeContainsPanics(t *testing.T) {
	logs := captureLog(t)
	c := New("ws://127.0.0.1:0")
	t.Cleanup(func() { _ = c.Close() })
	unsubscribe := c.OnConnectionStateChange(func(ConnectionState) { panic("boom") })
	defer unsubscribe()
	r := newStateRecorder()
	c.OnConnectionStateChange(r.record)
	r.waitFor(t, []ConnectionState{StateDisconnected})
	c.setState(StateConnecting)
	r.waitFor(t, []ConnectionState{StateDisconnected, StateConnecting})
	if logs.count("connection state listener panicked") == 0 {
		t.Errorf("panic not logged:\n%s", logs.String())
	}
}

// greetWorker makes the mock acknowledge every connection with workerregistered while
// ack is true, as the engine does.
func greetWorker(m *mockEngine, ack *atomic.Bool) {
	m.onConnect = func(conn *websocket.Conn) {
		if !ack.Load() {
			return
		}
		ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
		defer cancel()
		_ = m.send(ctx, conn, &WorkerRegisteredMessage{WorkerID: "w-1", ReattachToken: "tok"})
	}
}

// A6: WaitUntilRegistered returns nil once the engine acknowledged the worker, and waits
// again for the acknowledgement of the next connection after a disconnect.
func TestWaitUntilRegisteredSucceedsPerConnection(t *testing.T) {
	m := newMockEngine(t)
	var ack atomic.Bool
	ack.Store(true)
	greetWorker(m, &ack)
	c := RegisterWorker(m.url, fastReconnect())
	t.Cleanup(func() { _ = c.Close() })

	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	if err := c.WaitUntilRegistered(ctx); err != nil {
		t.Fatalf("WaitUntilRegistered: %v", err)
	}
	// The mock acknowledges on connect, before the client's own replay arrives: let the
	// first connection's replay finish before the connection is dropped.
	workerRegisterFrame(t, m)

	// The next connection is not acknowledged until the engine says so.
	ack.Store(false)
	reconnectAndWaitReplay(t, m)
	short, cancelShort := context.WithTimeout(context.Background(), 100*time.Millisecond)
	defer cancelShort()
	if err := c.WaitUntilRegistered(short); !errors.Is(err, context.DeadlineExceeded) {
		t.Fatalf("before the new acknowledgement: error = %v, want context.DeadlineExceeded", err)
	}

	done := make(chan error, 1)
	go func() { done <- c.WaitUntilRegistered(ctx) }()
	m.mu.Lock()
	conn := m.active
	m.mu.Unlock()
	if err := m.send(ctx, conn, &WorkerRegisteredMessage{WorkerID: "w-2"}); err != nil {
		t.Fatal(err)
	}
	select {
	case err := <-done:
		if err != nil {
			t.Fatalf("after the acknowledgement: %v", err)
		}
	case <-time.After(3 * time.Second):
		t.Fatal("WaitUntilRegistered did not return after workerregistered")
	}
}

// A6: a terminal rejection is returned as FatalError.
func TestWaitUntilRegisteredReturnsFatalError(t *testing.T) {
	m := newMockEngine(t)
	m.onConnect = func(conn *websocket.Conn) {
		ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
		defer cancel()
		_ = m.send(ctx, conn, &RegistrationRejectedMessage{
			Code:          "WORKER_NAMESPACE_CONFLICT",
			Namespace:     "orders",
			WorkerName:    "state",
			OwnerWorkerID: "owner-123",
		})
	}
	c := RegisterWorker(m.url, WithName("state"), WithNamespace("orders"))
	t.Cleanup(func() { _ = c.Close() })

	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	err := c.WaitUntilRegistered(ctx)
	var rejected *RegistrationRejectedError
	if !errors.As(err, &rejected) || rejected.Code != "WORKER_NAMESPACE_CONFLICT" {
		t.Fatalf("error = %v, want the WORKER_NAMESPACE_CONFLICT rejection", err)
	}
	if err != c.FatalError() {
		t.Errorf("error = %v, want FatalError() %v", err, c.FatalError())
	}

	// A client refused at construction reports it without being started.
	blank := New(m.url, WithNamespace(" "))
	if err := blank.WaitUntilRegistered(ctx); err == nil || err != blank.FatalError() {
		t.Errorf("blank namespace: error = %v, want FatalError() %v", err, blank.FatalError())
	}
}

// A6: cancellation returns ctx.Err(); a client not started, or closed, returns
// ErrNotConnected.
func TestWaitUntilRegisteredCancelAndNotConnected(t *testing.T) {
	m := newMockEngine(t) // never acknowledges the worker
	c := New(m.url)
	t.Cleanup(func() { _ = c.Close() })

	if err := c.WaitUntilRegistered(context.Background()); !errors.Is(err, ErrNotConnected) {
		t.Errorf("not started: error = %v, want ErrNotConnected", err)
	}

	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	if err := c.Connect(ctx); err != nil {
		t.Fatal(err)
	}
	waitCtx, cancelWait := context.WithCancel(context.Background())
	done := make(chan error, 1)
	go func() { done <- c.WaitUntilRegistered(waitCtx) }()
	time.Sleep(50 * time.Millisecond)
	cancelWait()
	select {
	case err := <-done:
		if !errors.Is(err, context.Canceled) {
			t.Errorf("cancelled: error = %v, want context.Canceled", err)
		}
	case <-time.After(3 * time.Second):
		t.Fatal("WaitUntilRegistered ignored cancellation")
	}

	go func() { done <- c.WaitUntilRegistered(context.Background()) }()
	time.Sleep(50 * time.Millisecond)
	_ = c.Close()
	select {
	case err := <-done:
		if !errors.Is(err, ErrNotConnected) {
			t.Errorf("closed: error = %v, want ErrNotConnected", err)
		}
	case <-time.After(3 * time.Second):
		t.Fatal("WaitUntilRegistered did not return on Close")
	}
}

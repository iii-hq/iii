package iii

import (
	"context"
	"fmt"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/coder/websocket"
)

func TestClientSendsWebSocketKeepalivePing(t *testing.T) {
	pingReceived := make(chan struct{}, 1)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		conn, err := websocket.Accept(w, r, &websocket.AcceptOptions{
			OnPingReceived: func(context.Context, []byte) bool {
				select {
				case pingReceived <- struct{}{}:
				default:
				}
				return true
			},
		})
		if err != nil {
			return
		}
		defer conn.CloseNow()
		for {
			if _, _, err := conn.Read(context.Background()); err != nil {
				return
			}
		}
	}))
	t.Cleanup(server.Close)

	client := New("ws" + strings.TrimPrefix(server.URL, "http"))
	client.keepaliveInterval = 20 * time.Millisecond
	client.keepaliveTimeout = 100 * time.Millisecond
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	if err := client.Connect(ctx); err != nil {
		t.Fatalf("Connect: %v", err)
	}
	t.Cleanup(func() { _ = client.Close() })

	select {
	case <-pingReceived:
	case <-ctx.Done():
		t.Fatal("client did not send a WebSocket keepalive ping")
	}
}

func TestKeepaliveTimeoutReconnects(t *testing.T) {
	var connections atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		conn, err := websocket.Accept(w, r, &websocket.AcceptOptions{
			OnPingReceived: func(context.Context, []byte) bool { return false },
		})
		if err != nil {
			return
		}
		connections.Add(1)
		defer conn.CloseNow()
		for {
			if _, _, err := conn.Read(context.Background()); err != nil {
				return
			}
		}
	}))
	t.Cleanup(server.Close)

	client := New(
		"ws"+strings.TrimPrefix(server.URL, "http"),
		WithReconnectConfig(ReconnectConfig{
			InitialDelay:      10 * time.Millisecond,
			MaxDelay:          20 * time.Millisecond,
			BackoffMultiplier: 2,
			JitterFactor:      0,
			MaxRetries:        -1,
		}),
	)
	client.keepaliveInterval = 20 * time.Millisecond
	client.keepaliveTimeout = 50 * time.Millisecond
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	if err := client.Connect(ctx); err != nil {
		t.Fatalf("Connect: %v", err)
	}
	t.Cleanup(func() { _ = client.Close() })

	deadline := time.NewTimer(time.Second)
	defer deadline.Stop()
	for connections.Load() < 2 {
		select {
		case <-deadline.C:
			t.Fatalf("connections = %d, want a reconnect after keepalive timeout", connections.Load())
		case <-time.After(5 * time.Millisecond):
		}
	}
}

func TestReconnectBackoffResetsAfterWorkerRegistration(t *testing.T) {
	var mu sync.Mutex
	var connectedAt []time.Time
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		conn, err := websocket.Accept(w, r, nil)
		if err != nil {
			return
		}
		defer conn.CloseNow()

		mu.Lock()
		connectionNumber := len(connectedAt) + 1
		connectedAt = append(connectedAt, time.Now())
		mu.Unlock()

		readCtx, cancelRead := context.WithTimeout(context.Background(), time.Second)
		_, _, err = conn.Read(readCtx)
		cancelRead()
		if err != nil {
			return
		}
		frame, err := MarshalMessage(&WorkerRegisteredMessage{
			WorkerID: fmt.Sprintf("worker-%d", connectionNumber),
		})
		if err != nil {
			return
		}
		writeCtx, cancelWrite := context.WithTimeout(context.Background(), time.Second)
		err = conn.Write(writeCtx, websocket.MessageText, frame)
		cancelWrite()
		if err != nil {
			return
		}
		time.Sleep(20 * time.Millisecond)
	}))
	t.Cleanup(server.Close)

	client := New(
		"ws"+strings.TrimPrefix(server.URL, "http"),
		WithReconnectConfig(ReconnectConfig{
			InitialDelay:      10 * time.Millisecond,
			MaxDelay:          400 * time.Millisecond,
			BackoffMultiplier: 40,
			JitterFactor:      0,
			MaxRetries:        -1,
		}),
	)
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	if err := client.Connect(ctx); err != nil {
		t.Fatalf("Connect: %v", err)
	}
	t.Cleanup(func() { _ = client.Close() })

	deadline := time.NewTimer(time.Second)
	defer deadline.Stop()
	for {
		mu.Lock()
		count := len(connectedAt)
		mu.Unlock()
		if count >= 3 {
			break
		}
		select {
		case <-deadline.C:
			t.Fatalf("connections = %d, want three registered connections", count)
		case <-time.After(5 * time.Millisecond):
		}
	}

	mu.Lock()
	secondReconnectDelay := connectedAt[2].Sub(connectedAt[1])
	mu.Unlock()
	if secondReconnectDelay >= 200*time.Millisecond {
		t.Fatalf("second reconnect delay = %s, want registration to reset the backoff", secondReconnectDelay)
	}
}

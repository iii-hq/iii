package iii

import (
	"context"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/coder/websocket"
)

// Review round 3, finding D: the engine's /otel endpoint sends a text frame (its RBAC
// refusal) and keeps the socket open. Client.Close must stay well within
// otelShutdownTimeout: with coder/websocket's CloseRead, that frame made the telemetry
// socket's Close wait 15s for its own read goroutine, and Client.Close waited behind it.
func TestOtelCloseBoundedWhenTheEngineSendsText(t *testing.T) {
	m := newMockEngine(t)
	sent := make(chan struct{}, 1)
	mux := http.NewServeMux()
	mux.HandleFunc("/otel", func(w http.ResponseWriter, r *http.Request) {
		conn, err := websocket.Accept(w, r, nil)
		if err != nil {
			return
		}
		_ = conn.Write(context.Background(), websocket.MessageText,
			[]byte(`{"type":"error","error":{"code":"FORBIDDEN","message":"telemetry refused for this worker"}}`))
		select {
		case sent <- struct{}{}:
		default:
		}
		for {
			if _, _, err := conn.Read(context.Background()); err != nil {
				return
			}
		}
	})
	mux.HandleFunc("/", m.handle)
	srv := httptest.NewServer(mux)
	defer srv.Close()

	c := New("ws"+strings.TrimPrefix(srv.URL, "http"), otelOn(OtelConfig{}))
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	if err := c.Connect(ctx); err != nil {
		t.Fatalf("Connect: %v", err)
	}
	select {
	case <-sent:
	case <-time.After(3 * time.Second):
		t.Fatal("the client's telemetry never connected to /otel")
	}
	time.Sleep(100 * time.Millisecond) // the client reads the text frame

	start := time.Now()
	_ = c.Close()
	if d := time.Since(start); d > 5*time.Second {
		t.Fatalf("Client.Close took %v after the engine sent a text frame on /otel", d)
	}
}

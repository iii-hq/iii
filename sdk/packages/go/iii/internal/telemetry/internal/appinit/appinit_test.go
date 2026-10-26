package appinit_test

import (
	"context"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/coder/websocket"
	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/propagation"
	"go.opentelemetry.io/otel/trace"

	iii "github.com/iii-hq/iii/sdk/packages/go/iii"
	"github.com/iii-hq/iii/sdk/packages/go/iii/internal/telemetry/internal/appinit"
)

// Review round 2, finding B, the reviewer's scenario: the application installs its
// tracer provider and a value-type propagator in the init() of a package that the Go
// runtime initializes before internal/telemetry (see the appinit package comment). The
// client must keep them while its telemetry runs and after Close.
func TestApplicationGlobalsSetInInitAreKept(t *testing.T) {
	kept := func(when string) {
		t.Helper()
		if otel.GetTracerProvider() != trace.TracerProvider(appinit.TracerProvider) {
			t.Errorf("%s: the application's tracer provider was replaced by %T", when, otel.GetTracerProvider())
		}
		if otel.GetTextMapPropagator() != propagation.TextMapPropagator(appinit.Propagator) {
			t.Errorf("%s: the application's propagator was replaced by %T", when, otel.GetTextMapPropagator())
		}
	}
	kept("before the client starts")

	// A minimal engine: the worker endpoint and the /otel endpoint, both draining frames.
	var otelConns atomic.Int32
	drain := func(w http.ResponseWriter, r *http.Request) {
		conn, err := websocket.Accept(w, r, nil)
		if err != nil {
			return
		}
		conn.SetReadLimit(64 << 20)
		for {
			if _, _, err := conn.Read(context.Background()); err != nil {
				return
			}
		}
	}
	mux := http.NewServeMux()
	mux.HandleFunc("/otel", func(w http.ResponseWriter, r *http.Request) {
		otelConns.Add(1)
		drain(w, r)
	})
	mux.HandleFunc("/", drain)
	srv := httptest.NewServer(mux)
	defer srv.Close()

	on := true
	c := iii.New("ws"+strings.TrimPrefix(srv.URL, "http"), iii.WithOtel(iii.OtelConfig{Enabled: &on}))
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	if err := c.Connect(ctx); err != nil {
		t.Fatalf("Connect: %v", err)
	}
	// The client's telemetry really runs: it dialled the /otel endpoint.
	deadline := time.Now().Add(3 * time.Second)
	for otelConns.Load() == 0 {
		if time.Now().After(deadline) {
			t.Fatal("the client's telemetry never connected to /otel")
		}
		time.Sleep(5 * time.Millisecond)
	}
	kept("while the client runs")

	_ = c.Close()
	kept("after Close")
}

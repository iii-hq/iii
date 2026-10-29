//go:build integration

package iii_test

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"testing"
	"time"

	iii "github.com/iii-hq/iii/sdk/packages/go/iii"
)

// Mirrors sdk/packages/rust/iii/tests/api_triggers.rs: observe the http worker's
// http::on-request with a route and confirm a real HTTP request routes through the
// engine to the Go handler and back. This is the full inbound path (http worker ->
// call through the route's observer -> handler -> invocationresult -> HTTP response),
// and exercises the HTTP request and response shapes (request under "body", response
// as {status_code, body}).

func TestHTTPRouteRoundtrip(t *testing.T) {
	c := connect(t)

	if _, err := c.RegisterFunction("test::http::go::greet", func(ctx context.Context, data json.RawMessage) (any, error) {
		var req struct {
			Body struct {
				Name string `json:"name"`
			} `json:"body"`
		}
		if err := json.Unmarshal(data, &req); err != nil {
			return nil, err
		}
		return map[string]any{
			"status_code": 200,
			"body":        map[string]string{"greeting": "hi " + req.Body.Name},
		}, nil
	}); err != nil {
		t.Fatalf("RegisterFunction: %v", err)
	}

	// Derive a unique path from the test name + a nonce, so retries, `go test -count`,
	// and concurrent runs against the same engine don't collide on the http worker's
	// route table.
	path := "/test-go-greet-" + uniqueSuffix(t)
	route, err := c.Observe(iii.Source{
		FunctionID: "http::on-request",
		Args:       iii.Args{"method": "POST", "path": path},
	}, "test::http::go::greet")
	if err != nil {
		t.Fatalf("Observe: %v", err)
	}
	if err := route.Ready(ctxFor(t, 10*time.Second)); err != nil {
		t.Fatalf("route not active: %v", err)
	}

	// POST to the engine's HTTP API and assert the handler's response comes back.
	reqBody := bytes.NewBufferString(`{"name":"world"}`)
	httpReq, _ := http.NewRequest(http.MethodPost, engineHTTPURL()+path, reqBody)
	httpReq.Header.Set("Content-Type", "application/json")

	resp, err := httpClient().Do(httpReq)
	if err != nil {
		t.Fatalf("HTTP POST %s: %v", path, err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d, want 200", resp.StatusCode)
	}
	body, _ := io.ReadAll(resp.Body)
	var out struct {
		Greeting string `json:"greeting"`
	}
	if err := json.Unmarshal(body, &out); err != nil {
		t.Fatalf("decode response: %v\nraw: %s", err, body)
	}
	if out.Greeting != "hi world" {
		t.Errorf("greeting = %q, want %q", out.Greeting, "hi world")
	}
}

// TestConflictingRouteStructureIsRejected mirrors the Python/Rust suites: two HTTP routes
// with identical structure but swapped path-param names must not crash the engine. The
// first route keeps serving and the http worker rejects the second observer with
// HTTP_ROUTE_CONFLICT.
func TestConflictingRouteStructureIsRejected(t *testing.T) {
	c := connect(t)

	handler := func(_ context.Context, _ json.RawMessage) (any, error) {
		return map[string]any{"status_code": 200, "body": map[string]bool{"ok": true}}, nil
	}
	if _, err := c.RegisterFunction("test::api::conflict::a::go", handler); err != nil {
		t.Fatalf("RegisterFunction A: %v", err)
	}
	if _, err := c.RegisterFunction("test::api::conflict::b::go", handler); err != nil {
		t.Fatalf("RegisterFunction B: %v", err)
	}

	// Shared literal prefix (unique per run) so A and B collide structurally without
	// clashing with other runs against the same engine.
	base := "/test/go/conflict-" + uniqueSuffix(t)
	first, err := c.Observe(iii.Source{
		FunctionID: "http::on-request",
		Args:       iii.Args{"method": "GET", "path": base + "/:listId/:userId"},
	}, "test::api::conflict::a::go")
	if err != nil {
		t.Fatalf("Observe A: %v", err)
	}
	if err := first.Ready(ctxFor(t, 10*time.Second)); err != nil {
		t.Fatalf("route A not active: %v", err)
	}
	// Second route has the same shape with swapped param names -> conflict.
	second, err := c.Observe(iii.Source{
		FunctionID: "http::on-request",
		Args:       iii.Args{"method": "GET", "path": base + "/:userId/:listId"},
	}, "test::api::conflict::b::go")
	if err != nil {
		t.Fatalf("Observe B: %v", err)
	}
	var rej *iii.ObserveRejectionError
	if err := second.Ready(ctxFor(t, 10*time.Second)); !errors.As(err, &rej) || rej.Code != "HTTP_ROUTE_CONFLICT" {
		t.Fatalf("route B: %v, want HTTP_ROUTE_CONFLICT", err)
	}

	// The engine stayed alive and the first route still serves.
	resp, err := httpClient().Get(engineHTTPURL() + base + "/list1/user1")
	if err != nil {
		t.Fatalf("HTTP GET: %v", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d, want 200", resp.StatusCode)
	}
	body, _ := io.ReadAll(resp.Body)
	var out struct {
		OK bool `json:"ok"`
	}
	if err := json.Unmarshal(body, &out); err != nil {
		t.Fatalf("decode response: %v\nraw: %s", err, body)
	}
	if !out.OK {
		t.Errorf("ok = false, want true; raw: %s", body)
	}
}

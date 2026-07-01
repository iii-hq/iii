//go:build integration

package iii_test

import (
	"bytes"
	"context"
	"encoding/json"
	"io"
	"net/http"
	"testing"
	"time"

	iii "github.com/iii-hq/iii/sdk/packages/go/iii"
)

// Mirrors sdk/packages/rust/iii/tests/api_triggers.rs: register an HTTP trigger and
// confirm a real HTTP request routes through the engine to the Go handler and back. This
// is the full inbound path (engine -> invokefunction -> handler -> invocationresult ->
// HTTP response), and exercises the HTTP envelope (request under "body", response as
// {status_code, body}).

func TestHTTPTriggerRoundtrip(t *testing.T) {
	c := connect(t)

	if err := c.RegisterFunction("test::http::go::greet", func(ctx context.Context, data json.RawMessage) (any, error) {
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

	// Derive a unique path and trigger id from the test name + a nonce, so retries,
	// `go test -count`, and concurrent runs against the same engine don't collide on the
	// engine's global HTTP route registry.
	suffix := uniqueSuffix(t)
	path := "/test-go-greet-" + suffix
	if err := c.RegisterTrigger(
		"test-go-http-"+suffix,
		"http",
		"test::http::go::greet",
		json.RawMessage(`{"api_path":"`+path+`","http_method":"POST"}`),
		nil,
	); err != nil {
		t.Fatalf("RegisterTrigger: %v", err)
	}
	settle()

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
// first route keeps serving and the second is rejected (never becomes an active registered
// trigger).
func TestConflictingRouteStructureIsRejected(t *testing.T) {
	c := connect(t)

	handler := func(_ context.Context, _ json.RawMessage) (any, error) {
		return map[string]any{"status_code": 200, "body": map[string]bool{"ok": true}}, nil
	}
	if err := c.RegisterFunction("test::api::conflict::a::go", handler); err != nil {
		t.Fatalf("RegisterFunction A: %v", err)
	}
	if err := c.RegisterFunction("test::api::conflict::b::go", handler); err != nil {
		t.Fatalf("RegisterFunction B: %v", err)
	}

	// Shared literal prefix (unique per run) so A and B collide structurally without
	// clashing with other runs against the engine-global registry.
	suffix := uniqueSuffix(t)
	base := "test/go/conflict-" + suffix

	if err := c.RegisterTrigger(
		"test-go-conflict-a-"+suffix,
		"http",
		"test::api::conflict::a::go",
		json.RawMessage(`{"api_path":"`+base+`/:listId/:userId","http_method":"GET"}`),
		nil,
	); err != nil {
		t.Fatalf("RegisterTrigger A: %v", err)
	}
	// Second route has the same axum shape with swapped param names -> conflict.
	if err := c.RegisterTrigger(
		"test-go-conflict-b-"+suffix,
		"http",
		"test::api::conflict::b::go",
		json.RawMessage(`{"api_path":"`+base+`/:userId/:listId","http_method":"GET"}`),
		nil,
	); err != nil {
		t.Fatalf("RegisterTrigger B: %v", err)
	}
	settle()

	// Engine stayed alive and the first route still serves — no panic.
	resp, err := httpClient().Get(engineHTTPURL() + "/" + base + "/list1/user1")
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

	// Exactly one of the two routes survives: the engine rejects whichever conflicting
	// registration it processes second (wire order is not guaranteed), so the loser
	// never becomes an active registered trigger.
	registered := 0
	for _, fid := range []string{"test::api::conflict::a::go", "test::api::conflict::b::go"} {
		res, err := c.Trigger(ctxFor(t, 5*time.Second), iii.TriggerRequest{
			FunctionID: iii.FnListRegisteredTriggers,
			Data:       json.RawMessage(`{"function_id":"` + fid + `"}`),
		})
		if err != nil {
			t.Fatalf("engine::registered-triggers::list (%s): %v", fid, err)
		}
		var out struct {
			RegisteredTriggers []struct {
				ID string `json:"id"`
			} `json:"registered_triggers"`
		}
		if err := json.Unmarshal(res, &out); err != nil {
			t.Fatalf("decode registered-triggers list: %v\nraw: %s", err, res)
		}
		registered += len(out.RegisteredTriggers)
	}
	if registered != 1 {
		t.Errorf("exactly one conflicting route must be registered, found %d", registered)
	}
}

//go:build integration

package iii_test

import (
	"bytes"
	"context"
	"encoding/json"
	"io"
	"net/http"
	"testing"
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

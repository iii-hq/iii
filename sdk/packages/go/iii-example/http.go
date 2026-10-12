package main

import (
	"context"
	"encoding/json"
	"log"

	iii "github.com/iii-hq/iii/sdk/packages/go/iii"
)

// httpRequest is the subset of the engine's HTTP-trigger ApiRequest envelope this
// example reads. When a function is invoked via an "http" trigger, the engine wraps the
// request as { path, method, body, headers, ... } and the parsed request body lives
// under "body" — not at the top level. See engine/src/workers/rest_api/README.md.
type httpRequest struct {
	Body struct {
		Name string `json:"name"`
	} `json:"body"`
}

// httpResponse is the engine's HTTP-trigger ApiResponse envelope: the function must
// return { status_code, body } for the HTTP worker to build a response. Returning the
// payload directly would yield an empty HTTP body.
type httpResponse struct {
	StatusCode int `json:"status_code"`
	Body       any `json:"body"`
}

// setupHTTP registers hello::greet and exposes it at POST /greet. Mirrors
// http_example.rs.
func setupHTTP(client *iii.Client) {
	if err := client.RegisterFunction("hello::greet", func(ctx context.Context, data json.RawMessage) (any, error) {
		var req httpRequest
		if err := json.Unmarshal(data, &req); err != nil {
			return nil, err
		}
		name := req.Body.Name
		if name == "" {
			name = "world"
		}
		return httpResponse{
			StatusCode: 200,
			Body:       map[string]string{"message": "Hello, " + name + "!"},
		}, nil
	}); err != nil {
		log.Fatalf("register hello::greet: %v", err)
	}

	if err := client.RegisterTrigger(
		"hello-http",
		"http",
		"hello::greet",
		json.RawMessage(`{"api_path":"/greet","http_method":"POST"}`),
		nil,
	); err != nil {
		log.Fatalf("register http trigger: %v", err)
	}
}

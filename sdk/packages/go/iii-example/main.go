// Command example is a hello-world iii worker, mirroring the Node SDK README's
// hello-world (sdk/packages/node/iii/README.md). It registers a function, binds an HTTP
// trigger to it, invokes it once to show the round trip, then serves until interrupted
// so the HTTP trigger can be exercised against a running engine.
//
// Run it against a local engine:
//
//	iii --use-default-config   # in another terminal, engine on :49134 / :3111
//	go run .                   # from sdk/packages/go/iii-example
//	curl -X POST localhost:3111/greet -H 'Content-Type: application/json' -d '{"name":"world"}'
//
// The Content-Type header matters: the engine only parses a JSON body into the request
// envelope's "body" field, so a form-urlencoded request (curl's -d default) arrives with
// body=null and the handler falls back to its default.
package main

import (
	"context"
	"encoding/json"
	"log"
	"os"
	"os/signal"
	"syscall"
	"time"

	iii "github.com/iii-hq/iii/sdk/packages/go/iii"
)

// httpRequest is the subset of the engine's HTTP-trigger ApiRequest envelope this
// example reads. When a function is invoked via an "http" trigger, the engine wraps the
// request as { path, method, body, headers, ... } and the parsed request body lives
// under "body" — not at the top level. See engine/src/workers/rest_api/README.md.
type httpRequest struct {
	Body greetBody `json:"body"`
}

type greetBody struct {
	Name string `json:"name"`
}

// httpResponse is the engine's HTTP-trigger ApiResponse envelope: the function must
// return { status_code, body } for the HTTP worker to build a response. Returning the
// payload directly would yield an empty HTTP body.
type httpResponse struct {
	StatusCode int `json:"status_code"`
	Body       any `json:"body"`
}

func main() {
	url := os.Getenv("III_URL")
	if url == "" {
		url = iii.DefaultEngineURL
	}

	// RegisterWorker creates the client and starts connecting in the background, matching
	// registerWorker in the Node/Rust SDKs. Register functions/triggers below; they are
	// sent once Connect reports the first connection.
	client := iii.RegisterWorker(url)

	// Register the function. The handler receives the raw JSON payload (and optional
	// per-invocation metadata, unused here) and returns any value, which the SDK marshals
	// into the invocation result. Because this function is exposed over HTTP, it speaks the
	// engine's HTTP envelope: read the request from req.Body and return an ApiResponse
	// { status_code, body }.
	if err := client.RegisterFunction("hello::greet", func(ctx context.Context, data, metadata json.RawMessage) (any, error) {
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
		log.Fatalf("register function: %v", err)
	}

	// Bind an HTTP trigger so the engine exposes the function at POST /greet.
	if err := client.RegisterTrigger(
		"hello-http",
		"http",
		"hello::greet",
		json.RawMessage(`{"api_path":"/greet","http_method":"POST"}`),
		nil,
	); err != nil {
		log.Fatalf("register trigger: %v", err)
	}

	// Connect and run the registration handshake.
	connectCtx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	if err := client.Connect(connectCtx); err != nil {
		log.Fatalf("connect to engine at %s: %v", url, err)
	}
	defer client.Close()
	log.Printf("worker connected to %s", url)

	// Invoke the function once over the socket to show the await round trip. The payload
	// uses the same HTTP envelope the handler reads (body.name), so this matches what the
	// engine sends for a POST /greet.
	callCtx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	result, err := client.Trigger(callCtx, iii.TriggerRequest{
		FunctionID: "hello::greet",
		Data:       json.RawMessage(`{"body":{"name":"world"}}`),
	})
	if err != nil {
		log.Printf("trigger hello::greet: %v", err)
	} else {
		log.Printf("hello::greet returned: %s", result)
	}

	// Serve until interrupted so the HTTP trigger stays live.
	log.Print("serving; POST to /greet on the engine's HTTP port, or Ctrl-C to exit")
	sig := make(chan os.Signal, 1)
	signal.Notify(sig, os.Interrupt, syscall.SIGTERM)
	<-sig
	log.Print("shutting down")
}

// Command example is a hello-world iii worker, mirroring the Node SDK README's
// hello-world (sdk/packages/node/iii/README.md). It registers a function, routes
// POST /greet to it by observing the http worker's http::on-request, observes its own
// function with a second one, calls it once to show the round trip, then serves until
// interrupted so the route can be exercised against a running engine.
//
// Run it against a local engine:
//
//	iii --use-default-config   # in another terminal, engine on :49134 / :3111
//	go run .                   # from sdk/packages/go/iii-example
//	curl -X POST localhost:3111/greet -H 'Content-Type: application/json' -d '{"name":"world"}'
//
// The http worker parses a request body sent as application/json into the request's
// "body" field; for any other content type "body" is null, and the handler greets
// "world".
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

// httpRequest is the subset of the HTTP request this example reads. The target of an
// http::on-request observer receives the request as { path, method, body, headers, ... };
// body is the parsed JSON body.
type httpRequest struct {
	Body greetBody `json:"body"`
}

type greetBody struct {
	Name string `json:"name"`
}

// httpResponse is the HTTP response: the function returns { status_code, body }, which
// the http worker sends as the response.
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
	// registerWorker in the Node/Rust SDKs. Register functions and observers below; they
	// are sent once Connect reports the first connection.
	client := iii.RegisterWorker(url)

	// Register the function. The handler receives the raw JSON payload and returns any
	// value, which the SDK marshals into the invocation result. Because this function is
	// exposed over HTTP, it reads the request from req.Body and returns
	// { status_code, body }.
	if _, err := client.RegisterFunction("hello::greet", func(ctx context.Context, data json.RawMessage) (any, error) {
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

	// Route POST /greet to the function: observe the http worker's http::on-request
	// with the route as args. The http worker calls hello::greet through the observer
	// and answers the request with its result.
	if _, err := client.Observe(iii.Source{
		FunctionID: "http::on-request",
		Args:       iii.Args{"method": "POST", "path": "/greet"},
	}, "hello::greet"); err != nil {
		log.Fatalf("observe http::on-request: %v", err)
	}

	// Every function is observable: log each greeting hello::greet returns.
	if _, err := client.RegisterFunction("hello::on-greeted", func(ctx context.Context, data json.RawMessage) (any, error) {
		log.Printf("greeted: %s", data)
		return nil, nil
	}); err != nil {
		log.Fatalf("register function: %v", err)
	}
	if _, err := client.Observe(iii.Source{FunctionID: "hello::greet", Deliver: iii.DeliverOutput}, "hello::on-greeted"); err != nil {
		log.Fatalf("observe hello::greet: %v", err)
	}

	// Connect and run the registration handshake.
	connectCtx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	if err := client.Connect(connectCtx); err != nil {
		log.Fatalf("connect to engine at %s: %v", url, err)
	}
	defer client.Close()
	log.Printf("worker connected to %s", url)

	// Call the function once over the socket to show the await round trip. The payload
	// has the request shape the handler reads (body.name), so this matches what the http
	// worker sends for a POST /greet.
	callCtx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	result, err := client.Call(callCtx, iii.CallRequest{
		FunctionID: "hello::greet",
		Payload:    json.RawMessage(`{"body":{"name":"world"}}`),
	})
	if err != nil {
		log.Printf("call hello::greet: %v", err)
	} else {
		log.Printf("hello::greet returned: %s", result)
	}

	// Serve until interrupted so the route stays live.
	log.Print("serving; POST to /greet on the engine's HTTP port, or Ctrl-C to exit")
	sig := make(chan os.Signal, 1)
	signal.Notify(sig, os.Interrupt, syscall.SIGTERM)
	<-sig
	log.Print("shutting down")
	// Graceful: buffered reports and telemetry get up to five seconds to reach the engine.
	shutdownCtx, cancelShutdown := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancelShutdown()
	if err := client.Shutdown(shutdownCtx); err != nil {
		log.Printf("shutdown cut short: %v", err)
	}
}

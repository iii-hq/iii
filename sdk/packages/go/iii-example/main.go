// Command example is a worker that demonstrates the iii Go SDK, mirroring the Rust
// iii-example crate: each feature lives in its own file with a setup(client) function,
// and main wires them together, connects, runs a couple of demo invocations, then serves
// until interrupted so the HTTP/cron/etc. triggers can fire against a running engine.
//
// Run it against a local engine:
//
//	iii --use-default-config   # in another terminal, engine on :49134 / :3111
//	go run .                   # from sdk/packages/go/iii-example
//	curl -X POST localhost:3111/greet -d '{"name":"world"}'
//
// Set III_URL to point at a non-default engine.
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

func main() {
	url := os.Getenv("III_URL")
	if url == "" {
		url = iii.DefaultEngineURL
	}

	// RegisterWorker creates the client and starts connecting in the background, matching
	// registerWorker in the Node/Rust SDKs. Register functions/triggers below; they are
	// sent once Connect reports the first connection.
	client := iii.RegisterWorker(url)

	// Register each feature's functions and triggers. Each setup mirrors a
	// *_example.rs file in the Rust iii-example crate.
	setupHTTP(client)        // HTTP-triggered function (the README hello-world)
	setupCron(client)        // built-in cron trigger
	setupTriggerType(client) // a custom trigger TYPE this worker implements
	setupLogger(client)      // engine log functions

	// Structured OpenTelemetry logging via the observability package. This is independent
	// of the engine connection — it honors OTEL_EXPORTER_OTLP_ENDPOINT (use the bundled
	// docker-compose stack) or prints to stdout — so run it before connecting. The
	// returned shutdown flushes the log batch on exit.
	defer demoObservability(context.Background())()

	// Connect and run the registration handshake.
	connectCtx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	if err := client.Connect(connectCtx); err != nil {
		log.Fatalf("connect to engine at %s: %v", url, err)
	}
	defer client.Close()
	log.Printf("worker connected to %s", url)

	// Demo 1: invoke the HTTP greet function over the socket (uses the HTTP envelope,
	// so it matches what the engine sends for a POST /greet).
	callCtx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	if res, err := client.Trigger(callCtx, iii.TriggerRequest{
		FunctionID: "hello::greet",
		Data:       json.RawMessage(`{"body":{"name":"world"}}`),
	}); err != nil {
		log.Printf("trigger hello::greet: %v", err)
	} else {
		log.Printf("hello::greet returned: %s", res)
	}

	// Demo 2: stream some bytes through a channel and read them back (channels.go).
	demoChannel(client)

	// Serve until interrupted so the registered triggers stay live.
	log.Print("serving; POST to /greet on the engine's HTTP port, or Ctrl-C to exit")
	sig := make(chan os.Signal, 1)
	signal.Notify(sig, os.Interrupt, syscall.SIGTERM)
	<-sig
	log.Print("shutting down")
}

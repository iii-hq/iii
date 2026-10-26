package iii_test

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log"
	"time"

	iii "github.com/iii-hq/iii/sdk/packages/go/iii"
)

// Example_helloWorker is a complete hello-world worker: register a function, bind an HTTP
// trigger to it, and connect. The handler speaks the engine's HTTP-trigger envelope (the
// request body is under "body"; the response is {status_code, body}).
func Example_helloWorker() {
	client := iii.RegisterWorker(iii.DefaultEngineURL) // ws://127.0.0.1:49134

	greet, err := client.RegisterFunction("hello::greet", func(ctx context.Context, data json.RawMessage) (any, error) {
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
			"body":        map[string]string{"message": "Hello, " + req.Body.Name + "!"},
		}, nil
	})
	if err != nil {
		log.Fatal(err)
	}

	if _, err := client.RegisterTrigger(iii.RegisterTriggerInput{
		Type:       "http",
		FunctionID: greet.ID,
		Config:     json.RawMessage(`{"api_path":"/greet","http_method":"POST"}`),
	}); err != nil {
		log.Fatal(err)
	}

	if err := client.Connect(context.Background()); err != nil {
		log.Fatal(err)
	}
	defer client.Close()
}

// ExampleFunctionRef_Unregister removes a function through the handle its registration
// returned; a reconnect no longer registers it.
func ExampleFunctionRef_Unregister() {
	client := iii.RegisterWorker(iii.DefaultEngineURL)
	defer client.Close()

	ref, err := client.RegisterFunction("jobs::nightly", func(ctx context.Context, data json.RawMessage) (any, error) {
		return nil, nil
	})
	if err != nil {
		log.Fatal(err)
	}
	// Later, when the job is retired:
	if err := ref.Unregister(); err != nil {
		log.Fatal(err)
	}
}

// ExampleClient_Shutdown stops a worker gracefully on SIGTERM-style exits: buffered
// reports and telemetry get up to five seconds to reach the engine.
func ExampleClient_Shutdown() {
	client := iii.RegisterWorker(iii.DefaultEngineURL)

	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	if err := client.Shutdown(ctx); err != nil {
		log.Printf("shutdown cut short: %v", err)
	}
}

// ExampleClient_RegisterFunction_withMetadata attaches registration metadata via the
// optional options value; the handler signature is unchanged.
func ExampleClient_RegisterFunction_withMetadata() {
	client := iii.New(iii.DefaultEngineURL)

	handler := func(ctx context.Context, data json.RawMessage) (any, error) {
		return map[string]any{"ok": true}, nil
	}

	client.RegisterFunction("orders::create", handler, iii.RegisterFunctionOptions{
		Metadata: json.RawMessage(`{"owner":"billing-team","priority":"high"}`),
	})
}

// ExampleMetadataFromContext reads the optional per-invocation metadata sidecar from the
// handler's ctx; ok is false when the caller attached none.
func ExampleMetadataFromContext() {
	client := iii.New(iii.DefaultEngineURL)

	client.RegisterFunction("orders::create",
		func(ctx context.Context, data json.RawMessage) (any, error) {
			metadata, ok := iii.MetadataFromContext(ctx)
			return map[string]any{"ok": true, "has_metadata": ok, "metadata": metadata}, nil
		})
}

// ExampleClient_Trigger invokes a function and awaits its result, then shows the typed
// error handling: ErrTimeout for a missed deadline and InvocationError for a remote
// failure.
func ExampleClient_Trigger() {
	client := iii.RegisterWorker(iii.DefaultEngineURL)
	defer client.Close()

	res, err := client.Trigger(context.Background(), iii.TriggerRequest{
		FunctionID: "orders::create",
		Payload:    json.RawMessage(`{"item":"widget"}`),
	})
	switch {
	case errors.Is(err, iii.ErrTimeout):
		fmt.Println("timed out")
	case err != nil:
		var ie *iii.InvocationError
		if errors.As(err, &ie) {
			fmt.Println("remote error:", ie.Code)
		}
	default:
		fmt.Printf("result: %s\n", res)
	}
}

// greetRequest and greetResponse describe a function's contract; their JSON Schemas are
// inferred and advertised to the engine by RegisterFunctionTyped.
type greetRequest struct {
	Name string `json:"name" jsonschema:"required"`
}

type greetResponse struct {
	Message string `json:"message"`
}

// ExampleRegisterFunctionTyped registers a function whose request and response schemas
// are inferred from the Go types, the Go counterpart of the Rust SDK's JsonSchema derive.
func ExampleRegisterFunctionTyped() {
	client := iii.RegisterWorker(iii.DefaultEngineURL)
	defer client.Close()

	iii.RegisterFunctionTyped(client, "hello::greet",
		func(ctx context.Context, req greetRequest) (greetResponse, error) {
			return greetResponse{Message: "Hello, " + req.Name + "!"}, nil
		})
}

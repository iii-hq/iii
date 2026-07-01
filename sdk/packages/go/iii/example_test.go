package iii_test

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log"

	iii "github.com/iii-hq/iii/sdk/packages/go/iii"
)

// Example_helloWorker is a complete hello-world worker: register a function, bind an HTTP
// trigger to it, and connect. The handler speaks the engine's HTTP-trigger envelope (the
// request body is under "body"; the response is {status_code, body}).
func Example_helloWorker() {
	client := iii.RegisterWorker(iii.DefaultEngineURL) // ws://localhost:49134

	client.RegisterFunction("hello::greet", func(ctx context.Context, data, metadata json.RawMessage) (any, error) {
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

	client.RegisterTrigger("hello-http", "http", "hello::greet",
		json.RawMessage(`{"api_path":"/greet","http_method":"POST"}`), nil)

	if err := client.Connect(context.Background()); err != nil {
		log.Fatal(err)
	}
	defer client.Close()
}

// ExampleClient_Trigger invokes a function and awaits its result, then shows the typed
// error handling: ErrTimeout for a missed deadline and InvocationError for a remote
// failure.
func ExampleClient_Trigger() {
	client := iii.RegisterWorker(iii.DefaultEngineURL)
	defer client.Close()

	res, err := client.Trigger(context.Background(), iii.TriggerRequest{
		FunctionID: "orders::create",
		Data:       json.RawMessage(`{"item":"widget"}`),
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

	iii.RegisterFunctionTyped[greetRequest, greetResponse](client, "hello::greet",
		func(ctx context.Context, req greetRequest) (greetResponse, error) {
			return greetResponse{Message: "Hello, " + req.Name + "!"}, nil
		})
}

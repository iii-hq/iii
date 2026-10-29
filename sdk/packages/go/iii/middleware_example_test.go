package iii_test

import (
	"context"
	"encoding/json"
	"fmt"

	iii "github.com/iii-hq/iii/sdk/packages/go/iii"
)

// ExampleMiddlewareFunctionInput decodes the input the engine sends to an RBAC
// middleware function.
func ExampleMiddlewareFunctionInput() {
	raw := []byte(`{"function_id":"orders::create","payload":{"item":"book"},"action":null,"context":{"source":"browser"},"namespace":"default"}`)

	var in iii.MiddlewareFunctionInput
	if err := json.Unmarshal(raw, &in); err != nil {
		panic(err)
	}
	fmt.Println(in.FunctionID, in.Namespace, in.Action == nil)
	fmt.Println(string(in.Payload), string(in.Context))
	// Output:
	// orders::create default true
	// {"item":"book"} {"source":"browser"}
}

// ExampleMiddlewareFunctionInput_forward registers a middleware that stamps the session's
// auth context onto the payload and forwards the call to its target, keeping the
// caller's action and namespace.
func ExampleMiddlewareFunctionInput_forward() {
	client := iii.RegisterWorker(iii.DefaultEngineURL)
	defer client.Close()

	_, _ = iii.RegisterFunctionTyped(client, "auth::middleware",
		func(ctx context.Context, in iii.MiddlewareFunctionInput) (json.RawMessage, error) {
			var payload map[string]any
			if err := json.Unmarshal(in.Payload, &payload); err != nil {
				return nil, err
			}
			payload["_auth"] = in.Context
			data, err := json.Marshal(payload)
			if err != nil {
				return nil, err
			}
			namespace := in.Namespace
			if namespace == "" {
				namespace = "default" // absent means the engine's default namespace
			}
			return client.Call(ctx, iii.CallRequest{
				FunctionID: in.FunctionID,
				Payload:    data,
				Action:     in.Action,
				Namespace:  namespace,
			})
		})
}

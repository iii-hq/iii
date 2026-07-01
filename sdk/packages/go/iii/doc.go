// Package iii is a Go SDK for the iii worker protocol.
//
// A process becomes an iii "worker" by opening a single WebSocket to the engine and
// registering functions and triggers; the engine invokes those functions and the worker
// replies over the same socket. The SDK handles the connection lifecycle (connect,
// reconnect with backoff, offline buffering) so callers work in terms of functions and
// triggers, not frames.
//
// # Quick start
//
// Create a worker, register a function, bind a trigger, and connect:
//
//	client := iii.RegisterWorker(iii.DefaultEngineURL) // ws://localhost:49134
//
//	client.RegisterFunction("hello::greet", func(ctx context.Context, data json.RawMessage) (any, error) {
//		var req struct {
//			Body struct {
//				Name string `json:"name"`
//			} `json:"body"`
//		}
//		_ = json.Unmarshal(data, &req)
//		return map[string]any{
//			"status_code": 200,
//			"body":        map[string]string{"message": "Hello, " + req.Body.Name + "!"},
//		}, nil
//	})
//
//	client.RegisterTrigger("hello-http", "http", "hello::greet",
//		json.RawMessage(`{"api_path":"/greet","http_method":"POST"}`), nil)
//
//	if err := client.Connect(context.Background()); err != nil {
//		log.Fatal(err)
//	}
//	defer client.Close()
//
// # Invoking functions
//
// [Client.Trigger] invokes a function and, by default, awaits its result. The
// [TriggerRequest] Action field selects the delivery semantics: the default (nil) awaits
// the result, [VoidAction] is fire-and-forget, and [EnqueueAction] routes through a named
// queue. Errors are typed: [ErrTimeout] for a missed deadline, [ErrNotConnected] for a
// call cancelled by shutdown, and [InvocationError] (via errors.As) for a remote failure.
//
// # Schema inference
//
// [RegisterFunctionTyped] infers JSON Schemas for a function's request and response from
// the Go types and advertises them to the engine — the Go counterpart of the Rust SDK's
// JsonSchema derive.
//
// # Channels
//
// For bulk or streaming data, [Client.CreateChannel] opens a streaming data channel; see
// [ChannelWriter] and [ChannelReader].
//
// The package mirrors the iii Node and Rust SDKs; see the repository README at
// https://github.com/iii-hq/iii for the engine and the other SDKs.
package iii

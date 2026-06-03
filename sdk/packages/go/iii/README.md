# iii Go SDK

Go SDK for the [iii engine](https://github.com/iii-hq/iii). A process becomes an iii
**worker** by opening a single WebSocket to the engine and registering **functions** and
**triggers**; the engine invokes those functions and the worker replies over the same
socket.

[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](../../../LICENSE)

## Install

```bash
go get github.com/iii-hq/iii/sdk/packages/go/iii
```

Requires Go 1.23+.

## Hello World

```go
package main

import (
	"context"
	"encoding/json"
	"log"

	iii "github.com/iii-hq/iii/sdk/packages/go/iii"
)

func main() {
	// registerWorker-style entry point, matching the Node/Rust SDKs: creates the client
	// and starts connecting in the background.
	client := iii.RegisterWorker(iii.DefaultEngineURL) // ws://localhost:49134

	// Exposed over HTTP, so the handler speaks the engine's HTTP envelope: the request
	// body is under "body", and the response is { status_code, body }.
	client.RegisterFunction("hello::greet", func(ctx context.Context, data json.RawMessage) (any, error) {
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

	// RegisterWorker already started connecting; Connect blocks until the first
	// connection is established (or the context is cancelled).
	if err := client.Connect(context.Background()); err != nil {
		log.Fatal(err)
	}
	defer client.Close()

	result, err := client.Trigger(context.Background(), iii.TriggerRequest{
		FunctionID: "hello::greet",
		Data:       json.RawMessage(`{"body":{"name":"world"}}`),
	})
	if err != nil {
		log.Fatal(err)
	}
	log.Printf("result: %s", result) // {"status_code":200,"body":{"message":"Hello, world!"}}
}
```

> **HTTP trigger envelope.** When a function is reached over an `http` trigger, the
> engine wraps the request as `{ path, method, body, headers, … }` and expects the
> function to return `{ status_code, body }`. Functions invoked only over the socket can
> use any payload shape. See the engine's
> [REST API docs](../../../engine/src/workers/rest_api/README.md).

A complete, runnable version lives in the [`iii-example`](../iii-example) module.

## API

| Operation | Signature | Description |
| --- | --- | --- |
| Register worker | `iii.RegisterWorker(url, opts...) *Client` | Create a client and start connecting in the background (the `registerWorker` entry point). |
| Create | `iii.New(url, opts...) *Client` | Build a client without connecting (call `Connect` yourself). |
| Connect | `client.Connect(ctx) error` | Start the lifecycle if needed and block until connected. |
| Register function (typed) | `iii.RegisterFunctionTyped[Req, Resp](client, id, handler)` | Register a function with request/response schemas inferred from the types. |
| Register function | `client.RegisterFunction(id, handler) error` | Register a function the engine can invoke by name. |
| Register trigger | `client.RegisterTrigger(id, triggerType, functionID, config, metadata) error` | Bind a trigger (HTTP, cron, queue, …) to a function. |
| Register trigger type | `client.RegisterTriggerType(id, description, handler) error` | Implement a custom trigger type. |
| Invoke (await) | `client.Trigger(ctx, TriggerRequest{...})` | Invoke a function and wait for the result. |
| Invoke (fire-and-forget) | `client.Trigger(ctx, TriggerRequest{Action: iii.VoidAction()})` | Invoke without waiting. |
| Invoke (enqueue) | `client.Trigger(ctx, TriggerRequest{Action: iii.EnqueueAction("queue")})` | Route the invocation through a named queue. |
| Create channel | `client.CreateChannel(ctx, bufferSize)` | Open a streaming data channel (writer + reader ends). |
| Close | `client.Close() error` | Stop reconnecting, cancel pending calls, close the socket. |

`Register*` may be called before or after `Connect`; registrations are kept in memory and
(re)sent to the engine on every (re)connection.

### Registering functions

A handler receives the raw JSON payload and returns any value (marshaled into the
invocation result) or an error:

```go
client.RegisterFunction("orders::create", func(ctx context.Context, data json.RawMessage) (any, error) {
	var in struct {
		Item string `json:"item"`
	}
	if err := json.Unmarshal(data, &in); err != nil {
		return nil, err
	}
	return map[string]any{"id": "123", "item": in.Item}, nil
})
```

Returning an `*iii.InvocationError` preserves its `Code` on the wire; any other error is
reported with code `invocation_failed`. A handler panic is recovered and reported the
same way, so a caller gets an error rather than a timeout.

### Schema inference

For the engine (and typed callers / the dashboard) to know a function's contract, use
`RegisterFunctionTyped`, which infers the request and response JSON Schemas from the type
parameters and advertises them as `request_format` / `response_format`. This is the Go
counterpart of the Rust SDK's `#[derive(JsonSchema)]`. Go has no compile-time derive, so
inference is reflection-based (via [`invopop/jsonschema`](https://github.com/invopop/jsonschema),
the analog of Rust's `schemars`); use `json` and `jsonschema` struct tags to shape the
schema.

```go
type CreateOrderRequest struct {
	Item     string `json:"item" jsonschema:"required"`
	Quantity int    `json:"quantity" jsonschema:"minimum=1"`
}
type OrderResult struct {
	ID string `json:"id"`
}

iii.RegisterFunctionTyped[CreateOrderRequest, OrderResult](client, "orders::create",
	func(ctx context.Context, req CreateOrderRequest) (OrderResult, error) {
		return OrderResult{ID: "ord_123"}, nil
	})
// Advertised request_format:
// {"type":"object","properties":{"item":{"type":"string"},
//  "quantity":{"type":"integer","minimum":1}},"required":["item","quantity"]}
```

The handler works with the concrete `CreateOrderRequest` (the SDK unmarshals the payload)
and returns the typed `OrderResult`. Use `RegisterFunction` for schemaless functions or
when you want to send a hand-written schema. `iii.InferSchema[T]()` returns the schema for
a type if you want to inspect or reuse it.

### Invoking functions

```go
// Await the result.
res, err := client.Trigger(ctx, iii.TriggerRequest{
	FunctionID: "orders::create",
	Data:       json.RawMessage(`{"item":"widget"}`),
})

// Fire-and-forget (e.g. logging).
client.Trigger(ctx, iii.TriggerRequest{
	FunctionID: iii.LogInfo,
	Data:       json.RawMessage(`{"message":"page_view"}`),
	Action:     iii.VoidAction(),
})

// Enqueue through a named queue and await the receipt.
client.Trigger(ctx, iii.TriggerRequest{
	FunctionID: "jobs::process",
	Action:     iii.EnqueueAction("jobs"),
})
```

Errors are typed: `errors.Is(err, iii.ErrTimeout)` for a missed deadline,
`errors.Is(err, iii.ErrNotConnected)` for a call cancelled by shutdown, and
`errors.As(err, &ie)` for an `*iii.InvocationError` (carrying the remote `Code`,
`Message`, and `Stacktrace`).

## Connection behavior

- **Reconnect** with exponential backoff and jitter (start 1s, ×2, cap 30s, ±30%, retry
  forever). Override with `iii.WithReconnectConfig`.
- **Offline buffer**: invocations sent while disconnected are buffered and flushed on
  reconnect; registrations are replayed from the in-memory registries.
- **Worker metadata** is registered last on each connect, tagged `runtime: "go"`.

## Streaming channels

For bulk or streaming data, open a channel instead of passing everything through a single
invocation result. `CreateChannel` returns a writer and a reader, each backed by its own
WebSocket; pass either end's ref (`WriterRef` / `ReaderRef`) to another worker inside a
trigger payload, and it opens the opposite end.

```go
ch, err := client.CreateChannel(ctx, nil)
// stream bytes...
ch.Writer.Write(ctx, payload)        // binary frames (64 KiB chunks)
ch.Writer.SendMessage(ctx, progress) // discrete text message
ch.Writer.Close()                    // signals end-of-stream

// hand ch.ReaderRef to a processor function via Trigger; on that side:
refs, _ := iii.ExtractChannelRefs(data)
reader := iii.OpenReader(client.Address(), refs["reader"])
reader.OnMessage(func(m string) { /* text messages */ })
all, _ := reader.ReadAll(ctx)        // binary stream, until the writer closes
```

On a channel socket, **a text frame is a message** (`SendMessage` / `OnMessage`) and **a
binary frame is stream data** (`Write` / `ReadAll`) — disambiguated by WebSocket opcode,
no envelope. The writer ends the stream with `Close`; the reader sees that as EOF.

## Observability

The wire protocol carries W3C trace context (`traceparent` / `baggage`) across the
engine→worker→engine hop, and the SDK echoes it back on results. Wiring it to an
OpenTelemetry SDK (real spans around invocations) is planned as a follow-up; see
[iii-hq/iii#1719](https://github.com/iii-hq/iii/issues/1719).

## Resources

- [iii engine](https://github.com/iii-hq/iii)
- [Node SDK](../../node/iii) · [Rust SDK](../../rust/iii) — the references this SDK mirrors
- [Examples](https://github.com/iii-hq/iii-examples) (and a runnable Go worker in [`iii-example`](../iii-example))
- [Documentation](https://iii.dev/docs)
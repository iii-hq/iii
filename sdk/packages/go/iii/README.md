# iii Go SDK

Go SDK for the [iii engine](https://github.com/iii-hq/iii). A process becomes an iii
**worker** by opening a single WebSocket to the engine and registering **functions** and
**triggers**; the engine invokes those functions and the worker replies over the same
socket.

[![Go Reference](https://pkg.go.dev/badge/github.com/iii-hq/iii/sdk/packages/go/iii.svg)](https://pkg.go.dev/github.com/iii-hq/iii/sdk/packages/go/iii)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)

## Install

```bash
go get github.com/iii-hq/iii/sdk/packages/go/iii
```

Requires Go 1.25+.

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
	// and starts connecting in the background. An empty address means III_URL when it
	// is set, else iii.DefaultEngineURL (ws://127.0.0.1:49134).
	client := iii.RegisterWorker("")

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
> use any payload shape. See the [HTTP trigger documentation](https://iii.dev/docs).

A complete, runnable version lives in the [`iii-example`](../iii-example) module.

## API

| Operation | Signature | Description |
| --- | --- | --- |
| Register worker | `iii.RegisterWorker(url, opts...) *Client` | Create a client and start connecting in the background (the `registerWorker` entry point). |
| Create | `iii.New(url, opts...) *Client` | Build a client without connecting (call `Connect` yourself). |
| Engine address | `iii.EngineURLFromEnv() string` | `III_URL`, trimmed, else `DefaultEngineURL` (`ws://127.0.0.1:49134`). `New("")` and `RegisterWorker("")` use it. |
| Connect | `client.Connect(ctx) error` | Start the lifecycle if needed and block until connected. |
| Wait for registration | `client.WaitUntilRegistered(ctx) error` | Block until the engine acknowledged the worker on the current connection. |
| Connection state | `client.OnConnectionStateChange(listener) (unsubscribe func())` | Follow `connecting`, `connected`, `reconnecting`, `disconnected` and `failed`, in order. |
| Register function (typed) | `iii.RegisterFunctionTyped[Req, Resp](client, id, handler, opts...)` | Register a function with request/response schemas inferred from the types. |
| Register function | `client.RegisterFunction(id, handler, opts...) error` | Register a function the engine can invoke by name. |
| Register HTTP function | `client.RegisterHTTPFunction(id, iii.HTTPInvocationRef{...}, opts...) error` | Register a function hosted elsewhere that the engine invokes over HTTP. |
| Unregister | `client.UnregisterFunction(id) error` | Remove a function; reconnects no longer register it. |
| Read invocation metadata | `iii.MetadataFromContext(ctx) (json.RawMessage, bool)` | Read the optional per-invocation metadata sidecar inside a handler. |
| Observe | `client.Observe(iii.Source{FunctionID, Args, Filters, ...}, target, opts...) (*ObserverHandle, error)` | Invoke `target` on every matching invocation (or owner event) of a function. |
| Current invocation | `iii.InvocationFromContext(ctx) (iii.Invocation, bool)` | The invocation a handler runs in (id, caller, activation, observe depth). |
| Owner hooks | `RegisterFunctionOptions{Observe: &iii.ObserveHooks{...}}` | React when the function is observed; hand events to observers with `obs.Emit` / `obs.Call`. |
| Observed? | `client.IsObserved(id)`, `client.Observers(id)` | The active observers of a function this worker registered. |
| Register trigger (deprecated) | `client.RegisterTrigger(id, triggerType, functionID, config, metadata...) error` | Bind a trigger (HTTP, cron, queue, …) to a function. Use `Observe`. |
| Register trigger type (deprecated) | `client.RegisterTriggerType(id, description, handler, opts...) error` | Implement a custom trigger type. Use `ObserveHooks`. |
| Unregister trigger, trigger type | `client.UnregisterTrigger(id) error`, `client.UnregisterTriggerType(id) error` (deprecated) | Remove a trigger or a trigger type; reconnects no longer register it. |
| Invoke (await) | `client.Trigger(ctx, TriggerRequest{...})` | Invoke a function and wait for the result. |
| Invoke (fire-and-forget) | `client.Trigger(ctx, TriggerRequest{Action: iii.VoidAction()})` | Invoke without waiting. |
| Invoke (enqueue) | `client.Trigger(ctx, TriggerRequest{Action: iii.EnqueueAction("queue")})` | Route the invocation through a named queue. |
| Create channel | `client.CreateChannel(ctx, bufferSize)` | Open a streaming data channel (writer + reader ends). |
| Open channel end | `client.OpenReader(ref)`, `client.OpenWriter(ref)` | Open the end of a channel received in a payload, with the client's address and headers. |
| Close | `client.Close() error` | Stop reconnecting, cancel pending calls, close the socket. |

`Register*` may be called before or after `Connect`; registrations are kept in memory and
(re)sent to the engine on every (re)connection, until the matching `Unregister*` call.

### Options

Pass options to `New` or `RegisterWorker`:

| Option | Effect |
| --- | --- |
| `iii.WithName(name)` | Worker name reported to the engine (default `hostname:pid`; a non-empty `III_WORKER_NAME` wins). |
| `iii.WithNamespace(namespace)` | Namespace the worker registers into (else `III_NAMESPACE`, else the engine default). |
| `iii.WithDescription(text)` | One-line summary shown by `engine::workers::list` / `engine::workers::info`. |
| `iii.WithHeaders(h)` | Headers for the WebSocket handshake of the main connection and of channels. |
| `iii.WithReconnectConfig(cfg)` | Reconnect schedule (see [Connection behavior](#connection-behavior)). |
| `iii.WithInvocationTimeout(d)` | How long `Trigger` waits when `TriggerRequest.Timeout` is zero (default `iii.DefaultInvocationTimeout`, 30s). |
| `iii.WithTelemetry(iii.TelemetryOptions{...})` | Language, project name and framework reported in the worker metadata. |

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

Per-invocation metadata rides on the handler's `ctx` (so the `Handler` signature never
changes); read it with `MetadataFromContext` — `ok` is `false` when the caller attached
none:

```go
client.RegisterFunction("orders::create", func(ctx context.Context, data json.RawMessage) (any, error) {
	if metadata, ok := iii.MetadataFromContext(ctx); ok {
		_ = metadata // raw JSON sidecar, e.g. {"tenant":"acme"}
	}
	return map[string]any{"ok": true}, nil
})
```

Describe the function, advertise its request and response JSON Schemas, and attach
registration metadata with the optional options struct. They travel as `description`,
`request_format`, `response_format` and `metadata`; a format that is not valid JSON is
refused with an error.

```go
ordersHandler := func(ctx context.Context, data json.RawMessage) (any, error) {
	return map[string]any{"ok": true}, nil
}

client.RegisterFunction("orders::create", ordersHandler, iii.RegisterFunctionOptions{
	Description:    "Creates an order",
	RequestFormat:  json.RawMessage(`{"type":"object","properties":{"item":{"type":"string"}},"required":["item"]}`),
	ResponseFormat: json.RawMessage(`{"type":"object","properties":{"id":{"type":"string"}}}`),
	Metadata:       json.RawMessage(`{"owner":"billing-team","priority":"high"}`),
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
and returns the typed `OrderResult`. Typed handlers read the per-invocation metadata
sidecar from `ctx` with `iii.MetadataFromContext`, same as raw handlers. Use
`RegisterFunction` for schemaless functions. To send a hand-written schema, set
`RequestFormat` / `ResponseFormat` in `RegisterFunctionOptions`: with
`RegisterFunctionTyped` they replace the inferred schema, and `Description` passes
through. `iii.InferSchema[T]()` returns the schema for a type if you want to inspect or
reuse it.

### HTTP-invoked functions

A function hosted elsewhere (a Lambda, a Cloudflare Worker, any HTTP endpoint) can be
registered without a local handler: the engine calls it over HTTP. The method defaults
to `POST`. The auth constructors name the engine environment variables that hold the
secrets, never the secrets themselves.

```go
timeout := uint64(10000)
err := client.RegisterHTTPFunction("billing::charge", iii.HTTPInvocationRef{
	URL:       "https://billing.example.com/charge",
	TimeoutMs: &timeout,
	Headers:   map[string]string{"x-team": "billing"},
	Auth:      iii.BearerAuth("BILLING_TOKEN"), // or iii.HMACAuth("KEY"), iii.APIKeyAuth("x-api-key", "KEY")
}, iii.RegisterFunctionOptions{Description: "Charges a card"})
```

### Unregistering

`client.UnregisterFunction(id)`, `client.UnregisterTrigger(id)` and the deprecated
`client.UnregisterTriggerType(id)` remove a registration: the engine is told when the
client is connected, and later reconnects no longer replay it. `UnregisterFunction` also
releases the function's observers. Each returns an error when nothing is registered
under `id`.

### Describing custom trigger types

Pass one `RegisterTriggerTypeOptions` value to describe the configuration accepted
when binding a trigger and the payload delivered when it fires. Given a `handler`
that implements `iii.TriggerHandler`:

```go
err := client.RegisterTriggerType("messaging::events", "Filtered message events", handler,
	iii.RegisterTriggerTypeOptions{
		TriggerRequestFormat: json.RawMessage(`{
			"type":"object",
			"properties":{"chat_ids":{"type":"array","items":{"type":"string"}}},
			"additionalProperties":false
		}`),
		CallRequestFormat: json.RawMessage(`{
			"type":"object",
			"properties":{"message_id":{"type":"string"},"text":{"type":"string"}},
			"required":["message_id"]
		}`),
	})
if err != nil {
	log.Fatal(err)
}
```

`engine::triggers::info` exposes `TriggerRequestFormat` as `configuration_schema`
and `CallRequestFormat` as `request_schema`. Inspect a provider using
`{"id":"messaging::events","namespace":"your-worker-namespace"}`. You can also
generate either schema with `iii.InferSchema[T]()`.

Existing three-argument calls remain valid. Nil or empty schema bytes omit the
corresponding field. The SDK copies the bytes for registration and reconnect
replay, rejects malformed JSON or multiple options values, and preserves the
previous registration when an update is rejected. Schemas describe the contract;
handlers remain responsible for validating trigger configuration and payloads.
The SDK checks JSON syntax, not JSON Schema semantics.

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

An await or enqueue call waits `TriggerRequest.Timeout`, or, when that is zero, the
client's invocation timeout (`iii.WithInvocationTimeout`, default 30s).

Errors are typed: `errors.Is(err, iii.ErrTimeout)` for a missed deadline,
`errors.Is(err, iii.ErrNotConnected)` for a call cancelled by shutdown, and
`errors.As(err, &ie)` for an `*iii.InvocationError` (carrying the remote `Code`,
`Message`, and `Stacktrace`).

A call to a function this worker registered itself runs in-process once the engine
confirmed it (no frames on the wire unless someone observes it). The handler gets its
own copy of the payload, and errors and timeouts look as on the remote path. Pass
`Dispatch: iii.DispatchRemote` to go through the engine anyway, or register the
function with `LocalDispatch: &off` (`off := false`).

Inside a handler, pass the handler's `ctx` to `Trigger`: the call then carries the
handler as its caller (observers can filter on `caller.*`) and the observe depth the
engine's loop guard reads.

### Observing functions

Every function is observable. The target is a registered function; it receives the
`iii.Observed` envelope (`input`, `output` or `error`, `caller`, ...).

```go
client.RegisterFunction("alerts::big-order", func(ctx context.Context, data json.RawMessage) (any, error) {
	var ev iii.Observed
	_ = json.Unmarshal(data, &ev)
	return nil, notify(ev.Input, ev.Output)
})

observer, err := client.Observe(iii.Source{
	FunctionID: "orders::create",
	Filters:    iii.Filters{"input.body.total": iii.Op{"$gte": 500}},
}, "alerts::big-order")
if err == nil {
	err = observer.Ready(ctx) // an *iii.ObserveRejectionError carries the engine's code
}
// later
_ = observer.Dispose(ctx)
```

Engine-hosted sources are observed the same way, e.g. worker changes:

```go
client.Observe(iii.Source{FunctionID: iii.FnOnWorkersChanged, Deliver: iii.DeliverInput}, "ops::on-workers")
```

A function of your own can hand its events to the observers they belong to (owner
delivery), and react when one is added or removed. Here a worker's own scheduler
function routes each tick to the observers of its expression:

```go
var mu sync.Mutex
schedules := map[string][]*iii.Observer{} // expression -> observers

client.RegisterFunction("reports::on-tick", func(ctx context.Context, data json.RawMessage) (any, error) {
	var tick struct{ Expression string `json:"expression"` }
	_ = json.Unmarshal(data, &tick)
	mu.Lock()
	observers := append([]*iii.Observer(nil), schedules[tick.Expression]...)
	mu.Unlock()
	for _, obs := range observers {
		obs.Emit(ctx, data) // false when the observer's filters reject the tick
	}
	return nil, nil
}, iii.RegisterFunctionOptions{Observe: &iii.ObserveHooks{
	Delivery:   iii.DeliveryOwner,
	ArgsFormat: json.RawMessage(`{"type":"object","required":["expression"],"properties":{"expression":{"type":"string"}}}`),
	OnObserverAdded: func(ctx context.Context, obs *iii.Observer) error {
		expr, _ := obs.Args["expression"].(string)
		mu.Lock()
		schedules[expr] = append(schedules[expr], obs)
		mu.Unlock()
		return nil // or iii.ObserveRejection("INVALID_EXPRESSION", "...")
	},
	OnObserverRemoved: func(ctx context.Context, obs *iii.Observer) {
		// remove obs from schedules
	},
}})
```

`obs.Call(ctx, payload)` invokes the observer's target through the engine and returns
its result (an HTTP router answers with it). The filter evaluator is the `filter`
subpackage (`github.com/iii-hq/iii/sdk/packages/go/iii/filter`), the same language
the engine runs.

### Trigger APIs (deprecated)

`RegisterTrigger` and `RegisterTriggerType` keep working in 0.25 but are deprecated:
they log a warning once per process. Use `client.Observe(source, target)` and
`RegisterFunction(id, handler, iii.RegisterFunctionOptions{Observe: ...})` instead.
Built-in trigger providers (HTTP, cron, queue, ...) keep `RegisterTrigger` until they
ship their source functions. See the
[migration guide](https://iii.dev/docs/upgrading/from-0-24-x).

## Connection behavior

- **Engine address**: an explicit address wins, then `III_URL`, then
  `ws://127.0.0.1:49134`, as in the other SDKs (`iii.EngineURLFromEnv`).
- **Reconnect** with exponential backoff and jitter (start 1s, ×2, cap 30s, ±30%, retry
  forever). Override with `iii.WithReconnectConfig`.
- **Offline buffer**: invocations sent while disconnected are buffered and flushed on
  reconnect; registrations are replayed from the in-memory registries.
- **Worker metadata** is registered last on each connect, tagged `runtime: "go"`, with
  `isolation` (`III_ISOLATION`, else `null`) and `telemetry`: `language` (from `LANG`),
  `project_name` (the last element of the module path in the nearest `go.mod`) and
  `framework` (`iii-go`). Override them with `iii.WithTelemetry`.

Follow the connection, or wait for the engine to accept the worker before calling out:

```go
client := iii.RegisterWorker("")
unsubscribe := client.OnConnectionStateChange(func(state iii.ConnectionState) {
	log.Printf("engine link: %s", state) // current state first, then every change
})
defer unsubscribe()

ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
defer cancel()
if err := client.WaitUntilRegistered(ctx); err != nil {
	log.Fatal(err) // client.FatalError() on a terminal rejection, or ctx.Err()
}
```

Listeners run one at a time, in order, on a goroutine of the client's own, so they may
call the client.

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
reader := client.OpenReader(refs["reader"]) // client address, WithHeaders and III_TOKEN
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

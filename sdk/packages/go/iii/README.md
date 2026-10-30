# iii Go SDK

Go SDK for the [iii engine](https://github.com/iii-hq/iii). A process becomes an iii
**worker** by opening a single WebSocket to the engine and registering **functions**; the
engine invokes those functions and the worker replies over the same socket. Every
function is observable: when a function runs, its **observers** run.

[![Go Reference](https://pkg.go.dev/badge/github.com/iii-hq/iii/sdk/packages/go/iii.svg)](https://pkg.go.dev/github.com/iii-hq/iii/sdk/packages/go/iii)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)

## Install

```bash
go get github.com/iii-hq/iii/sdk/packages/go/iii
```

Requires Go 1.25 or later.

## Hello World

```go
package main

import (
	"context"
	"encoding/json"
	"log"
	"time"

	iii "github.com/iii-hq/iii/sdk/packages/go/iii"
)

func main() {
	// registerWorker-style entry point, matching the Node/Rust SDKs: creates the client
	// and starts connecting in the background. An empty address means III_URL when it
	// is set, else iii.DefaultEngineURL (ws://127.0.0.1:49134).
	client := iii.RegisterWorker("")

	// Served over HTTP, so the handler reads the request (its body is under "body") and
	// returns the response { status_code, body }.
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

	// POST /greet runs hello::greet: it observes the http worker's http::on-request,
	// with the route as args.
	if _, err := client.Observe(iii.Source{
		FunctionID: "http::on-request",
		Args:       iii.Args{"method": "POST", "path": "/greet"},
	}, greet.ID); err != nil {
		log.Fatal(err)
	}

	// RegisterWorker already started connecting; Connect blocks until the first
	// connection is established (or the context is cancelled).
	if err := client.Connect(context.Background()); err != nil {
		log.Fatal(err)
	}
	defer func() {
		// Graceful stop: buffered reports and telemetry get up to five seconds.
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		if err := client.Shutdown(ctx); err != nil {
			log.Printf("shutdown cut short: %v", err)
		}
	}()

	result, err := client.Call(context.Background(), iii.CallRequest{
		FunctionID: greet.ID,
		Payload:    json.RawMessage(`{"body":{"name":"world"}}`),
	})
	if err != nil {
		log.Printf("call: %v", err) // not log.Fatal: it would skip the deferred Shutdown
		return
	}
	log.Printf("result: %s", result) // {"status_code":200,"body":{"message":"Hello, world!"}}
}
```

> **HTTP requests.** The target of an `http::on-request` observer receives the request
> as `{ path, method, body, headers, … }` and returns `{ status_code, body }`. Functions
> called only over the socket can use any payload shape. See the
> [HTTP documentation](https://iii.dev/docs).

A complete, runnable version is in the [`iii-example`](../iii-example) module.

## API

| Operation | Signature | Description |
| --- | --- | --- |
| Register worker | `iii.RegisterWorker(url, opts...) *Client` | Create a client and start connecting in the background (the `registerWorker` entry point). |
| Create | `iii.New(url, opts...) *Client` | Build a client without connecting (call `Connect` yourself). |
| Engine address | `iii.EngineURLFromEnv() string` | `III_URL`, trimmed, else `DefaultEngineURL` (`ws://127.0.0.1:49134`). `New("")` and `RegisterWorker("")` use it. |
| Connect | `client.Connect(ctx) error` | Start the lifecycle if needed and block until connected. |
| Wait for registration | `client.WaitUntilRegistered(ctx) error` | Block until the engine acknowledged the worker on the current connection. |
| Connection state | `client.ConnectionState() ConnectionState`, `client.OnConnectionStateChange(listener) (unsubscribe func())` | The current state, and every change (`connecting`, `connected`, `reconnecting`, `disconnected`, `failed`) in order. |
| Worker id | `client.WorkerID() string` | The id the engine assigned (empty before the first acknowledgement). |
| Register function (typed) | `iii.RegisterFunctionTyped[Req, Resp](client, id, handler, opts...) (*FunctionRef, error)` | Register a function with request/response schemas inferred from the types. |
| Register function | `client.RegisterFunction(id, handler, opts...) (*FunctionRef, error)` | Register a function the engine can invoke by name. `iii.Identity` returns its input: the handler of an event source. |
| Register HTTP function | `client.RegisterHTTPFunction(id, iii.HTTPInvocationRef{...}, opts...) (*FunctionRef, error)` | Register a function hosted elsewhere that the engine invokes over HTTP. |
| Unregister | `ref.Unregister() error` | Remove the function; later reconnects do not register it. |
| Read invocation metadata | `iii.MetadataFromContext(ctx) (json.RawMessage, bool)` | Read the optional per-invocation metadata sidecar inside a handler. |
| Call (await) | `client.Call(ctx, iii.CallRequest{FunctionID, Payload, ...})` | Call a function and wait for the result. `client.Trigger` and `iii.TriggerRequest` are the same under their 0.25 names. |
| Call (fire-and-forget) | `client.Call(ctx, iii.CallRequest{Action: iii.VoidAction()})` | Call without waiting. |
| Call (enqueue) | `client.Call(ctx, iii.CallRequest{Action: iii.EnqueueAction("queue")})` | Route the call through a named queue. |
| Observe | `client.Observe(iii.Source{FunctionID, Args, Filters, ...}, target, opts...) (*ObserverHandle, error)`, `client.ObserveFunction(id, target, opts...)` | Run `target` on every matching call of a function. |
| Current invocation | `iii.InvocationFromContext(ctx) (iii.Invocation, bool)` | The invocation a handler runs in (id, caller, activation, observe depth). |
| Observer hooks | `client.OnObserverAdded(hook)`, `client.OnObserverAddedFor(id, hook)`, `client.OnObserverRemoved(hook)`, `client.OnObserverRemovedFor(id, hook)`, `client.OnObserverMatchFor(id, match)`, `client.OnObserverDeliverFor(id, hook)`; `ref.OnObserverAdded`, `ref.OnObserverRemoved`, `ref.OnObserverMatch`, `ref.OnObserverDeliver` | Set up and tear down per observer, for every function or one; say which observers want each call; deliver calls yourself. |
| Match observers | `iii.MatchFunc(func(obs, call) bool)`, `iii.MatchKeys(observerKeys, callKeys)` | A predicate, or keys the SDK indexes. |
| Call through an observer | `client.Call(ctx, iii.CallRequest{Observer: obs, Payload: payload})` | Run an observer's target from a deliver hook. |
| Observed? | `ref.IsObserved()`, `ref.IsObservedFor(call)`, `ref.Observers()` (or `client.IsObserved(id)`, `client.IsObservedFor(id, call)`, `client.Observers(id)`) | The active observers of a function this worker hosts. |
| Create channel | `client.CreateChannel(ctx, bufferSize)` | Open a streaming data channel (writer + reader ends). |
| Open channel end | `client.OpenReader(ref)`, `client.OpenWriter(ref)` | Open the end of a channel received in a payload, with the client's address and headers. |
| Shut down | `client.Shutdown(ctx) error` | Stop gracefully: flush reports and telemetry within ctx, cancel pending calls, close the socket with a handshake. |
| Close | `client.Close() error` | Stop at once (io.Closer): short fixed flush bounds, cancel pending calls, drop the socket. |

`Register*`, `Observe` and the hook methods may be called before or after `Connect`;
registrations and observers are kept in memory and (re)sent to the engine on every
(re)connection, until they are unregistered or disposed.

### Options

Pass options to `New` or `RegisterWorker`:

| Option | Effect |
| --- | --- |
| `iii.WithName(name)` | Worker name reported to the engine (default `hostname:pid`; a non-empty `III_WORKER_NAME` wins). |
| `iii.WithNamespace(namespace)` | Namespace the worker registers into (else `III_NAMESPACE`, else the engine default). |
| `iii.WithDescription(text)` | One-line summary shown by `engine::workers::list` / `engine::workers::info`. |
| `iii.WithHeaders(h)` | Headers for the WebSocket handshake of the main connection and of channels. |
| `iii.WithReconnectConfig(cfg)` | Reconnect schedule (see [Connection behavior](#connection-behavior)). |
| `iii.WithInvocationTimeout(d)` | How long `Call` waits when `CallRequest.Timeout` is zero (default `iii.DefaultInvocationTimeout`, 30s). |
| `iii.WithTelemetry(iii.TelemetryOptions{...})` | Language, project name and framework reported in the worker metadata. |
| `iii.WithOtel(iii.OtelConfig{...})` | OpenTelemetry export to the engine: on by default, configured or turned off here (see [Observability](#observability)). |
| `iii.WithMetricsReporting(enabled)` | Worker resource gauges (memory, CPU, uptime) exported with the telemetry (default `true`). |

### Registering functions

A handler receives the raw JSON payload and returns any value (marshaled into the
invocation result) or an error. The registration returns a `*iii.FunctionRef`, the
function's handle. A function id is registered once per client: as in the Node, Python
and Rust SDKs, registering it again returns an error (`function id already registered`)
and an empty id is refused (`id is required`); unregister the function first to replace
it.

```go
ref, err := client.RegisterFunction("orders::create", func(ctx context.Context, data json.RawMessage) (any, error) {
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
changes); read it with `MetadataFromContext`. `ok` is `false` when the caller attached
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

Four more options describe the function to its observers. `ArgsFormat` is the JSON
Schema of the observers' args (the engine validates them, `INVALID_ARGS`); `Private`
lets only this worker call the function, while other workers may observe it;
`Observable: &off` refuses every observer (`NOT_OBSERVABLE`); and `Standby` makes the
worker wait for the function when another connection already hosts its id, registering
it again about once a second until the engine accepts it. These three travel as
`args_format`, `private` and `observable`; they need an engine with `observe.v2`, as a
match or deliver hook does, and against an older one the function stays unregistered
and the SDK logs `UNSUPPORTED` once.

```go
// an event source: its handler returns the event, only this worker calls it
ticks, err := client.RegisterFunction("cron::on-tick", iii.Identity, iii.FunctionOptions{
	ArgsFormat: json.RawMessage(`{"type":"object","required":["expression"],"properties":{"expression":{"type":"string"}}}`),
	Private:    true,
	Standby:    true,
})
```

A call this worker makes to its own private function passes the input and the result by
reference: the handler and the hooks treat both as read-only.

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
charge, err := client.RegisterHTTPFunction("billing::charge", iii.HTTPInvocationRef{
	URL:       "https://billing.example.com/charge",
	TimeoutMs: &timeout,
	Headers:   map[string]string{"x-team": "billing"},
	Auth:      iii.BearerAuth("BILLING_TOKEN"), // or iii.HMACAuth("KEY"), iii.APIKeyAuth("x-api-key", "KEY")
}, iii.RegisterFunctionOptions{Description: "Charges a card"})
```

### Unregistering

`ref.Unregister()` removes the function the handle refers to (from `RegisterFunction`,
`RegisterHTTPFunction` or `RegisterFunctionTyped`): the engine is told when the client
is connected, later reconnects do not replay the registration, and the function's
observers are released through its removed hooks. The hooks set for its id stay, for a
later registration of the id.

```go
ref, err := client.RegisterFunction("jobs::nightly", handler)
// ...
if err := ref.Unregister(); err != nil {
	log.Fatal(err) // only for a handle the client did not return
}
```

`Unregister` is idempotent: a second call, or a call on a handle whose id was registered
again since, does nothing and returns nil, as the Node SDK's `FunctionRef` does.

### Calling functions

```go
// Await the result.
res, err := client.Call(ctx, iii.CallRequest{
	FunctionID: "orders::create",
	Payload:    json.RawMessage(`{"item":"widget"}`),
})

// Fire-and-forget (e.g. logging).
client.Call(ctx, iii.CallRequest{
	FunctionID: iii.LogInfo,
	Payload:    json.RawMessage(`{"message":"page_view"}`),
	Action:     iii.VoidAction(),
})

// Enqueue through a named queue and await the receipt.
client.Call(ctx, iii.CallRequest{
	FunctionID: "jobs::process",
	Action:     iii.EnqueueAction("jobs"),
})
```

An await or enqueue call waits `CallRequest.Timeout`, or, when that is zero, the
client's invocation timeout (`iii.WithInvocationTimeout`, default 30s).
`client.Trigger`, `iii.TriggerRequest` and `iii.TriggerAction` are `Call`, `CallRequest`
and `CallAction` under their 0.25 names.

Errors are typed: `errors.Is(err, iii.ErrTimeout)` for a missed deadline,
`errors.Is(err, iii.ErrNotConnected)` for a call cancelled by shutdown, and
`errors.As(err, &ie)` for an `*iii.InvocationError` (carrying the remote `Code`,
`Message`, and `Stacktrace`).

A call to a function this worker registered itself runs in-process once the engine
confirmed it (no frames on the wire unless someone observes it). The handler gets its
own copy of the payload (a private function's is passed by reference), and errors and
timeouts look as on the remote path. Pass `Dispatch: iii.DispatchRemote` to go through
the engine anyway, or register the function with `LocalDispatch: &off` (`off := false`).

Inside a handler, pass the handler's `ctx` to `Call`: the call then carries the handler
as its caller (observers can filter on `caller.*`) and the observe depth the engine's
loop guard reads.

### Observing functions

Every function is observable. When the source function runs, the target runs; the
target is a registered function and receives the `iii.Observed` envelope (`input`,
`output` or `error`, `caller`, ...).

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

// every successful call, the observer defaults
client.ObserveFunction("orders::create", "audit::record")
```

`Source.Args` go to the worker that hosts the function, which reads them in its hooks
(a cron expression, a route, a table); `Source.Filters` select calls by `input`,
`output`, `error`, `caller` and `metadata`. Engine-hosted sources are observed the same
way, e.g. worker changes:

```go
client.Observe(iii.Source{FunctionID: iii.FnOnWorkersChanged, Deliver: iii.DeliverInput}, "ops::on-workers")
```

The filter evaluator is the `filter` subpackage
(`github.com/iii-hq/iii/sdk/packages/go/iii/filter`), the same language the engine runs.

### Hosting observed functions

The worker that hosts a function decides what observing it means with four hooks. Each
overrides something iii does by default:

| Hook | What iii does by default | Use it to |
| --- | --- | --- |
| `iii.AddedHook` | accepts the observer: it becomes active and receives the function's calls | set something up from the observer's args before it is accepted (a cron job, a database trigger, a route); return an error to reject it |
| `iii.RemovedHook` | stops delivering to the observer and forgets it | tear down what the added hook set up |
| `*iii.MatchHook` | every observer wants every call | say which observers want each call, from their args: a cron observer of `0 9 * * *` wants that expression's ticks |
| `iii.DeliverHook` | delivers each call to the matched observers whose `on` and filters match, and gives the caller the handler's result | deliver each call yourself to the matched observers, and decide what the caller gets back: route an HTTP request to one route and return its response |

`OnObserverAdded` and `OnObserverRemoved` are set for every function the worker hosts on
the client, and for one function by id (`OnObserverAddedFor`, `OnObserverRemovedFor`) or
on its `FunctionRef`. Both levels run: when an observer is added, the worker's hook and
then the function's; when it is removed, the function's and then the worker's. The match
and deliver hooks are set per function (`OnObserverMatchFor`, `OnObserverDeliverFor`,
`ref.OnObserverMatch`, `ref.OnObserverDeliver`). Each method returns its receiver, and
`nil` removes the hook.

```go
// every function this worker hosts: keep the event stream open while anything is observed
client.OnObserverAdded(func(ctx context.Context, obs *iii.Observer) error { return events.Retain(ctx) })
client.OnObserverRemoved(func(ctx context.Context, obs *iii.Observer) { events.Release(ctx) })

// one function, by id, before it is registered
client.OnObserverAddedFor("cron::on-tick", func(ctx context.Context, obs *iii.Observer) error {
	expression, _ := obs.Args["expression"].(string)
	return scheduler.Retain(expression) // an iii.ObserveRejection("INVALID_CRON_EXPRESSION", ...) rejects it
})
client.OnObserverRemovedFor("cron::on-tick", func(ctx context.Context, obs *iii.Observer) {
	expression, _ := obs.Args["expression"].(string)
	scheduler.Release(expression)
})
// an observer of expression X gets the ticks of expression X
client.OnObserverMatchFor("cron::on-tick", iii.MatchFunc(func(obs *iii.Observer, call *iii.Call) bool {
	var tick struct{ Expression string `json:"expression"` }
	_ = json.Unmarshal(call.Input, &tick)
	return obs.Args["expression"] == tick.Expression
}))

client.RegisterFunction("cron::on-tick", iii.Identity, iii.FunctionOptions{ArgsFormat: cronArgs, Private: true})

// the scheduler calls the function with each tick
client.Call(ctx, iii.CallRequest{FunctionID: "cron::on-tick", Payload: tick, Action: iii.VoidAction()})
```

How the added and removed hooks run:

- **Accepted.** An observer is accepted once every added hook that applies to it has
  returned nil; the SDK then acks it and it becomes active. `obs.Accepted(ctx)` waits for
  that, so a host can send a new observer its initial state: the added hook starts a
  goroutine that waits on `obs.Accepted` and then calls through the observer (called
  with the hook's own `ctx`, `Accepted` returns an error at once).
- **Rejected.** An added hook that returns an error rejects the observer with the code
  of an `iii.ObserveRejection(code, message)`, or `OBSERVE_REJECTED` for any other error;
  `obs.Reject(code, message)` called while the added hooks run rejects it with that code
  once they return. The removed hook of each level whose added hook already returned
  runs.
- **Removed.** The removed hooks run exactly once for every accepted observer: on
  unobserve, on disposal, and on a late rejection with `obs.Reject(code, message)`, which
  runs them at once, whether the worker is connected or not.
- **One at a time.** The hooks of one function run one at a time, in the order the
  engine announced the observers, also across an `Unregister` and a new registration of
  its id; hooks of different functions run concurrently. A hook past 30s fails the
  observer with `OWNER_TIMEOUT` and its ctx is cancelled.
- **Set hooks by id before registering.** The SDK acks no observer of a function before
  `RegisterFunction` returns, but a hook set on the returned handle can come after
  observers were accepted. A hook set while the function has active observers runs for
  each of them at once (an error late-rejects that observer), and the SDK logs one
  warning naming the function and the number of observers. A hook id this worker has not
  registered is reported once, a short while after its connection is registered or the
  hook is set.

#### Matching observers

The match hook answers one question: does this observer want this call? iii does
everything else as usual: the observer's `on`, filters, policy and `deliver` still apply.
`iii.MatchFunc` is a predicate, asked once per active observer per call; a predicate
that panics counts as no match, and the SDK logs it once per function.

`iii.MatchKeys` is for a function with many observers: an observer wants a call when
they share a key, and the SDK keeps an index instead of asking every observer. It
computes an observer's keys once, when it is accepted, files the observer under each and
unfiles it when it is removed; an observer whose key function panics is rejected with
`OBSERVE_REJECTED`. A call matches the observers filed under any of its keys, each once.
Keys express wildcards by listing them:

```go
rowKey := func(db, table, op string) string { return db + "|" + table + "|" + op }
client.OnObserverMatchFor("database::on-row-changed", iii.MatchKeys(
	func(obs *iii.Observer) []string { // what this observer listens for
		var args struct {
			DB, Table string
			Ops       []string
		}
		_ = obs.DecodeArgs(&args)
		if args.Table == "" {
			args.Table = "*"
		}
		if len(args.Ops) == 0 {
			args.Ops = []string{"*"}
		}
		var keys []string
		for _, op := range args.Ops {
			keys = append(keys, rowKey(args.DB, args.Table, op))
		}
		return keys
	},
	func(call *iii.Call) []string { // what this call is
		var change struct{ DB, Table, Op string }
		_ = json.Unmarshal(call.Input, &change)
		return []string{
			rowKey(change.DB, change.Table, change.Op), rowKey(change.DB, change.Table, "*"),
			rowKey(change.DB, "*", change.Op), rowKey(change.DB, "*", "*"),
		}
	},
))

// a notification from the database becomes a call, when someone wants it
if client.IsObservedFor("database::on-row-changed", &iii.Call{Input: change}) {
	client.Call(ctx, iii.CallRequest{FunctionID: "database::on-row-changed", Payload: change, Action: iii.VoidAction()})
}
```

`ref.IsObserved()` is one atomic load; `client.IsObserved(id)` adds a lock-free lookup of
the id. `IsObservedFor(id, call)` on a function matched by keys is one lookup per call
key; on any other function it is `IsObserved(id)`. A match hook set while the function
has active observers rebuilds the index from them.

#### Delivering yourself

The deliver hook receives the call (`iii.Call`: `Input`, `Output` and `Error` as JSON,
`Caller`, `Metadata`) and the observers that matched it, as they were when the call
started. What it returns is what the caller gets back, and iii delivers nothing for the
function. It runs inside the function's invocation, so calls it makes with its `ctx`
have that invocation as their caller. To call a target, check `obs.Matches(call)` (the
observer's `on` and filters) and call through the observer; the target's result can be
the caller's:

```go
client.OnObserverDeliverFor("http::on-request", func(ctx context.Context, call *iii.Call, observers []*iii.Observer) (any, error) {
	route := routes.Match(call.Input)
	if route == nil {
		return map[string]int{"status_code": 404}, nil
	}
	if !route.Obs.Matches(call) {
		return map[string]int{"status_code": 422}, nil
	}
	return client.Call(ctx, iii.CallRequest{Observer: route.Obs, Payload: call.Input})
})
```

A call through an observer goes to its stored target: the engine checks that this worker
hosts the observer's function and that the observer is active (`NOT_FUNCTION_OWNER`,
`OBSERVER_NOT_FOUND`, `OBSERVER_NOT_ACTIVE`) and applies its policy (`OVERFLOW` past
`max_in_flight`), metadata sidecar and loop guard.

Every call of a function, local or routed, is delivered by the worker that hosts it.
iii's delivery reports a local call's matches in one batched report item and lists a
routed call's matches on its answer, and an unobserved call sends nothing. A function
with a match or deliver hook needs an engine with `observe.v2`, as the options above do.
Against an engine without it, the engine delivers the routed calls of every other
function itself.

## Connection behavior

- **Engine address**: an explicit address wins, then `III_URL`, then
  `ws://127.0.0.1:49134`, as in the other SDKs (`iii.EngineURLFromEnv`).
- **Reconnect** with exponential backoff and jitter (start 1s, ×2, cap 30s, ±30%, retry
  forever). Override with `iii.WithReconnectConfig`.
- **Offline buffer**: invocations sent while disconnected are buffered and flushed on
  reconnect; registrations are replayed from the in-memory registries.
- **Telemetry** travels on a second WebSocket, to `<engine address>/otel`, with its own
  reconnect loop (see [Observability](#observability)).
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
call the client. `client.ConnectionState()` reads the current state and
`client.WorkerID()` the id the engine assigned.

Stop a worker with `Shutdown` or `Close`:

- `client.Shutdown(ctx)` is graceful, like the Node SDK's `shutdown()` and bounded by ctx
  like `http.Server.Shutdown`: it waits for buffered observe reports to be written,
  flushes and stops the telemetry exporters, cancels pending calls with
  `ErrNotConnected`, and closes the socket with a normal-closure handshake. When ctx ends
  first it cuts the rest short, also when the engine never answers the close handshake,
  and returns `ctx.Err()`; the client is stopped either way.
- `client.Close()` (`io.Closer`) stops at once: the report flush gets 250ms and the
  telemetry flush at most 10s, and the socket is dropped without a handshake. Called
  while a `Shutdown` runs, it cuts that Shutdown short (which then returns
  `context.Canceled`), as `http.Server.Close` does.

```go
ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
defer cancel()
if err := client.Shutdown(ctx); err != nil {
	log.Printf("shutdown cut short: %v", err)
}
```

## Streaming channels

For bulk or streaming data, open a channel instead of passing everything through a single
invocation result. `CreateChannel` returns a writer and a reader, each backed by its own
WebSocket; pass either end's ref (`WriterRef` / `ReaderRef`) to another worker inside a
call payload, and it opens the opposite end.

```go
ch, err := client.CreateChannel(ctx, nil)
// stream bytes...
ch.Writer.Write(ctx, payload)        // binary frames (64 KiB chunks)
ch.Writer.SendMessage(ctx, progress) // discrete text message
ch.Writer.Close()                    // signals end-of-stream

// hand ch.ReaderRef to a processor function via Call; on that side:
refs, _ := iii.ExtractChannelRefs(data)
reader := client.OpenReader(refs["reader"]) // client address, WithHeaders and III_TOKEN
reader.OnMessage(func(m string) { /* text messages */ })
all, _ := reader.ReadAll(ctx)        // binary stream, until the writer closes
```

On a channel socket, **a text frame is a message** (`SendMessage` / `OnMessage`) and **a
binary frame is stream data** (`Write` / `ReadAll`), told apart by WebSocket opcode,
no envelope. The writer ends the stream with `Close`; the reader sees that as EOF. If a
`Write` or `SendMessage` failed, `Close` ends the socket without the clean close instead,
and so does a process that exits before `Close`: the engine aborts the channel and
`NextBinary` / `ReadAll` return an error (with the bytes read so far) instead of EOF. A
reader's own `Close` ends its reads as EOF.

## Observability

OpenTelemetry is on by default. When the connection lifecycle starts (`RegisterWorker`
or `Connect`), the client opens a second WebSocket to the engine at
`<engine address>/otel`, with the same `WithHeaders` headers and `III_TOKEN` bearer as the
main connection, and exports spans, metrics and log records over it. The engine takes
them into its own observability pipeline, so a worker needs no exporter configuration
of its own. `client.Close()` flushes what is buffered and stops the pipeline.

What the SDK records:

- **Invocation spans.** Every handler runs inside an `execute <function id>` span (kind
  INTERNAL), a child of the trace context the invocation carried. The handler's `ctx`
  carries that span: pass it to `Call` and the call joins the trace. The SDK injects
  `traceparent` and `baggage` into outbound invocations and extracts them from inbound
  ones, also when export is turned off.
- **Invocation payloads.** The span records the handler's input and output as the
  `iii.invocation.input` and `iii.invocation.output` events. Values of sensitive keys
  (`password`, `secret`, `authorization`, `api_key`, `access_token`, keys ending in
  `_token`, ...) are replaced by `[REDACTED]`. Set `III_DISABLE_TRACE_PAYLOADS=1` (or
  `true`) to stop recording payloads, and `III_TRACE_PAYLOAD_MAX_BYTES` to cap their size.
- **Worker metrics.** Resource gauges (`iii.worker.memory.*`, `iii.worker.cpu.*`,
  `iii.worker.uptime_seconds`, ...) tagged with `worker.id` and `worker.name`, from the
  moment the engine acknowledges the worker. Turn them off with
  `iii.WithMetricsReporting(false)`.

Configure the pipeline with `iii.WithOtel`, or turn it off:

```go
// Defaults: export on, service name "iii-go", metrics every 60s.
client := iii.RegisterWorker("", iii.WithOtel(iii.OtelConfig{
	ServiceName:    "orders",
	ServiceVersion: "1.4.0",
}))

// Off: set OTEL_ENABLED=false (or 0, no, off) in the environment, or in code:
off := false
quiet := iii.RegisterWorker("", iii.WithOtel(iii.OtelConfig{Enabled: &off}))
```

An `OtelConfig` field left at its zero value reads its environment variable
(`OTEL_SERVICE_NAME`, `OTEL_METRICS_ENABLED`, `OTEL_SPANS_FLUSH_INTERVAL_MS`, ...), then
its default; the `OtelConfig` documentation lists each one.

**Global providers.** The SDK installs the global tracer provider, meter provider and
propagator only when the application has not set its own, and restores them on `Close`.
When the SDK's providers are the globals, spans and metrics a handler creates through
`go.opentelemetry.io/otel` are exported to the engine and parented to the invocation
span. An application that configured OpenTelemetry itself keeps its providers and its
propagator; the SDK's invocation spans still reach the engine.

**Helpers.** The `observability` package
(`github.com/iii-hq/iii/sdk/packages/go/iii/observability`) is the Go counterpart of the
Node SDK's `@iii-dev/helpers/observability`: a structured `Logger` whose records carry
the current trace, `WithSpan` for child spans, a traced HTTP client (`NewTransport`,
`NewHTTPClient`), trace context and baggage helpers (`InjectTraceparent`,
`ExtractContext`, `SetBaggageEntry`, ...), span helpers (`SetCurrentSpanAttribute`,
`RecordSpanEvent`, ...), payload redaction (`Redact`, `RedactAndTruncate`) and `Flush`
for a short-lived process. Go has no implicit active context, so each helper takes the
`ctx` the handler received.

```go
var logger observability.Logger // the zero value is ready to use

func quote(ctx context.Context, data json.RawMessage) (any, error) {
	logger.Info(ctx, "quote requested", map[string]any{"currency": "EUR"})

	return observability.WithSpan(ctx, "fetch-rates", observability.SpanOptions{},
		func(ctx context.Context, span observability.Span) (any, error) {
			// An HTTP client span, child of "fetch-rates".
			req, err := http.NewRequestWithContext(ctx, http.MethodGet, "https://rates.example.com/eur", nil)
			if err != nil {
				return nil, err
			}
			resp, err := observability.NewHTTPClient(nil).Do(req)
			if err != nil {
				return nil, err
			}
			defer resp.Body.Close()
			return map[string]int{"status": resp.StatusCode}, nil
		})
}
```

Without running telemetry (turned off, or no client started) the helpers fall back: the
`Logger` writes to the standard `log` package and spans use the global tracer provider.

## Migrating from 0.25

The module is 0.x, and this release changes its API. It requires **Go 1.25 or later**
(the module's `go` directive is 1.25).

**`Call` invokes a function.** `CallRequest` and `CallAction` are the request and the
action; `Trigger`, `TriggerRequest` and `TriggerAction` are the same function and types
under their 0.25 names, marked deprecated. `CallRequest.Payload` carries the input;
`Data` still works when `Payload` is nil, and is deprecated.

```go
// 0.25
client.Trigger(ctx, iii.TriggerRequest{FunctionID: "orders::create", Data: body})

// 0.26
client.Call(ctx, iii.CallRequest{FunctionID: "orders::create", Payload: body})
```

**Triggers are observers.** `RegisterTrigger`, `RegisterTriggerNamespaced`, `Trigger`
handles and `RegisterTriggerInput` are removed: observe the function that replaces the
trigger type, with the trigger's config as args, and the bound function as the target.
The trigger metadata is the observer's metadata sidecar.

```go
// 0.25
trigger, err := client.RegisterTrigger(iii.RegisterTriggerInput{
	Type:       "cron",
	FunctionID: "reports::daily-summary",
	Config:     json.RawMessage(`{"expression":"0 0 9 * * * *"}`),
	Metadata:   json.RawMessage(`{"team":"reports"}`),
})
// later: trigger.Unregister()

// 0.26
observer, err := client.Observe(iii.Source{
	FunctionID: "cron::on-tick",
	Args:       iii.Args{"expression": "0 0 9 * * * *"},
	Deliver:    iii.DeliverInput,
}, "reports::daily-summary", iii.ObserverOptions{Metadata: json.RawMessage(`{"team":"reports"}`)})
// later: observer.Dispose(ctx)
```

| Trigger type | Observe | Args |
| --- | --- | --- |
| `http` | `http::on-request` | `{ method, path, middleware_function_ids?, public_webhook? }` |
| `cron` | `cron::on-tick` | `{ expression }` |
| `database::row-changed` | `database::on-row-changed` | `{ db, table?, ops? }` |
| `durable:subscriber` | `queue::on-message` | `{ topic, max_retries?, backoff_ms?, queue_config?, group? }` |
| `subscribe` | `publish`, filtered on `input.topic` | none |
| `state` | `state::on-changed`, filtered on `input.scope`, `input.key` | none |
| `log`, `trace` | `iii.FnOnLog`, `iii.FnOnSpan` | none |
| `configuration` | `iii.FnOnConfigurationUpdate` | none |
| `engine::functions-available` | `iii.FnOnFunctionsChanged` | none |
| `engine::workers-available` | `iii.FnOnWorkersChanged` | none |
| `stream`, `stream:join`, `stream:leave` | `iii.FnOnStreamChanged`, `iii.FnOnStreamJoin`, `iii.FnOnStreamLeave` | none |
| none (new in 0.26) | `iii.FnOnObserversChanged` | none |

`TriggerFunctionsAvailable`, `TriggerLog`, `FnListTriggers`, `FnInfoTriggers`,
`FnListRegisteredTriggers` and `FnInfoRegisteredTriggers` are removed: the engine-hosted
sources are the `FnOn*` constants, also named as in the other SDKs by `iii.EngineSources`
(`iii.EngineSources.FunctionsChanged`, …), and `engine::functions::list` and `info`
describe each function's `args_format`, `private`, `observable` and `observer_count`.

**Trigger types are functions with hooks.** `RegisterTriggerType`, `TriggerTypeRef`,
`UnregisterTriggerType`, `TriggerHandler` and `TriggerConfig` are removed. Register a
function whose handler returns the event, with `ArgsFormat` for what the trigger config
was and `Private: true`; `TriggerHandler.RegisterTrigger` becomes an added hook,
`UnregisterTrigger` a removed hook, and firing each binding becomes one call of the
function, which iii delivers to the observers its match hook picks.

**Observer hooks are methods.** `RegisterFunctionOptions.Observe`, `ObserveHooks` and
`Delivery` are removed. `ArgsFormat` is an option of any function, `Allow: &off` is
`Observable: &off`, and the callbacks are hooks set by id on the client or on the
`FunctionRef`:

```go
// 0.25
client.RegisterFunction("chat::on-command", handler, iii.RegisterFunctionOptions{Observe: &iii.ObserveHooks{
	Delivery:          iii.DeliveryOwner,
	ArgsFormat:        commandArgs,
	OnObserverAdded:   add,
	OnObserverRemoved: remove,
}})

// 0.26
client.OnObserverAddedFor("chat::on-command", add)
client.OnObserverRemovedFor("chat::on-command", remove)
client.OnObserverMatchFor("chat::on-command", iii.MatchKeys(observerKeys, callKeys))
client.RegisterFunction("chat::on-command", iii.Identity, iii.FunctionOptions{ArgsFormat: commandArgs, Private: true})
```

| 0.25 | 0.26 |
| --- | --- |
| an index from args to observers, and `obs.Emit(ctx, event)` | call the function with the event; `OnObserverMatchFor(id, iii.MatchKeys(observerKeys, callKeys))` or `iii.MatchFunc(predicate)` |
| `obs.Call(ctx, payload)` | `client.Call(ctx, iii.CallRequest{Observer: obs, Payload: payload})` in the deliver hook, after `obs.Matches(call)` |
| `obs.Matches(iii.ObserveDoc{...})` | `obs.Matches(&iii.Call{...})`, which also applies `on` |
| `obs.Legacy`, `LegacyTriggerType` | consumers observe the function |
| a handler that refuses calls from other workers | `Private: true` |

**RBAC hooks.** In `helpers/workerconnection`, `AuthResult.AllowedTriggerTypes`,
`AuthResult.AllowTriggerTypeRegistration` and the trigger registration hook types are
removed; `OnFunctionRegistrationInput` and `OnFunctionRegistrationResult` carry
`private`, `observable` and `args_format`.

**Registrations return handles.** `RegisterFunction`, `RegisterHTTPFunction` and
`RegisterFunctionTyped` return `(*FunctionRef, error)`; unregister through the handle.
A function id is registered once per client: registering it again returns an error
(`function id already registered`), as the Node, Python and Rust SDKs do, and an empty id
is refused. To replace a function, unregister it first.

```go
ref, err := client.RegisterFunction("orders::create", handler)
if err != nil {
	log.Fatal(err)
}
_ = ref.Unregister() // when the function is retired
```

**`ConnectionState()` replaces `State()`**, which stays as a deprecated alias.

**New:** `client.Shutdown(ctx)` for a graceful stop (see
[Connection behavior](#connection-behavior)) and `client.WorkerID()`.

Other changes in this release: `DefaultEngineURL` is `ws://127.0.0.1:49134` and an empty
address reads `III_URL`; OpenTelemetry export is on by default (see
[Observability](#observability) to configure or turn it off); an invocation that reaches a
function registered with `RegisterHTTPFunction` is answered with `function_not_invokable`.

## Resources

- [iii engine](https://github.com/iii-hq/iii)
- [Node SDK](../../node/iii) · [Rust SDK](../../rust/iii): the references this SDK mirrors
- [Examples](https://github.com/iii-hq/iii-examples) (and a runnable Go worker in [`iii-example`](../iii-example))
- [Documentation](https://iii.dev/docs)

# iii Go SDK

Go SDK for the [iii engine](https://github.com/iii-hq/iii).

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

	iii "github.com/iii-hq/iii/sdk/packages/go/iii"
)

func main() {
	client := iii.RegisterWorker("ws://localhost:49134")

	client.RegisterFunction("hello::greet", func(ctx context.Context, data json.RawMessage) (any, error) {
		var in struct {
			Name string `json:"name"`
		}
		if err := json.Unmarshal(data, &in); err != nil {
			return nil, err
		}
		return map[string]string{"message": "Hello, " + in.Name + "!"}, nil
	})

	if err := client.Connect(context.Background()); err != nil {
		log.Fatal(err)
	}

	result, err := client.Call(context.Background(), iii.CallRequest{
		FunctionID: "hello::greet",
		Payload:    json.RawMessage(`{"name":"world"}`),
	})
	if err != nil {
		log.Fatal(err)
	}
	log.Printf("%s", result)
}
```

A complete, runnable version is in the [`iii-example`](../iii-example) module.

## API

| Operation | Signature | Description |
| --- | --- | --- |
| Initialize | `iii.RegisterWorker(url, opts...)` | Create a client and connect to the engine. Returns a `*Client` |
| Register function | `client.RegisterFunction(id, handler, opts...)` | Register a function that can be called by id and observed |
| Register function (typed) | `iii.RegisterFunctionTyped[Req, Resp](client, id, handler, opts...)` | Register a function with request and response schemas inferred from the types |
| Call (await) | `client.Call(ctx, iii.CallRequest{FunctionID, Payload})` | Call a function and wait for the result |
| Call (fire-and-forget) | `client.Call(ctx, iii.CallRequest{..., Action: iii.VoidAction()})` | Call without waiting |
| Call (enqueue) | `client.Call(ctx, iii.CallRequest{..., Action: iii.EnqueueAction("queue")})` | Route the call through a named queue |
| Observe | `client.Observe(iii.Source{FunctionID, Args, Filters}, target, opts...)` | Call `target` on every matching call of a function |
| Observer hooks | `OnObserverAddedFor`, `OnObserverRemovedFor`, `OnObservationMatchFor`, `OnObservationDeliverFor` | Change what observing a function this worker hosts means |
| Call through an observer | `client.Call(ctx, iii.CallRequest{Observer: obs, Payload: payload})` | Run an observer's target from a deliver hook |
| Current invocation | `iii.InvocationFromContext(ctx)` | The invocation the running handler serves (caller, activation) |
| Shut down | `client.Shutdown(ctx)` | Stop gracefully: flush reports and telemetry, cancel pending calls, close the socket |

### Registering Functions

```go
client.RegisterFunction("orders::create", func(ctx context.Context, data json.RawMessage) (any, error) {
	var in struct {
		Body struct {
			Item string `json:"item"`
		} `json:"body"`
	}
	if err := json.Unmarshal(data, &in); err != nil {
		return nil, err
	}
	return map[string]any{"status_code": 201, "body": map[string]string{"id": "123", "item": in.Body.Item}}, nil
})
```

The handler's return value is what the function's observers observe.

### Calling Functions

```go
result, err := client.Call(ctx, iii.CallRequest{
	FunctionID: "orders::create",
	Payload:    json.RawMessage(`{"body":{"item":"widget"}}`),
})

client.Call(ctx, iii.CallRequest{
	FunctionID: "analytics::track",
	Payload:    json.RawMessage(`{"event":"page_view"}`),
	Action:     iii.VoidAction(),
})
```

A call to a function this worker registered itself runs in-process once the
engine confirmed it (no frames on the wire unless someone observes it). Pass
`Dispatch: iii.DispatchRemote` to go through the engine anyway, or register the
function with `LocalDispatch: &off` (`off := false`).

### Observing Functions

The target is a registered function; it receives the `iii.Observed` envelope
(`input`, `output` or `error`, `caller`, ...).

```go
client.RegisterFunction("alerts::big-order", func(ctx context.Context, data json.RawMessage) (any, error) {
	var ev iii.Observed
	_ = json.Unmarshal(data, &ev)
	return nil, notify(ev.Input, ev.Output)
})

// every successful call
client.ObserveFunction("orders::create", "search::reindex-order")

// only large orders
observer, err := client.Observe(iii.Source{
	FunctionID: "orders::create",
	Filters:    iii.Filters{"input.body.total": iii.Op{"$gte": 500}},
}, "alerts::big-order")
if err == nil {
	err = observer.Ready(ctx)
}
// later
_ = observer.Dispose(ctx)
```

Filters are evaluated where the call is. The engine evaluates them for every
call it routes, holding the call's input, caller and metadata and the
answer's output or error. The SDK evaluates them for the calls a worker makes
to its own functions in-process, which the engine never sees, and matches
each call through an index of the filters' `$eq` and `$in` conditions: with
1,000 observers that each filter on their own `input.tenant`, a call
evaluates one filter.

### Hosting Observable Functions

The worker that hosts a function decides what observing it means with four
hooks, each overriding a default:

| Hook | Default |
| --- | --- |
| `OnObserverAddedFor(id, hook)` | accept the observer |
| `OnObserverRemovedFor(id, hook)` | stop delivering to it and forget it |
| `OnObservationMatchFor(id, match)` | every observer wants every call |
| `OnObservationDeliverFor(id, hook)` | run every matching observer whose filters match the call; return the handler's value |

`OnObserverAdded` and `OnObserverRemoved` can be set for every function the
worker hosts, or for one function by id or on its `FunctionRef`.
`OnObservationMatch` and `OnObservationDeliver` are set per function. Each method
returns its receiver, and `nil` removes a hook.

An event source returns the event it is called with. `ArgsFormat` describes
the `args` its observers pass, and `Private: true` lets only this worker call
it:

```go
client.OnObserverAddedFor("cron::on-tick", func(ctx context.Context, obs *iii.Observer) error {
	expression, _ := obs.Args["expression"].(string)
	if !isValidCron(expression) {
		return iii.ObserveRejection("INVALID_CRON_EXPRESSION", "bad expression")
	}
	return scheduler.Retain(expression)
})
client.OnObserverRemovedFor("cron::on-tick", func(ctx context.Context, obs *iii.Observer) {
	expression, _ := obs.Args["expression"].(string)
	scheduler.Release(expression)
})
// each tick reaches the observers of its expression
client.OnObservationMatchFor("cron::on-tick", iii.MatchFunc(func(obs *iii.Observer, call *iii.Call) bool {
	var tick struct{ Expression string `json:"expression"` }
	_ = json.Unmarshal(call.Input, &tick)
	return obs.Args["expression"] == tick.Expression
}))

client.RegisterFunction("cron::on-tick", iii.Identity, iii.FunctionOptions{
	ArgsFormat: json.RawMessage(`{"type":"object","required":["expression"],"properties":{"expression":{"type":"string"}}}`),
	Private:    true,
})

// a consumer
client.Observe(iii.Source{FunctionID: "cron::on-tick", Args: iii.Args{"expression": "0 0 9 * * * *"}}, "reports::daily")
```

With many observers, match by keys instead: the SDK keeps an index, one
lookup per key whatever the number of observers.

```go
client.OnObservationMatchFor("database::on-row-changed", iii.MatchKeys(
	func(obs *iii.Observer) []string { // what this observer listens for
		return []string{fmt.Sprint(obs.Args["table"], "|", obs.Args["op"])}
	},
	func(call *iii.Call) []string { // what this call is
		var change struct{ Table, Op string }
		_ = json.Unmarshal(call.Input, &change)
		return []string{change.Table + "|" + change.Op}
	},
))
```

A deliver hook delivers each call itself: it receives the call and the
observers that matched it, and what it returns is what the function's caller
gets back:

```go
client.OnObservationDeliverFor("http::on-request", func(ctx context.Context, call *iii.Call, observers []*iii.Observer) (any, error) {
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

To send one observer something of its own, such as its initial state, call
through that observer once `obs.Accepted` returns nil. The call runs that
observer's target alone, with the payload as given:

```go
client.OnObserverAddedFor("tmux::on-changed", func(_ context.Context, obs *iii.Observer) error {
	go func() {
		if obs.Accepted(context.Background()) != nil {
			return // a rejected observer gets nothing
		}
		_, _ = client.Call(context.Background(), iii.CallRequest{
			Observer: obs, Payload: stateFor(obs.Args), Action: iii.VoidAction(),
		})
	}()
	return nil
})
```

A worker's own call to its private function passes the input and the result
by reference: the handler and the hooks treat both as read-only.

A function that sets `ArgsFormat`, `Private: true`, `Observable: &off`, an
`OnObservationMatch` hook or an `OnObservationDeliver` hook needs an engine that
advertises `observe.v2`; against an older engine it stays unregistered, and the
SDK logs `UNSUPPORTED` once for it.

## Resources

- [Documentation](https://iii.dev/docs)
- [iii Engine](https://github.com/iii-hq/iii)
- [Examples](https://github.com/iii-hq/iii-examples)

---
name: iii-sdk-reference
description: >-
  Use when working with iii SDK APIs across Node.js, browser, Python, Rust, or Go: package
  installation, worker initialization, function registration, calling functions, observers and
  observer hooks, local dispatch, streams, logging, OpenTelemetry, and language-specific caveats.
---

# SDK Reference

Use this skill for language-specific SDK details. Use `iii-core-primitives` for the common model and
`iii-error-handling` for exception handling.

## Install

```bash
# TypeScript / Node.js
npm install iii-sdk

# Browser apps
npm install iii-browser-sdk

# Python
pip install iii-sdk

# Rust
cargo add iii-sdk

# Go
go get github.com/iii-hq/iii/sdk/packages/go/iii
```

## Choose the SDK

| SDK | Package | Best for | Important caveat |
| --- | --- | --- | --- |
| Node.js | `iii-sdk` | Server-side TypeScript/JavaScript workers | Supports custom headers, Logger, OpenTelemetry, HTTP-invoked functions |
| Browser | `iii-browser-sdk` | Web apps and interactive UI callbacks | Connect through the `rbac-proxy` worker's public port, never the engine port; keep secrets server-side |
| Python | `iii-sdk` | Sync or async Python workers | Use `call_async` inside async handlers |
| Rust | `iii-sdk` | High-performance tokio workers | Handler error type should map into `iii_sdk::Error` |
| Go | `github.com/iii-hq/iii/sdk/packages/go/iii` | Go services | Handlers take `(ctx, json.RawMessage)`; pass the handler's `ctx` to `Call` |

`Logger`/OpenTelemetry, HTTP request/response types, stream, queue, and worker-connection types live in
the helpers package, installed alongside the SDK: `@iii-dev/helpers` (Node, with submodules like
`/observability` and `/http`) or `iii-helpers` (Python `iii_helpers.*`, Rust `iii_helpers::*`).

## Common API Map

| Capability | Node / browser | Python | Rust | Go |
| --- | --- | --- | --- | --- |
| Connect worker | `registerWorker(url, options?)` | `register_worker(address, options?)` | `register_worker(url, InitOptions)` | `iii.RegisterWorker(url, opts...)` |
| Register local function | `registerFunction(id, handler, options?)` | `register_function(id, handler, **options)` | `register_function("id", RegisterFunction::new(...))` | `client.RegisterFunction(id, handler, opts...)` |
| Call a function | `call({ function_id, payload })` | `call(request)` / `call_async(request)` | `call(CallRequest::new(id, payload))` | `client.Call(ctx, iii.CallRequest{...})` |
| Durable enqueue | `CallAction.Enqueue({ queue })` | `{"type": "enqueue", "queue": name}` | `CallAction::Enqueue { queue }` | `iii.EnqueueAction(queue)` |
| Fire and forget | `CallAction.Void()` | `{"type": "void"}` | `CallAction::Void` | `iii.VoidAction()` |
| Observe a function | `observe(source, target, options?)`; `observe('fn::id', target)` | `observe(source, target, options)` | `observe(Source::function(id)..., target)` / `observe_with(.., ObserverOptions)` | `client.Observe(iii.Source{...}, target, opts...)` / `client.ObserveFunction(id, target)` |
| Function options for observers | `{ args_format, private, observable }` | `args_format=`, `private=`, `observable=` | `.args_format::<T>().private().observable(false)` | `iii.FunctionOptions{ArgsFormat, Private, ...}` |
| Observer hooks, one function | `iii.onObserverAdded(id, h)`, `onObserverRemoved(id, h)`, `onObservationMatch(id, m)`, `onObservationDeliver(id, h)`, or the same on the `FunctionRef` | `@iii.on_observer_added(id)`, `@ref.on_observer_removed`, `ref.on_observation_match(m)`, `ref.on_observation_deliver(h)` | `on_observer_added_for(id, h)`, `on_observer_removed_for(id, h)`, `on_observation_match_for(id, m)`, `on_observation_deliver_for(id, h)`, or `function.on_observer_added(h)` | `client.OnObserverAddedFor(id, h)`, `OnObserverRemovedFor`, `OnObservationMatchFor`, `OnObservationDeliverFor`, or `ref.OnObserverAdded(h)` |
| Observer hooks, every function | `iii.onObserverAdded(h)`, `iii.onObserverRemoved(h)` | `@iii.on_observer_added`, `@iii.on_observer_removed` | `on_observer_added(h)`, `on_observer_removed(h)` | `client.OnObserverAdded(h)`, `client.OnObserverRemoved(h)` |
| Match observers to calls | `onObservationMatch(id, (obs, call) => boolean)` or `onObservationMatch(id, { observer, call })` | `on_observation_match(id, predicate)` or `on_observation_match(id, observer=, call=)` | `Match::predicate(f)` or `Match::keys(observer_keys, call_keys)` | `iii.MatchFunc(f)` or `iii.MatchKeys(observerKeys, callKeys)` |
| Call through an observer (no metadata) | `call({ observer: obs, payload })` | `call_async({"observer": obs, "payload": p})` | `call(CallRequest::observer(&obs, payload))` | `client.Call(ctx, iii.CallRequest{Observer: obs, Payload: p})` |
| Current invocation | `currentInvocation()` | `current_invocation()` | `iii_sdk::current_invocation()` | `iii.InvocationFromContext(ctx)` |
| Stream data | a source or a sink in `payload` or in the result; the other side gets `InputStream` / `OutputStream` (`iii-sdk`, `iii-browser-sdk`; the browser takes Web streams, `Blob`, and `File`) | the same; `InputStream.from_file(path)`, `OutputStream.to_file(path)` (`iii`) | `InputStream::from_reader(r)`, `OutputStream::to_writer(w)` fields (`iii_sdk`) | `iii.InputStreamFromReader(r)`, `iii.OutputStreamToWriter(w)`; `*iii.InputStream` / `*iii.OutputStream` fields |
| Explicit pipe | `await iii.pipe({ bufferSize })` returns `{ input, output }` (Node and browser) | `iii.pipe()` / `await iii.pipe_async()` | `iii.pipe().await?` returns `Pipe { input, output }` | `input, output, err := client.Pipe(ctx)` |

A call's `metadata` is any JSON value, delivered to the handler beside the payload. `null` (Python
`None`, Rust `Value::Null`, Go `json.RawMessage("null")`) is the same as no metadata on every path,
local or routed. A call through an observer takes no metadata: its target receives the observer's,
and every SDK refuses one that names metadata with "a call through an observer takes no metadata;
its target receives the observer's metadata".

## Node.js

```typescript
import { registerWorker } from "iii-sdk";
import { Logger } from "@iii-dev/helpers/observability";

const iii = registerWorker("ws://localhost:49134", {
  workerName: "node-worker",
  invocationTimeoutMs: 30000,
});

iii.registerFunction("users::lookup", async (input) => {
  new Logger().info("looking up user", { userId: input.userId });
  return { userId: input.userId, name: "Ada" };
});
```

Node supports custom WebSocket headers, `Logger`, OpenTelemetry options, HTTP-invoked function
registration, call and observer metadata, streams, observers, and observer hooks. `CallRequest`,
`CallAction`, `ObserveRejection`, and `EngineSources` are exported from the package root.

## Browser

```typescript
import { registerWorker, CallAction } from "iii-browser-sdk";

const iii = registerWorker("wss://api.example.com/worker?token=session-token");

const result = await iii.call({
  function_id: "backend::get-user",
  payload: { userId: "123" },
});

await iii.call({
  function_id: "analytics::track",
  payload: { event: "page_view" },
  action: CallAction.Void(),
});
```

Do not expose the private engine worker port to untrusted browsers; put the `rbac-proxy` worker in
front of it (`iii call compose::add worker=rbac-proxy`). Browser workers cannot send custom
WebSocket headers and must not hold backend secrets.

The browser `call` sends `metadata`, `traceparent` and `baggage`, and handlers receive metadata as
their second argument. Browsers have no `AsyncLocalStorage`:
`iii.currentInvocation()` is only known until the handler's first `await`, so read it first and
wrap later calls in `iii.withInvocation(inv, () => ...)`.

## Python

```python
from iii import InitOptions, register_worker
from iii_helpers.observability import Logger

iii = register_worker(
    address="ws://localhost:49134",
    options=InitOptions(worker_name="python-worker"),
)

def lookup_user(data):
    Logger().info("looking up user", {"userId": data["userId"]})
    return {"userId": data["userId"], "name": "Ada"}

iii.register_function("users::lookup", lookup_user)
```

Python handlers may be sync or async. Use `await iii.call_async(request)` inside async handlers,
and `iii.call(request)` in sync contexts. `HttpResponse` (from `iii_helpers.http`) uses `status_code`, like the other helpers packages.

## Rust

```rust
use iii_sdk::{register_worker, InitOptions, RegisterFunction};
use serde_json::json;

let iii = register_worker("ws://127.0.0.1:49134", InitOptions::default());

iii.register_function(
    "users::lookup",
    RegisterFunction::new(|input: serde_json::Value| -> Result<serde_json::Value, iii_sdk::Error> {
        Ok(json!({ "userId": input["userId"], "name": "Ada" }))
    }).description("Look up a user"),
);
```

Rust supports typed handlers and schema extraction when input/output types derive
`schemars::JsonSchema`. Add the `otel` feature when using OpenTelemetry helpers. A local call runs
on the caller's task, and a sync handler (`RegisterFunction::new`) runs inline: make a blocking
handler async or opt out with `.local_dispatch(false)`. Rust has no overloading, so the client's
per-function hook methods take a `_for` suffix (`on_observer_added_for(id, hook)`); a hook is an
async closure or a type that implements `AddedHook`, `RemovedHook`, or `DeliverHook`.

## Go

```go
client := iii.RegisterWorker(iii.DefaultEngineURL, iii.WithNamespace("orders"))

client.RegisterFunction("users::lookup", func(ctx context.Context, data json.RawMessage) (any, error) {
	var in struct {
		UserID string `json:"userId"`
	}
	if err := json.Unmarshal(data, &in); err != nil {
		return nil, err
	}
	return map[string]string{"userId": in.UserID, "name": "Ada"}, nil
})
```

Per-invocation metadata rides on `ctx` (`iii.MetadataFromContext(ctx)`). Inside a handler, pass its
`ctx` to `client.Call` so the call carries the caller and the observe depth. `RegisterFunction`
returns `(*iii.FunctionRef, error)`; `iii.Identity` is a handler that returns its input, for event
sources.

## Observers and local dispatch

- `observe(source, target, options?)` is in every SDK: when the source function runs, the target
  runs. HTTP routes, cron schedules, and queue topics are observers of `http::on-request`,
  `cron::on-tick`, and `queue::on-message`. Details and examples per language: `iii-observe`.
- `registerFunction` returns a `FunctionRef` with the hook methods, `isObserved(call?)` and
  `observers()` (Python `is_observed(call=None)`, Rust `is_observed()` / `is_observed_for(&call)`,
  Go `ref.IsObserved()` / `ref.IsObservedFor(call)`). With a call, on a function whose
  `onObservationMatch` uses keys, `isObserved` looks up the call's keys.
- A call to a function the same worker registered runs in-process once the engine confirmed it.
  Force the engine path per call with `dispatch: 'remote'` (Node, browser, Python
  `"dispatch": "remote"`), Rust `CallRequest::new(..).dispatch(Dispatch::Remote)`, Go
  `Dispatch: iii.DispatchRemote`; or per function with `localDispatch: false`
  (`local_dispatch=False`, `.local_dispatch(false)`, `LocalDispatch: &off`).
- Against an engine without `observe.v2`, a function that declares `args_format`, `private`, or
  `observable: false`, or sets `onObservationMatch` or `onObservationDeliver`, stays unregistered and the SDK logs
  `UNSUPPORTED` once; everything else works.
- Against an engine without `observe.capture` (0.25, or a 0.26 release candidate), observers send
  the legacy `on` their `capture` implies and the SDK logs a one-time deprecation warning: upgrade
  the engine. The default `capture` is `[input, output, caller]`; `metadata` is opt-in.
- `trigger` is a deprecated alias of `call` in every SDK and the CLI (`trigger_async`, `Trigger`,
  `TriggerRequest`, `TriggerAction`, `iii trigger`): the same function or type, marked deprecated
  in types (JSDoc `@deprecated`, Rust `#[deprecated]`, Go `// Deprecated:`, Python docstring) with
  no runtime warning, so code written for 0.25 keeps compiling. Use `call`.

## Streams

From 0.27, every SDK (TypeScript for Node and the browser, Python, Rust, and Go) carries streams as
values of a call. There is no `createChannel` (Go `CreateChannel`), no refs to pass, and no
`iii-sdk/channel` subpath.

- **Send:** put a source in a payload. TypeScript: a Node `Readable`, a Web `ReadableStream`, a
  `Blob`/`File`, an async generator, or `InputStream.from(x)` (a string, a `Uint8Array`, or an
  iterable of them). Python: an async generator, a binary file opened for reading,
  `InputStream.from_file(path)`, `InputStream.from_iter(it)`, or `InputStream.from_bytes(b)`. Rust:
  `InputStream::from_reader(AsyncRead)`, `::from_stream(Stream<Item = Result<Bytes, E>>)`, or
  `::from_bytes(b)`. Go: `iii.InputStreamFromReader(io.Reader)` or `iii.InputStreamFromBytes(b)`.
  Browser: a `ReadableStream`, a `Blob`/`File`, an async generator, or `InputStream.from(x)`. Plain
  objects, strings, and buffers stay JSON.
- **Receive:** the handler gets an `InputStream` where the source was. In Rust, declare
  `InputStream` / `OutputStream` fields in a `Deserialize + JsonSchema` struct; they bind to the
  handler's worker on first use (`stream.bind(&iii)` when a process has several clients). In Go,
  declare `*iii.InputStream` / `*iii.OutputStream` fields in the struct you unmarshal into
  (`stream.Bind(client)` for JSON from outside a handler or a call result, with several clients).
- **Return:** put a source in the handler's result; the caller gets an `InputStream` in the result.
- **Sink:** pass a Node `Writable` / Web `WritableStream`, Python `OutputStream.to_file(path)` /
  `OutputStream.to(writer)`, Rust `OutputStream::to_writer(w)`, or Go
  `iii.OutputStreamToWriter(io.Writer)`. The handler gets an `OutputStream`, and the call resolves
  once the sink has everything.
- **Forward:** pass a received, unread `InputStream` on in another call: only its ref travels.
- **Read:** iterate for byte chunks; `bytes()` / `text()` (Python and Rust `read_all()`, `text()`);
  `lines()`; `messages()` for JSON text frames; `frames()` for both in order; `cancel()`; `tee()`
  before the first read for two copies (TypeScript, browser, Python). One reader: a second read
  fails. Go: an `InputStream` is an `io.Reader`; `ReadAll(ctx)`, `Text(ctx)`, and the iterators
  `Lines(ctx)`, `Messages(ctx)`, `Frames(ctx)`; `Cancel()`; no `tee()`.
- **Write:** `await out.write(chunk)` (bytes), `await out.send(value)` (one JSON message),
  `close()`, `abort(reason)`, `keepOpen()` (Python and Rust `keep_open()`), and the bridges
  `toReadable()` / `toWritable()` / `toWebStream()` (the browser has only `toWebStream()`). Go:
  `out.Write(ctx, b)`, `out.Send(ctx, v)`, `Close()`, `Abort(reason)`, `KeepOpen()`, and
  `Writer(ctx)` for an `io.Writer`.
- **Lifecycle:** returning closes a received `OutputStream` the handler used, and throwing aborts
  it; an `InputStream` the handler started and did not finish is cancelled, and a read of a
  cancelled stream fails with `STREAM_CLOSED` instead of ending. An untouched
  `OutputStream` is closed when the handler returns, except in a call through an observer (an HTTP
  route), where it is closed only when the handler returns nothing, so a route that returns
  `{ status_code, body }` keeps its buffered response.
- **Abort:** the reader gets `StreamAbortedError` (TypeScript, browser, Python),
  `Error::StreamAborted` (Rust), or an `*iii.InvocationError` (Go, `iii.CodeStreamAborted`), code
  `STREAM_ABORTED`, after the data already sent. The browser aborts with close code 4011. The
  `reason` is the engine's text, not the writer's. A call whose sink was aborted fails with
  `STREAM_ABORTED`.
- **Queues:** `CallAction.Enqueue` (Go `iii.EnqueueAction(queue)`) with a stream in the payload is
  refused with `STREAM_NOT_ALLOWED`; `CallAction.Void` is allowed.
- **HTTP:** a route's `request_body` is an `InputStream` and `response` an `OutputStream`:
  `await response.send({ type: "set_status", status_code: 200 })`, then `await response.write(chunk)`.
  In Go, `iiihttp.Wrap(fn)` (`helpers/http`) hands the handler a `StreamRequest`, an `io.Reader`
  over `request_body`, and a `StreamResponse` with `Status`, `Headers`, and `Write`.
- **Observers:** a captured `input` or `output` field that carried a stream arrives as
  `StreamInfo` (`{ stream: true, direction, id }`), not a live end.
- **Low-level refs:** `extractChannelRefs`, `isChannelRef`, `StreamChannelRef`, `ChannelDirection`,
  and `ChannelItem` live in `iii-sdk/protocol`, `iii-browser-sdk/protocol`, `iii.protocol`, and
  `iii_sdk::protocol`. Go keeps `iii.StreamChannelRef` and `iii.ChannelDirection`.

```typescript
import { createReadStream } from "node:fs";
import type { InputStream } from "iii-sdk";

iii.registerFunction("files::process", async ({ body }: { body: InputStream }) => {
  let bytes = 0;
  for await (const chunk of body) bytes += chunk.length;
  return { bytes };
});

await iii.call({ function_id: "files::process", payload: { body: createReadStream("report.csv") } });
```

```python
from iii import InputStream


async def process(payload: dict) -> dict:
    return {"bytes": len(await payload["body"].read_all())}


iii.register_function("files::process", process)
await iii.call_async(
    {"function_id": "files::process", "payload": {"body": InputStream.from_file("report.csv")}}
)
```

```rust
use iii_sdk::{CallRequest, Error, InputStream, RegisterFunction};

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct Upload {
    body: InputStream,
}

iii.register_function(
    "files::process",
    RegisterFunction::new_async(|u: Upload| async move {
        Ok::<_, Error>(serde_json::json!({ "bytes": u.body.read_all().await?.len() }))
    }),
);

let file = tokio::fs::File::open("report.csv").await?;
iii.call(CallRequest::new(
    "files::process",
    serde_json::json!({ "body": InputStream::from_reader(file) }),
))
.await?;
```

```go
type upload struct {
	Body *iii.InputStream `json:"body"`
}

iii.RegisterFunctionTyped(client, "files::process", func(ctx context.Context, u upload) (map[string]int64, error) {
	n, err := io.Copy(io.Discard, u.Body)
	return map[string]int64{"bytes": n}, err
})

file, err := os.Open("report.csv")
if err != nil {
	return err
}
payload, err := json.Marshal(map[string]any{"body": iii.InputStreamFromReader(file)})
if err != nil {
	return err
}
_, err = client.Call(ctx, iii.CallRequest{FunctionID: "files::process", Payload: payload})
```

Refs are the same JSON in every SDK, so streams cross between SDKs and between 0.26 and 0.27
workers in both directions.

## Namespaces

A worker belongs to one namespace: `options.namespace` (`InitOptions.namespace`) → the `III_NAMESPACE`
environment variable → the engine's `default`. Compose sets `III_NAMESPACE` to its daemon's namespace
(`iii compose -n dev ...`) for every worker it starts, so a whole project lands in one namespace
without any code change. Routing is strict: a function is only reachable in the namespace it
registered in.

- `iii.call({ function_id })` resolves in the calling worker's namespace. Calls to your own
  functions and to other workers declared in the same `worker-compose.yaml` need no namespace.
- Engine-owned functions register in `default`: `engine::*`, `configuration::*`, `stream::*`
  (from `iii-stream`) and the `observability::*` sources. `engine::*` and `configuration::*` resolve
  there implicitly (since 0.24.5); for the others pass `namespace: "default"` on the call, or on an
  `observe` source, when your worker runs in a Compose namespace.
- `observe` resolves its source in the worker's namespace, so an observer of `http::on-request`
  binds to the `http` worker of the same project.
- Never prefix a function id with a namespace. The id stays `orders::validate`; the namespace is a
  separate field.
- From the CLI, `iii call -n dev orders::validate ...` selects the namespace; omitting `-n`
  resolves in `default`.

```typescript
// Same-project worker: no namespace
await iii.call({ function_id: "orders::validate", payload: order });

// Engine-owned configuration worker: resolves in `default` without a namespace;
// naming it explicitly is equivalent
const cfg = await iii.call({
  function_id: "configuration::get",
  namespace: "default",
  payload: { id: "orders" },
});
```

```python
cfg = await iii.call_async(
    {"function_id": "configuration::get", "namespace": "default", "payload": {"id": "orders"}}
)
```

```rust
let cfg = iii
    .call(CallRequest::new("configuration::get", json!({ "id": "orders" })).namespace("default"))
    .await?;
```

## When to Use

- Use this skill for package names, SDK exports, initialization options, browser security constraints,
  stream API details, and language-specific syntax.
- Use this when a task asks for Python, Rust, or Go examples and the issue is SDK syntax rather
  than iii architecture.

## Boundaries

- For the common Worker/Function/Observer model, built-in source functions, and invocation mode
  decisions, use `iii-core-primitives`.
- For the observe model, filters, observer hooks, event sources, and upgrading 0.25 code, use
  `iii-observe`.
- For deployment config, engine-owned workers, RBAC (`rbac-proxy`), and ports, use
  `iii-engine-config`.
- For retryability and exception classes, use `iii-error-handling`.

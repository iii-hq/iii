---
type: how-to
trigger_type: http
title: Expose a function as an HTTP endpoint
---

# When to use

Register an `http` trigger when a registered function should be invoked by an inbound HTTP request matching an `(api_path, http_method)` pair. The worker handles routing, parsing, body limits, CORS, and timeouts; the bound function receives a structured `HttpRequest` and returns an `HttpResponse` envelope.

Reach for it when:

- You want to expose a function as a REST endpoint without standing up a separate HTTP server.
- You need URL path parameters (`/users/:id`) or query strings parsed and surfaced to the handler.
- You want condition-gated routes (the trigger's `condition_function_id` is invoked with the request and the handler is skipped on falsy/error) or per-route preHandler middleware (see [Add request preHandler middleware](iii://iii-http/http/middleware)).
- You want a single not-found handler for unmatched routes (configure `not_found_function` on the worker — see [the README](../../../README.md)).

Use [middleware](iii://iii-http/http/middleware) instead when the work is cross-cutting and should run before *every* matching request rather than be the route handler itself.

Prerequisite: the `iii-http` worker must be enabled in `config.yaml`. Handlers and triggers are registered from a connected worker via `iii.registerFunction` and `iii.registerTrigger` — the worker has no `http::*` engine functions.

# Inputs

Registration is a two-step pattern: define the handler function, then bind it to the `http` trigger type.

## Handler function

Register any function id. The handler receives the `HttpRequest` payload documented below in **Outputs** and must return an `HttpResponse` envelope.

```json
// iii.registerFunction — handler id only; no engine payload.
{ "id": "api::get-user" }
```

## Trigger registration

```json
{
  "type":        "http",                              // required. Must be exactly "http".
  "function_id": "api::get-user",                     // required. Handler invoked when the route matches.
  "config": {
    "api_path":                "/users/:id",         // required. URL path. ":name" segments become path_params on the request.
    "http_method":             "GET",                // required. HTTP method (case-insensitive: "GET", "POST", "PUT", "DELETE", "PATCH", etc.).
    "condition_function_id":   "auth::is-public",    // optional. Engine invokes this with the request; handler runs only on truthy.
    "middleware_function_ids": ["auth::require-bearer", "log::request"]  // optional. Per-route preHandler chain, invoked in array order before the handler.
  }
}
```

`type`, `function_id`, `config.api_path`, and `config.http_method` are required. The path syntax recognizes `:name` segments as path parameters; everything else is matched literally. Two routes registered against the same `(api_path, http_method)` pair conflict — the worker's hot router resolves to the most recently registered.

`condition_function_id` is optional. When set, the engine calls it with the same `HttpRequest` the handler would receive and runs the handler only when it returns truthy. A condition that errors is logged and the handler is skipped — the request gets the worker's not-found response (or `404` if `not_found_function` is unset).

`middleware_function_ids` is optional. Each id is invoked in array order **after** any global middleware configured on the worker; see [Add request preHandler middleware](iii://iii-http/http/middleware) for the contract.

# Outputs

When a route matches, the engine invokes `function_id` with the `HttpRequest` payload:

```json
{
  "path":         "/users/123",                       // The full request path as received.
  "method":       "GET",                              // The HTTP method.
  "path_params":  { "id": "123" },                    // Variables extracted from `:name` segments in api_path.
  "query_params": { "fields": "name,email" },         // URL query string parameters.
  "headers":      { "content-type": "application/json", "authorization": "Bearer ..." },  // Request headers (lowercased keys).
  "body":         { "any": "json" },                  // Parsed request body. JSON object/array/scalar; `null` when the request had no body.
  "trigger":      { "type": "http", "path": "/users/:id", "method": "GET" },  // Optional. Trigger metadata so handlers bound to multiple routes can dispatch on the original `api_path`.
  "context":      { "user_id": "u_123" }              // Populated by middleware via { action: "continue" }; merged across all middleware in chain order.
}
```

- `path_params` keys come from `:name` segments in the configured `api_path`. A request to `/users/123` against `/users/:id` produces `{"id":"123"}`. Path params are always strings; coerce numerically inside the handler if needed.
- `query_params` flattens the URL's query string. Repeated keys keep only the **last** value (this worker uses a `HashMap<String, String>`, not a multimap); if you need multi-valued params, pre-encode them as a comma-joined value or split into distinct keys.
- `headers` keys are lowercase. Match case-insensitively in the handler.
- `body` is the parsed JSON body. Empty bodies arrive as `null`. The worker rejects bodies larger than `body_limit` (default 1 MB) before the handler runs.
- `trigger` is populated when this request was routed via an `http` trigger; downstream workers that share handlers across trigger types (`http`, `cron`, `state`, ...) branch on `trigger.type`.
- `context` accumulates fields each middleware adds when it returns `{ action: "continue", context: { ... } }`. Auth middleware typically attaches the verified user; rate-limit middleware attaches counter info.

The handler must return an `HttpResponse` envelope:

```json
{
  "status_code": 200,                                  // HTTP status code. Defaults to 200 when omitted.
  "headers":     { "Content-Type": "application/json", "X-Custom": "value" },  // Response headers. Either an object (preferred) or an array of "Name: value" strings.
  "body":        { "user": { "id": "123", "name": "Alice" } }                  // Response payload. JSON object/array/scalar/string; serialization is chosen by Content-Type (see below).
}
```

- `status_code` defaults to 200 when omitted or non-numeric.
- `headers` accepts both shapes:
  - Object form (`{ "Name": "value" }`) — preferred.
  - Array form (`["Name: value", "Other: value"]`) — entries without a colon or with non-string values are silently dropped.
- `body` serialization follows the `Content-Type`:
  - When `Content-Type` is unset, the worker defaults to `application/json` and serializes `body` as JSON.
  - When `Content-Type` is set and the body is a JSON string, the string's bytes are written verbatim (use this for HTML, XML, plain text).
  - When `Content-Type` is set, not `application/json`, and the body is an object/array, it's stringified before writing.
- The default `Content-Type: application/json` is added only when the handler doesn't set its own.

# Worked example

The typical pattern is a one-step registration:

- Register a function with `iii.registerFunction` to receive the `HttpRequest` payload above.
- Bind it via `iii.registerTrigger({ type: 'http', function_id, config: { api_path, http_method } })`.

For path parameters, use `:name` segments in `api_path`; the matching value arrives in `path_params[name]`. For per-route preHandlers (auth checks, rate limits), set `middleware_function_ids` on the trigger config — the chain runs after any global middleware configured in `iii-config.yaml`.

For runnable scaffolds in TypeScript, Python, and Rust, see the http worker source and the SDK usage examples in [the iii main repo](https://github.com/iii-hq/iii).

# Related

- [Add request preHandler middleware](iii://iii-http/http/middleware) — the matching how-to for cross-cutting concerns; `middleware_function_ids` on a trigger references middleware ids documented there.
- `iii-http` worker config (see [the README](../../../README.md)) — `port`, `host`, `default_timeout`, `concurrency_request_limit`, `cors`, `body_limit`, `trust_proxy`, `request_id_header`, `ignore_trailing_slash`, `not_found_function`.
- `state`/`stream`/`cron` reactive triggers — pair with `http` when an inbound request should kick off a state mutation that fans out via reactive triggers.

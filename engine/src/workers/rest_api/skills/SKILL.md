---
name: iii-http
description: >-
  Expose registered functions as HTTP endpoints via an `http` trigger, with a
  preHandler middleware chain for auth, rate limiting, and logging. Reach for it
  to serve REST without a separate web server.
---

# iii-http

The `iii-http` worker exposes registered functions as HTTP endpoints. It runs an axum server on the configured `port`/`host`, routes inbound requests against `http` triggers (the only trigger type it provides), and invokes the bound function with a structured `HttpRequest`. The handler returns an `HttpResponse` envelope (`status_code`/`headers`/`body`) which the worker serializes to the wire — JSON, plain text, or bytes depending on the `Content-Type` the handler sets.

The worker has no callable functions. Its surface is the `http` trigger plus a middleware system that runs before each handler. Routes are registered through `iii.registerTrigger({ type: 'http', ... })`. Middleware are plain registered functions referenced either globally (`iii-config.yaml` → `middleware:`) or per-route (`middleware_function_ids` on the trigger); the worker invokes them in order before the request reaches its handler.

## When to Use

- Exposing a function as a REST endpoint without standing up a separate HTTP server.
- You need URL path parameters (`/users/:id`) or query strings parsed and surfaced to the handler.
- You want condition-gated routes (`condition_function_id`) or per-route preHandler middleware.
- You need one not-found handler for unmatched routes (`not_found_function`).

## Boundaries

- No callable functions — never invoked through an `http::*` id; routes are bound with `iii.registerTrigger`.
- Two routes on the same `(api_path, http_method)` conflict; the hot router resolves to the most recently registered.
- `query_params` is a single-valued map — repeated keys keep only the last value; pre-encode multi-valued params.
- Middleware runs before body parsing, so it sees headers/path/query but not `body`; use it for preconditions, not response logic (that is the handler).

## Reactive triggers

Bind an `http` trigger when a registered function should be invoked by an inbound request matching an `(api_path, http_method)` pair. The worker handles routing, parsing, body limits, CORS, and timeouts; the handler receives an `HttpRequest` and returns an `HttpResponse`.

### How to bind

1. Register a handler: `iii.registerFunction('api::get-user', handler)`.
2. Register the trigger:

```typescript
iii.registerTrigger({
  type: 'http',
  function_id: 'api::get-user',
  config: {
    api_path: '/users/:id',   // required. `:name` segments become path_params.
    http_method: 'GET',       // required.
    // condition_function_id and middleware_function_ids are also supported.
  },
})
```

`:name` segments arrive as string `path_params`; `condition_function_id` gates the firing; `middleware_function_ids` runs per-route preHandlers after the global chain. For the `HttpRequest` / `HttpResponse` shapes, call `iii get function info` on the handler function id.

## Middleware

Register a middleware function when the same logic must run before multiple routes — authentication, rate limiting, request logging, header normalization. A middleware is a regular registered function that receives a subset of the request (no `body`) and returns one of two shapes: `{ action: 'continue', context? }` to proceed (optionally enriching `HttpRequest.context`), or `{ action: 'respond', response }` to short-circuit with an immediate `HttpResponse`.

Target middleware at routes two ways:

- Global — list ids in `iii-config.yaml` → `middleware:` (each `{ function_id, phase: preHandler, priority }`); runs on every matched route, sorted by `priority` ascending.
- Per-route — `middleware_function_ids` on the `http` trigger; runs after the global chain, in array order.

The full preHandler order per request: route match → global middleware → route `condition_function_id` → per-route middleware → body parsing → handler.

## Configuration

Worker config keys: `port` (default `3111`), `host` (default `0.0.0.0`), `default_timeout` (ms, default `30000`), `concurrency_request_limit` (default `1024`), `body_limit` (bytes, default `1048576`), `trust_proxy` (default `false`), `request_id_header` (default `x-request-id`), `ignore_trailing_slash` (default `false`), `not_found_function`, `cors.allowed_origins`, `cors.allowed_methods`, and the `middleware:` array described above.

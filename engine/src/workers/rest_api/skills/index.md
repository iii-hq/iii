---
type: index
title: iii-http
---

# iii-http

The `iii-http` worker exposes registered functions as HTTP endpoints. It runs an axum server on the configured `port`/`host`, routes incoming requests against `http` triggers (the only trigger type it provides), and invokes the bound function with an `HttpRequest` payload. The function returns an `HttpResponse` envelope (`status_code`/`headers`/`body`) which the worker serializes back to the wire — choosing JSON, plain text, or bytes based on the `Content-Type` header the handler sets.

The worker has **no callable functions**. Its entire surface is one trigger type (`http`) plus a middleware system that runs before each handler. Routes are registered through `iii.registerTrigger({ type: 'http', ... })`. Middleware is plain registered functions whose ids are listed in either the global config (`iii-config.yaml` → `middleware:`) or the per-trigger `middleware_function_ids` array; the worker invokes them in order before each request reaches its handler.

Worker config keys: `port` (default `3111`), `host` (default `0.0.0.0`), `default_timeout` (per-request cap in milliseconds, default `30000`), `concurrency_request_limit` (default `1024`), `body_limit` (max request body bytes, default `1048576` / 1 MB), `trust_proxy` (trust `X-Forwarded-For` and friends, default `false`), `request_id_header` (default `x-request-id`), `ignore_trailing_slash` (default `false`), `not_found_function` (function id invoked when no route matches; receives the same `HttpRequest` payload), `cors.allowed_origins`, `cors.allowed_methods`, and a `middleware:` array of global preHandler entries (see [Add request preHandler middleware](iii://iii-http/http/middleware) for the per-entry schema).

- **`http` trigger** — bind a registered function to an `(api_path, http_method)` pair. Optional `condition_function_id` gates the firing; optional `middleware_function_ids` runs per-route preHandlers.
- **Middleware functions** — preHandler hooks for cross-cutting concerns (auth, rate limiting, request logging). Each middleware returns either `{ action: "continue" }` to proceed or `{ action: "respond", response }` to short-circuit with an immediate response.

## How-tos

### `http` triggers

- [Expose a function as an HTTP endpoint](iii://iii-http/http/reactive-triggers) — register a handler and an `http` trigger to route an `(api_path, http_method)` pair to that handler, including the `HttpRequest` payload shape, the `HttpResponse` return contract, and the per-route condition/middleware hooks.
- [Add request preHandler middleware](iii://iii-http/http/middleware) — register middleware functions globally (via `iii-config.yaml`) or per-route (via `middleware_function_ids`) to run authentication, rate limiting, logging, or any other cross-cutting concern before the handler.

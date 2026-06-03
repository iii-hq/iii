---
name: iii-http
description: >-
  Expose registered functions as HTTP endpoints via an `http` trigger, with a
  preHandler middleware chain for auth, rate limiting, and logging. Reach for it
  to serve REST without standing up a separate web server.
---

# iii-http

The `iii-http` worker exposes registered functions as HTTP endpoints. It runs an HTTP server on the configured host/port, matches each inbound request against the `http` triggers you bind, and invokes the bound function with the request. The handler returns a response envelope that the worker serializes to the wire — JSON, plain text, or bytes, depending on the `Content-Type` the handler sets.

The worker has no callable functions of its own — you never invoke an `http::*` id. Its surface is the `http` trigger TYPE plus a middleware chain that runs before each handler. You expose an endpoint by registering a normal function and binding an `http` trigger to it.

## Do NOT stand up your own web server

This is the way to serve HTTP on iii, and the only one: register a function, then bind an `http` trigger to it. Do NOT write or run your own web server — no `express`, no `http.createServer`, no `fastify`, no framework listening on a port. iii does not route to a server you start yourself; such a process is unreachable as an iii endpoint, and running it as a foreground process (e.g. inside a sandbox) just hangs. If you are reaching for a web framework or a `listen()` call, stop — that instinct is from another ecosystem. The iii equivalent is one `iii.registerTrigger({ type: 'http', ... })` per route; this worker is the server.

## Get the contract from the engine

This page explains WHEN and HOW to serve HTTP; the engine is the source of truth for the exact shapes. Before you bind a trigger or write a handler, fetch the contract and build to it — do not guess field names, types, or defaults from memory:

```
engine::triggers::info { id: "http" }
```

returns the `http` trigger's configuration schema (the route fields — path, method, optional route gating, per-route middleware) and the request envelope your handler receives. For the response envelope your handler must return, inspect a bound handler with `engine::functions::info { function_id: "<your-handler-id>" }`. Use `engine::triggers::list` to discover trigger types in the first place.

## Set the right HTTP status

YOUR handler chooses the HTTP status code — it is a field on the response envelope (fetch its exact name/shape from the contract above; do not hardcode it from memory). The status is NOT inferred from the body, so a handler that just returns a value or an error object yields `200` by default. For error cases, set the matching status explicitly: a not-found path returns `404`, a bad request `400`, and so on. Returning `200` with an `{ error: ... }` body is a bug — it "isn't a 500," but it also isn't the status the requirement asked for, and callers (and tests) will read it as success. Avoiding a crash is not the same as returning the right status.

## When to Use

- Expose a function as a REST endpoint without standing up a separate HTTP server.
- You need URL path parameters or query strings parsed and handed to the handler.
- You want condition-gated routes or per-route preHandler middleware.
- You need a single not-found handler for unmatched routes.

## Boundaries

- No callable functions — the worker is never reached through an `http::*` id; routes are bound with `iii.registerTrigger`, not called.
- Two routes on the same path + method conflict; the router resolves to the most recently registered one.
- Query-string parsing is single-valued — a repeated key keeps only the last value; pre-encode multi-valued params yourself.
- Middleware runs BEFORE body parsing, so it sees headers, path, and query but not the body — use it for preconditions, not response logic (that belongs in the handler).

## Binding a route

Register your handler as a normal function, then bind an `http` trigger to it with `iii.registerTrigger` using the `http` trigger type. Shape the trigger config to what `engine::triggers::info { id: "http" }` describes — the route path and method, an optional gating function evaluated before the handler, and per-route middleware. Path segments you declare in the route arrive as string path parameters on the request; the gating function, if set, can veto a request before the handler runs.

## Where your route is served

Your bound route is served by the `iii-http` worker on its configured host and port — NOT on a port you pick. The default port is `3111` (host `0.0.0.0`), so a route with path `/example` is reachable at `http://localhost:3111/example` on a default deployment. Do NOT guess other ports (3000, 8080, the engine's WS port): if the deployment overrides the default, read the live host/port from the `iii-http` worker's configuration (`config.yaml`) or its status rather than probing. Verify a live endpoint with `web::fetch` against that base URL, never `curl`.

## Middleware

Register a middleware function when the same logic must run before multiple routes — authentication, rate limiting, request logging, header normalization. A middleware is a regular registered function that receives a subset of the request (without the body) and either lets the request proceed (optionally enriching its context) or short-circuits with an immediate response. Target middleware two ways:

- Globally — via the worker's middleware config in `config.yaml`; runs on every matched route, in priority order.
- Per-route — via the trigger's middleware list; runs after the global chain, in the order given.

The full preHandler order per request: route match → global middleware → route gating function → per-route middleware → body parsing → handler.

## Configuration

The worker is configured in `config.yaml`: the listen host and port, request timeout, concurrency and body-size limits, proxy trust and request-id header, trailing-slash handling, a not-found handler function, CORS, and the global middleware chain. For the full set of keys and their defaults, read the worker's configuration rather than hardcoding values from memory.

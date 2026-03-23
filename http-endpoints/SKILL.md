---
name: http-endpoints
description: >-
  Exposes iii functions as REST API endpoints using registerFunction and
  registerTrigger type:'http'. Use when building HTTP APIs, webhooks, or any
  inbound request handling where iii owns the route.
---

# HTTP Endpoints

Comparable to: Express, Fastify, Flask

## Key Concepts

- A **Function** handles the request logic and returns a response
- An **HTTP Trigger** binds a route (path + method) to that function
- The handler receives an `ApiRequest` with `body`, `path_params`, `headers`, `method`
- The handler returns `{ status_code, body, headers }`
- The `RestApiModule` in iii-config.yaml exposes the HTTP server (default port 3111)

## iii Primitives Used

| Primitive | Purpose |
|-----------|---------|
| `init(url)` | Connect worker to engine |
| `registerFunction({ id }, handler)` | Define the request handler |
| `registerTrigger({ type: 'http', function_id, config })` | Bind route to function |
| `config: { api_path, http_method }` | Route path and HTTP method |

## Common Patterns

- `init('ws://localhost:49134')` -- connect to engine
- `registerFunction({ id: 'users::create' }, async (req) => { ... })` -- handler receives `ApiRequest`
- `registerTrigger({ type: 'http', function_id: 'users::create', config: { api_path: '/users', http_method: 'POST' } })`
- Return `{ status_code: 200, body: { ... }, headers: { 'Content-Type': 'application/json' } }`
- Path parameters: `api_path: '/users/:id'` -- accessed via `req.path_params.id`

## Reference

See [references/REFERENCE.md](references/REFERENCE.md) for the full API reference with TypeScript, Python, Rust examples and module config.

## Pattern Boundaries

- If the task is about calling external HTTP endpoints as iii functions, use `http-invoked-functions`.
- If the task involves scheduled execution, use `cron-scheduling`.
- If the task needs async processing behind the endpoint, combine with `queue-processing`.
- Stay with `http-endpoints` when the goal is exposing inbound REST routes that iii owns.

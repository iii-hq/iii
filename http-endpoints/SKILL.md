---
name: http-endpoints
description: >-
  Exposes iii functions as REST API endpoints. Use when building HTTP APIs,
  webhooks, or inbound request handling where iii owns the route.
---

# HTTP Endpoints

Comparable to: Express, Fastify, Flask

## Key Concepts

Use the concepts below when they fit the task. Not every HTTP endpoint needs all of them.

- Each route is a **registered function** bound to a path and method via an HTTP trigger
- The handler receives an **ApiRequest** object containing `body`, `path_params`, `headers`, and `method`
- Handlers return `{ status_code, body, headers }` to shape the HTTP response
- **RestApiModule** serves all registered routes on port 3111
- Path parameters use colon syntax (e.g. `/users/:id`) and arrive in `path_params`

## Architecture

    HTTP request
      → RestApiModule (port 3111)
        → registerTrigger route match (method + path)
          → registerFunction handler (receives ApiRequest)
            → { status_code, body, headers } response

## iii Primitives Used

| Primitive                                           | Purpose                                    |
| --------------------------------------------------- | ------------------------------------------ |
| `registerFunction`                                  | Define the handler for a route             |
| `registerTrigger({ type: 'http' })`                 | Bind a route path and method to a function |
| `config: { api_path: '/path', http_method: 'GET' }` | Route configuration on the trigger         |

## Reference Implementation

See [../references/http-endpoints.js](../references/http-endpoints.js) for the full working example — a REST API with parameterized routes handling GET and POST requests.

## Common Patterns

Code using this pattern commonly includes, when relevant:

- `registerWorker(url, { workerName })` — worker initialization
- `registerFunction(id, handler)` — define the route handler
- `registerTrigger({ type: 'http', config: { api_path, http_method } })` — bind path and method
- `req.body` — parsed request body for POST/PUT
- `req.path_params` — extracted path parameters
- `return { status_code: 200, body: { data }, headers: { 'Content-Type': 'application/json' } }` — response shape
- `const logger = new Logger()` — structured logging per handler

## Adapting This Pattern

Use the adaptations below when they apply to the task.

- Add more routes by registering additional functions and HTTP triggers with distinct paths or methods
- Use `path_params` for resource identifiers (e.g. `/orders/:orderId`)
- Return appropriate status codes (201 for creation, 404 for not found, 400 for bad input)
- For authenticated routes, inspect `req.headers` for tokens or API keys
- Chain work behind an endpoint by enqueuing to a queue after returning a 202 Accepted

## Pattern Boundaries

- If the task is about calling external HTTP APIs from iii functions, prefer `http-invoked-functions`.
- If async processing is needed behind the endpoint, prefer `queue-processing` for the background work.
- Stay with `http-endpoints` when iii owns the route and handles the inbound request directly.

## When to Use

- Use this skill when the task is primarily about `http-endpoints` in the iii engine.
- Triggers when the request directly asks for this pattern or an equivalent implementation.

## Boundaries

- Never use this skill as a generic fallback for unrelated tasks.
- You must not apply this skill when a more specific iii skill is a better fit.
- Always verify environment and safety constraints before applying examples from this skill.

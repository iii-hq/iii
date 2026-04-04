---
name: iii-http-middleware
description: >-
  Registers engine-level middleware functions that run before HTTP handlers.
  Use when adding authentication, request logging, rate limiting, or any
  pre-handler logic to HTTP endpoints.
---

# HTTP Middleware

Comparable to: Express middleware, Fastify hooks, Django middleware

## Key Concepts

Use the concepts below when they fit the task. Not every middleware setup needs all of them.

- Middleware functions are **registered like normal functions** but return `{ action: 'continue' }` or `{ action: 'respond', response }` instead of a normal response
- Middleware is attached to HTTP triggers via `middleware_function_ids` in the trigger config
- The engine executes middleware in **order** — first middleware runs first, then the next, then the handler
- Middleware receives a `MiddlewareFunctionInput` with `phase`, `request` (path_params, query_params, headers, method), and `context` from auth
- Returning `{ action: 'respond' }` **short-circuits** the chain — the handler never runs
- Returning `{ action: 'continue' }` passes to the next middleware or the handler

## Architecture

    HTTP request
      → RestApiModule (port 3111)
        → Middleware 1 (continue / respond)
          → Middleware 2 (continue / respond)
            → registerFunction handler
              → { status_code, body, headers } response

## iii Primitives Used

| Primitive                                                    | Purpose                                          |
| ------------------------------------------------------------ | ------------------------------------------------ |
| `registerFunction(id, handler)`                              | Define a middleware function                      |
| `registerTrigger({ config: { middleware_function_ids } })`   | Attach middleware to an HTTP trigger              |
| `{ action: 'continue' }`                                    | Pass to next middleware or handler                |
| `{ action: 'respond', response: { status_code, body } }`   | Short-circuit and return response immediately     |
| `req.request.headers`                                        | Access request headers in middleware              |
| `req.context`                                                | Access auth context from RBAC auth function       |

## Reference Implementation

See [../references/http-middleware.js](../references/http-middleware.js) for the full working example — auth and logging middleware protecting HTTP endpoints.

## Common Patterns

Code using this pattern commonly includes, when relevant:

- `iii.registerFunction('middleware::auth', async (req) => { ... })` — auth middleware checking headers
- `iii.registerFunction('middleware::rate-limit', async (req) => { ... })` — rate limiting middleware
- `iii.registerFunction('middleware::request-logger', async (req) => { ... })` — request logging
- `req.request?.headers?.authorization` — reading auth tokens
- `return { action: 'respond', response: { status_code: 401, body: { error: 'Unauthorized' } } }` — reject request
- `return { action: 'continue' }` — allow request through
- `config: { middleware_function_ids: ['middleware::auth', 'middleware::logger'] }` — attach to trigger

## Adapting This Pattern

Use the adaptations below when they apply to the task.

- Chain multiple middleware for layered concerns (logging before auth before rate-limiting)
- Use middleware for cross-cutting concerns shared across multiple endpoints
- Combine with RBAC auth functions for role-based access control — auth context flows to middleware via `req.context`
- Keep middleware functions focused on one concern each for reusability

## Pattern Boundaries

- If the task is just exposing HTTP endpoints without middleware, prefer `iii-http-endpoints`.
- If auth needs are complex (RBAC with function discovery control), combine this with RBAC worker auth functions.
- Stay with `iii-http-middleware` when the primary need is pre-handler processing for HTTP routes.

## When to Use

- Use this skill when the task is primarily about `iii-http-middleware` in the iii engine.
- Triggers when the request directly asks for this pattern or an equivalent implementation.

## Boundaries

- Never use this skill as a generic fallback for unrelated tasks.
- You must not apply this skill when a more specific iii skill is a better fit.
- Always verify environment and safety constraints before applying examples from this skill.

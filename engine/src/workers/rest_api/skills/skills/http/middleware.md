---
type: how-to
trigger_type: http
title: Add request preHandler middleware
---

# When to use

Register a middleware function when the same logic must run before **multiple** route handlers — authentication, rate limiting, request logging, header normalization, IP allow-listing. Middleware sits between the worker's body parser and the handler; it inspects the request and either lets it proceed (with optional `context` enrichment) or short-circuits with an immediate response.

| Question                                                                              | Use this                                                            |
|---------------------------------------------------------------------------------------|---------------------------------------------------------------------|
| Should this run on **every** matched HTTP route?                                      | Global middleware (configured in `iii-config.yaml` → `middleware:`) |
| Should this run only on a specific subset of routes?                                  | Per-route middleware (`middleware_function_ids` on the trigger)     |
| Is the function the route's actual response logic, not a precondition?                | The handler itself — see [Expose a function as an HTTP endpoint](iii://iii-http/http/reactive-triggers) |
| Should it gate firing without supplying its own response?                             | The trigger's `condition_function_id` (returns truthy/falsy only)  |

Reach for it when:

- You need to enforce auth across many endpoints — register one auth middleware globally and have it short-circuit on missing/invalid bearer tokens.
- You want to attach a per-request value (verified user, tenant id, request id) to `context` so handlers receive it without re-parsing the request.
- You need rate-limiting or request-size accounting that runs uniformly without each handler having to call into a shared library.

# Inputs

A middleware is just a regular registered function whose function id is referenced by config. Two registration paths target it at routes:

## Global middleware

Configure on the worker in `iii-config.yaml`. Runs on **every** matching HTTP route, sorted by `priority` ascending (lower priority runs first).

```yaml
- name: iii-http
  config:
    middleware:
      - function_id: "global::rate-limiter"          # required. Function id of the middleware to invoke.
        phase: preHandler                            # optional. Currently only "preHandler" is supported. Defaults to "preHandler".
        priority: 5                                  # optional. Execution order, lower runs first. Defaults to 0.
      - function_id: "global::auth"
        phase: preHandler
        priority: 10
```

## Per-route middleware

Set `middleware_function_ids` on the `http` trigger config. Runs **after** the global chain, in array order.

```json
{
  "type":        "http",
  "function_id": "api::get-admin-data",
  "config": {
    "api_path":                "/admin/data",
    "http_method":             "GET",
    "middleware_function_ids": ["auth::require-admin", "audit::log-access"]   // invoked in order; either may short-circuit before the handler runs
  }
}
```

The full preHandler chain executes in this order on every request:

1. Route match (against `api_path` + `http_method`).
2. Global middleware (sorted by `priority` ascending).
3. Route condition (the trigger's optional `condition_function_id`).
4. Per-route middleware (in `middleware_function_ids` array order).
5. Body parsing.
6. Handler function.

Any middleware can stop the chain by returning `{ action: "respond", ... }`; subsequent middleware and the handler are skipped.

# Outputs

The middleware function receives a **subset** of the `HttpRequest` — body is omitted because middleware runs before body parsing:

```json
{
  "path":         "/users/123",                       // request path
  "method":       "GET",                              // HTTP method
  "path_params":  { "id": "123" },                    // path variables from :name segments
  "query_params": { "fields": "name,email" },         // query string
  "headers":      { "authorization": "Bearer ...", "content-type": "application/json" }  // request headers (lowercased keys)
}
```

The middleware must return one of two shapes:

```json
// Continue: pass control to the next middleware (or the handler when this is the last one).
{
  "action":  "continue",
  "context": { "user_id": "u_123", "scopes": ["read", "write"] }   // optional. Merged into HttpRequest.context for downstream middleware and the handler.
}
```

```json
// Respond: short-circuit with an immediate HTTP response. Subsequent middleware and the handler do not run.
{
  "action":   "respond",
  "response": {                                       // The same shape an HTTP handler returns — see "Outputs" in the http trigger how-to for full rules.
    "status_code": 401,
    "headers":     { "Content-Type": "application/json" },
    "body":        { "error": "missing_or_invalid_bearer" }
  }
}
```

- `action` is required and must be exactly `"continue"` or `"respond"`. Any other value (including a missing `action` field) is treated as an error and short-circuits the request with a 500.
- On `"continue"`, the optional `context` field is shallow-merged into `HttpRequest.context`; later middleware can read or extend it; the handler sees the final accumulated object.
- On `"respond"`, the `response` object follows the same `HttpResponse` rules as a regular handler — `status_code` defaults to 200 if omitted, headers can be object or array form, and Content-Type drives body serialization.

# Worked example

A bearer-auth middleware returns `respond` with 401 when the token is missing or invalid, and `continue` with `context` populated when it's valid:

```json
{
  "action":   "respond",
  "response": {
    "status_code": 401,
    "headers":     { "Content-Type": "application/json" },
    "body":        { "error": "missing_or_invalid_bearer" }
  }
}
```

Three patterns reach for middleware in different ways:

- **Global auth.** Register an auth middleware whose function id is in the global `middleware:` list with `phase: preHandler`. It inspects `headers.authorization`, returns the `respond` shape above on missing/invalid tokens, or `{ "action": "continue", "context": { "user_id": "u_1", "scopes": ["read", "write"] } }` on success — every handler then receives `HttpRequest.context.user_id` without parsing the header itself.
- **Per-route admin gate.** On routes that need elevated privilege, set `middleware_function_ids: ["auth::require-admin"]` on the trigger config. The route's `condition_function_id` is the right place for cheap boolean checks; middleware is the right place when you need to enrich `context` or return a structured error response.
- **Logging + rate-limit chain.** Stack multiple middleware globally with explicit priorities (`priority: 5` for rate-limit before `priority: 10` for auth), so the cheaper check runs first and rejects 429s before authentication work happens.

For runnable scaffolds in TypeScript, Python, and Rust, see the http worker source and the SDK usage examples in [the iii main repo](https://github.com/iii-hq/iii).

# Related

- [Expose a function as an HTTP endpoint](iii://iii-http/http/reactive-triggers) — the route how-to; `middleware_function_ids` is documented there as part of the trigger config.
- `iii-http` worker config (`middleware:` block) — each entry is `{ function_id: <required>, phase: "preHandler" (default; only supported value), priority: <integer, default 0> }`. Lower `priority` runs first across the global chain.
- `condition_function_id` (on the http trigger) — the lighter-weight gate when you only need a boolean check and don't need to attach context or shape the error response.

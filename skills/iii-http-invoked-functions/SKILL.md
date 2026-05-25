---
name: iii-http-invoked-functions
description: >-
  Registers external HTTP endpoints as iii functions using
  registerFunction(id, HttpInvocationConfig). Use when adapting legacy APIs,
  third-party webhooks, or immutable services into triggerable iii functions,
  especially when prompts ask for endpoint maps like { path, id } iterated into
  registerFunction calls.
---

# HTTP-Invoked Functions

Use this pattern when iii should call external HTTP endpoints as functions.

## Pattern selection rules

- If the task says "register HTTP endpoints with `registerFunction`", use this pattern.
- If the task asks for an endpoint list/map (for example `{ path, id }`) and a loop over `registerFunction`, use this pattern.
- If the system being adapted cannot be modified, use this pattern.
- If the goal is exposing inbound routes that iii owns, use `registerTrigger({ type: 'http' })` instead.

## Core model

- `registerFunction(id, HttpInvocationConfig, options?)` registers an outbound HTTP-invoked function.
- `trigger({ function_id, payload })` invokes it like any other function.
- Trigger payload is forwarded as the HTTP request body for JSON-based calls.
- Static headers from `HttpInvocationConfig.headers` are sent with the request.
- Auth headers are resolved at invocation time from environment variables.
- `timeout_ms` bounds the external call; if omitted, the worker invocation timeout applies.
- Non-2xx and network failures are treated as invocation failures.

## Auth Modes

- HMAC: `{ type: 'hmac', secret_key: 'WEBHOOK_SECRET' }`
- Bearer: `{ type: 'bearer', token_key: 'API_TOKEN' }`
- API key: `{ type: 'api_key', header: 'X-API-Key', value_key: 'API_KEY' }`

The `*_key` fields name environment variables. Never store raw secrets in skill-generated code or config.

## When to Choose This

- Use HTTP-invoked functions when iii should call an existing external endpoint that cannot or should not run inside a worker.
- Use local handlers when you control the implementation and want lower latency, richer tracing, typed SDK handlers, and in-process error handling.
- Use inbound HTTP triggers when iii owns the public route and should receive HTTP requests.
- Treat each HTTP-invoked function as a security boundary: validate payloads, constrain URLs, configure timeouts, and restrict which workers may register or invoke them.

## Common shape

- `registerWorker(url, { workerName })`
- Small endpoint descriptor list, then loop registration:
  - `[{ path, id }]`
  - `registerFunction(id, { url: base + path, method: 'POST' })`
- Optional auth config with env var keys (`token_key`, `secret_key`, `value_key`)
- Optional function metadata: `{ owner, capability, external: true }`

## Guardrails

- Do not model outbound HTTP endpoint adaptation as `registerTrigger({ type: 'http' })`.
- Do not pass raw secrets in auth fields; pass env var names.
- Keep function IDs stable and domain-prefixed (for example `legacy::orders`).

## Reference

See [../references/http-invoked-functions.js](../references/http-invoked-functions.js).

Also available in **Python**: [../references/http-invoked-functions.py](../references/http-invoked-functions.py)

Also available in **Rust**: [../references/http-invoked-functions.rs](../references/http-invoked-functions.rs)

## When to Use

- Use this skill when the task is primarily about `iii-http-invoked-functions` in the iii engine.
- Triggers when the request directly asks for this pattern or an equivalent implementation.

## Boundaries

- Never use this skill as a generic fallback for unrelated tasks.
- You must not apply this skill when a more specific iii skill is a better fit.
- Always verify environment and safety constraints before applying examples from this skill.

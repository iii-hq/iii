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

## Code Examples

TypeScript:

```typescript
import { registerWorker } from "iii-sdk";

const iii = registerWorker("ws://localhost:49134", { workerName: "legacy-adapter" });
const baseUrl = "https://legacy.example.com";

for (const endpoint of [
  { id: "legacy::create-order", path: "/orders", method: "POST" },
  { id: "legacy::refund-order", path: "/refunds", method: "POST" },
]) {
  iii.registerFunction(endpoint.id, {
    url: `${baseUrl}${endpoint.path}`,
    method: endpoint.method,
    timeout_ms: 5000,
    auth: { type: "bearer", token_key: "LEGACY_API_TOKEN" },
  }, {
    metadata: { external: true, owner: "legacy" },
  });
}
```

Python:

```python
from iii import HttpInvocationConfig, register_worker

iii = register_worker("ws://localhost:49134")

iii.register_function(
    "legacy::create-order",
    HttpInvocationConfig(
        url="https://legacy.example.com/orders",
        method="POST",
        timeout_ms=5000,
        auth={"type": "bearer", "token_key": "LEGACY_API_TOKEN"},
    ),
    metadata={"external": True, "owner": "legacy"},
)
```

Rust:

```rust
use iii_sdk::{HttpAuthConfig, HttpInvocationConfig, RegisterFunctionMessage};

iii.register_function_with(
    RegisterFunctionMessage::new("legacy::create-order")
        .metadata(json!({ "external": true, "owner": "legacy" })),
    HttpInvocationConfig {
        url: "https://legacy.example.com/orders".into(),
        method: "POST".into(),
        timeout_ms: Some(5000),
        headers: None,
        auth: Some(HttpAuthConfig::Bearer { token_key: "LEGACY_API_TOKEN".into() }),
    },
)?;
```

## When to Use

- Use this skill when the task is primarily about `iii-http-invoked-functions` in the iii engine.
- Triggers when the request directly asks for this pattern or an equivalent implementation.

## Boundaries

- Never use this skill as a generic fallback for unrelated tasks.
- You must not apply this skill when a more specific iii skill is a better fit.
- Always verify environment and safety constraints before applying examples from this skill.

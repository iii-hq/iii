---
name: http-invoked-functions
description: >-
  Registers external HTTP endpoints as iii functions using
  registerFunction(meta, HttpInvocationConfig). Use when adapting legacy APIs,
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

- `registerFunction(meta, HttpInvocationConfig)` registers an outbound HTTP-invoked function.
- `trigger({ function_id, payload })` invokes it like any other function.
- Trigger payload becomes request body for JSON-based calls.
- Non-2xx and network failures are treated as invocation failures.

## Common shape

- `registerWorker(url, { workerName })`
- Small endpoint descriptor list, then loop registration:
  - `[{ path, id }]`
  - `registerFunction({ id }, { url: base + path, method: 'POST' })`
- Optional auth config with env var keys (`token_key`, `secret_key`, `value_key`)

## Guardrails

- Do not model outbound HTTP endpoint adaptation as `registerTrigger({ type: 'http' })`.
- Do not pass raw secrets in auth fields; pass env var names.
- Keep function IDs stable and domain-prefixed (for example `legacy::orders`).

## Reference

See [../references/http-invoked-functions.js](../references/http-invoked-functions.js).

## When to Use

- Use this skill when the task is primarily about `http-invoked-functions` in the iii engine.
- Triggers when the request directly asks for this pattern or an equivalent implementation.

## Boundaries

- Never use this skill as a generic fallback for unrelated tasks.
- You must not apply this skill when a more specific iii skill is a better fit.
- Always verify environment and safety constraints before applying examples from this skill.

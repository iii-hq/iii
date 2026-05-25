---
name: iii-worker-rbac
description: >-
  Configure worker RBAC, filtered discovery, FORBIDDEN errors, registration
  hooks, auth functions, and safe browser/public worker-manager listeners in
  iii. Use when configuring permissions, tenant isolation, exposed functions,
  browser-safe worker access, or denied invocation behavior.
---

# Worker RBAC

RBAC belongs on `iii-worker-manager` listeners. Use it to expose a constrained function surface to a worker or browser client while keeping the private engine worker port internal.

## Core Concepts

- Public or untrusted clients connect to an RBAC-enabled worker-manager listener, not the private worker port.
- `auth_function_id` receives request connection data and returns an `AuthResult`.
- Discovery is filtered: callers only see functions and trigger types they are allowed to use.
- `forbidden_functions` override allowed lists and exposure filters.
- Denied invocation returns `FORBIDDEN`.
- A missing function and a forbidden function are different: missing means the engine has no callable registration; forbidden means policy denied access.

## AuthInput

Auth functions receive:

- `headers`
- `query_params`
- `ip_address`

Browsers cannot reliably send custom WebSocket headers, so browser auth usually comes from query params, cookies, or upstream session context.

## AuthResult

Return:

- `allowed_functions: string[]`
- `forbidden_functions: string[]`
- `allowed_trigger_types?: string[]`
- `allow_trigger_type_registration: boolean`
- `allow_function_registration?: boolean`
- `function_registration_prefix?: string`
- `context: Record<string, unknown>`

## Access Resolution

1. If a function matches `forbidden_functions`, deny.
2. If it matches `allowed_functions`, allow.
3. Allow `engine::channels::create` so channels can be created on RBAC listeners.
4. If it matches configured `expose_functions`, allow.
5. Otherwise deny.

## Registration Hooks

RBAC can affect registration as well as invocation:

- `on_trigger_registration_function_id`
- `on_trigger_type_registration_function_id`
- `on_function_registration_function_id`

Use hooks to approve, deny, rewrite metadata, or enforce naming/prefix policy for registrations made through the listener.

`function_registration_prefix` transparently prefixes registered functions from that session. Agents should call the discovered/prefixed function IDs returned by discovery, not guessed unprefixed IDs.

## Common Config Shape

```yaml
workers:
  - name: iii-worker-manager
    config:
      listeners:
        - host: 127.0.0.1
          port: 49135
          rbac:
            auth_function_id: auth::browser-session
            expose_functions:
              - match("public::*")
            on_function_registration_function_id: policy::function-registration
```

## Agent Behavior

- Call discovery functions through the same listener the target client uses.
- Treat `FORBIDDEN` as policy denial, not a retryable missing-worker problem.
- Do not suggest installing a worker when the function exists but is forbidden.
- When registration is denied, inspect registration hook results and policy config.
- Use exposed metadata and allowed lists to choose callable functions.

## Pattern Boundaries

- For SDK error types, prefer `iii-error-handling`.
- For browser connection behavior, prefer `iii-browser-sdk`.
- For engine config layout, prefer `iii-engine-config`.

## When to Use

- Use this skill when the task mentions RBAC, permissions, auth functions, filtered discovery, `FORBIDDEN`, exposed functions, registration hooks, browser-safe worker access, or tenant-isolated function surfaces.

## Boundaries

- Do not expose the private worker WebSocket port to untrusted clients.
- Do not hide policy denial by treating every failure as `function_not_found`.
- Do not generate removed service APIs or adapter-extension APIs.

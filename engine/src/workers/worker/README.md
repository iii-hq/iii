# iii-worker-manager

Mandatory engine worker that opens WebSocket listeners for SDK workers to connect to. The first
`iii-worker-manager` entry sets the main engine port (default `49134`); `#instance` entries start
independent listeners, typically a public RBAC listener. Channel WebSocket endpoints are mounted
on every listener at `/ws/channels/{channel_id}`.

## Skills

Install the `iii-worker-manager` agent skill for Claude Code, Cursor, and 30+ other agents:

```bash
npx skills add iii-hq/iii --full-depth --skill iii-worker-manager
```

## Sample Configuration

```yaml
engine:
  workers:
    # Main engine port — internal worker-to-worker traffic.
    iii-worker-manager:
      port: 49134

    # Public RBAC listener — auth, middleware, and gated registration.
    "iii-worker-manager#rbac":
      host: 0.0.0.0
      port: 49135
      middleware_function_id: my-project::middleware-function
      rbac:
        auth_function_id: my-project::auth-function
        on_function_registration_function_id: my-project::on-function-reg
        on_observer_registration_function_id: my-project::on-observer-reg
        expose_functions:
          - match("api::*")
          - match("*::public")
          - metadata:
              public: true
```

## Configuration

| Field | Type | Description |
|---|---|---|
| `port` | integer | Port to bind. Defaults to `49134`. The first `iii-worker-manager` entry in the config sets the main engine port; additional entries start separate listeners. |
| `host` | string | Host to bind. Defaults to `0.0.0.0`. |
| `middleware_function_id` | string | Function ID invoked before every worker call on this listener. Receives `MiddlewareFunctionInput`; the middleware is responsible for invoking the target function and returning its result. Works with or without RBAC. |
| `rbac` | RbacConfig | RBAC block. When present, the listener applies role-based access control to every connection. |

## Multiple Listeners

`iii-worker-manager` is a `mandatory` worker. Multiple entries are supported and each opens a separate listener with its own port, host, middleware, and RBAC config. The typical production shape is one internal listener on `49134` for trusted workers and one external RBAC listener on a different port for untrusted clients.

## Middleware

When `middleware_function_id` is set, every invocation routed through this listener is delivered to the middleware instead of the target function. The middleware decides whether to invoke the target (via `iii.call`) and what to return.

### MiddlewareFunctionInput

| Field | Type | Description |
|---|---|---|
| `function_id` | string | The function the worker wants to invoke. |
| `payload` | object | The payload the worker sent. |
| `action` | CallAction or omitted | Routing action (`enqueue`, `void`), if any. |
| `context` | object | Auth context from the session's `AuthResult.context`. Empty object when RBAC is not configured. |

## RBAC

When the `rbac` block is present, the listener authenticates every connection against `auth_function_id`, applies the resulting allow/deny rules to every invocation, and routes registration requests through optional hook functions.

### RBAC Configuration

| Field | Type | Description |
|---|---|---|
| `auth_function_id` | string | Function ID called once per WebSocket upgrade. Receives `AuthInput`, returns `AuthResult`. If unset, every connection is allowed and `expose_functions` alone gates access. |
| `expose_functions` | FunctionFilter[] | List of filters. A function is exposed if **any** filter matches. Empty list = no functions exposed (other than the infrastructure carve-out below). |
| `on_function_registration_function_id` | string | Hook called before each `registerFunction` from the worker. Returns mapped fields or throws to deny. |
| `on_observer_registration_function_id` | string | Hook called before each `observe` from the worker. Returns mapped fields or throws to deny. |

### Function Filters

Two filter shapes are supported. They can be mixed in the same `expose_functions` list.

#### Wildcard match

`match("pattern")` with `*` matching any number of characters. Anchored at both ends.

```yaml
expose_functions:
  - match("api::*")          # api::users::list, api::orders::create, …
  - match("*::public")       # anything ending in ::public
  - match("api::*::read")    # api::users::read, api::orders::read, …
  - match("*")               # everything
```

#### Metadata match

Match against the function's registered `metadata`. All keys in the filter must match (AND); multiple filters in `expose_functions` are OR'd.

```yaml
expose_functions:
  - metadata:
      public: true                  # metadata.public === true
  - metadata:
      tier: free                    # metadata.tier === "free"
      name: match("*public*")       # metadata.name contains "public"
```

### Authentication

`auth_function_id` runs once per WebSocket upgrade. If it throws or returns no result, the connection is rejected.

#### AuthInput

| Field | Type | Description |
|---|---|---|
| `headers` | `Record<string, string>` | HTTP headers from the WebSocket upgrade request. |
| `query_params` | `Record<string, string[]>` | Query parameters. Each key maps to an array of values to support repeated keys. |
| `ip_address` | string | IP address of the connecting client. |

#### AuthResult

| Field | Type | Default | Description |
|---|---|---|---|
| `allowed_functions` | string[] | `[]` | Additional function IDs to allow beyond `expose_functions`. |
| `forbidden_functions` | string[] | `[]` | Function IDs to deny even when they match `expose_functions`. Takes precedence over everything else. |
| `allow_function_registration` | boolean | `true` | Whether the worker may register new functions. |
| `function_registration_prefix` | string or omitted | omitted | When set, function IDs registered by this worker are prefixed with `{prefix}::`, and so is the target function of each observer it creates. The prefix is stripped when invoking the worker, so the worker SDK never sees it. |
| `context` | object | `{}` | Arbitrary context forwarded to `middleware_function_id` and registration hooks on every call from this session. |

### Access Resolution Order

Every invocation through an RBAC listener walks this decision flow:

1. If `function_id` is in `forbidden_functions` → **deny**.
2. If `function_id` is in `allowed_functions` → **allow**.
3. If `function_id` is one of the always-allowed infrastructure functions → **allow** (carve-out below).
4. If any `expose_functions` filter matches → **allow**.
5. Otherwise → **deny**.

The infrastructure carve-out is a fixed slice of function IDs that an RBAC listener always allows so connection setup, logging, and context propagation keep working regardless of the operator's filters:

```
engine::channels::create
engine::workers::register
engine::log::info
engine::log::warn
engine::log::error
engine::log::debug
engine::log::trace
engine::baggage::get
engine::baggage::set
engine::baggage::get_all
```

The carve-out is part of the worker's public contract: within a major version it is additive-only. Adding a function ID to `forbidden_functions` still denies it (rule 1) — but doing so for an infrastructure ID logs a warning, and the worker may behave unpredictably (broken connection setup, lost logs, missing context).

### Function Registration Prefix

When `AuthResult.function_registration_prefix` is set, the engine transparently prefixes every function ID this session registers with `{prefix}::`. The target function of each observer the session creates is prefixed too. When the engine dispatches an invocation back to the worker, the prefix is stripped so the worker SDK finds the local handler.

This gives every authenticated session a private namespace without the worker code having to manage prefixes.

### Registration Hooks

Each hook receives the registration details plus `AuthResult.context`. Return a result object with the (possibly mapped) fields to allow the registration; throw to deny. Omitted result fields keep the original value.

#### OnFunctionRegistrationInput / Result

| Input field | Type | Description |
|---|---|---|
| `function_id` | string | ID being registered (after `function_registration_prefix`, if any). |
| `description` | string or omitted | Description supplied by the worker. |
| `metadata` | object or omitted | Metadata supplied by the worker. |
| `namespace` | string | Namespace the function is registered in. |
| `private` | boolean | `true` when only the registering worker may call the function. |
| `observable` | boolean | `false` when nobody may observe the function. |
| `args_format` | object or omitted | JSON Schema of the `args` observers pass. |
| `context` | object | Auth context for this session. |

| Result field | Type | Description |
|---|---|---|
| `function_id` | string or omitted | Mapped function ID. |
| `description` | string or omitted | Mapped description. |
| `metadata` | object or omitted | Mapped metadata. |
| `private` | boolean or omitted | Mapped `private`. |
| `observable` | boolean or omitted | Mapped `observable`. |
| `args_format` | object or omitted | Mapped `args_format`. |

#### OnObserverRegistrationInput / Result

| Input field | Type | Description |
|---|---|---|
| `observer_id` | string | ID of the observer being created. |
| `function_id` | string | The observed function. |
| `namespace` | string | Namespace of the observed function. |
| `args` | object | Args for the observed function's worker. |
| `filters` | object or omitted | Filters that pick the calls reaching the target. |
| `target_function_id` | string | The function invoked on each matching call. |
| `target_namespace` | string | Namespace of the target function. |
| `metadata` | object or omitted | Metadata sent with each activation. |
| `context` | object | Auth context for this session. |

| Result field | Type | Description |
|---|---|---|
| `function_id` | string or omitted | Mapped observed function. |
| `target_function_id` | string or omitted | Mapped target function. |
| `args` | object or omitted | Mapped args. |
| `filters` | object or omitted | Mapped filters. |
| `metadata` | object or omitted | Mapped metadata. |

A denied observer ends in `error` with code `FORBIDDEN`. An event source is a function: registering one is gated by `allow_function_registration` and the function registration hook. Functions registered by a session are removed when the worker disconnects; the observers it created are disposed when it does not reconnect within `observers.observer_grace_ms`.

## Channels

Every `iii-worker-manager` listener mounts the channel WebSocket endpoint at `/ws/channels/{channel_id}` on the same port. SDK workers can use `createChannel()` without any extra configuration — channel data flows through whichever listener the worker is connected to. `engine::channels::create` is part of the always-allowed infrastructure carve-out so RBAC listeners can create channels even with empty `expose_functions`. Channel WebSocket access is independently validated by the `access_key` capability token returned with each `StreamChannelRef`.

A writer ends its stream with a close frame that has no code or code 1000. Any other end of the writer socket (another close code, a lost connection, or the channel owner disconnecting before the writer attached) reaches the reader as close code 1011 after the data sent before it; a clean end reaches it as a close frame with no code.

## Sample Code

A TypeScript auth function that validates a bearer token, denies destructive ops to read-only roles, and lets only admins register functions:

```typescript
import type { AuthInput, AuthResult } from 'iii-sdk'
import { registerWorker } from 'iii-sdk'

const iii = registerWorker('ws://localhost:49134')

iii.registerFunction(
  'my-project::auth-function',
  async (input: AuthInput): Promise<AuthResult> => {
    const token = input.headers?.['authorization']?.replace(/^Bearer\s+/i, '')
    const apiKey = input.query_params?.['api_key']?.[0]

    if (!token && !apiKey) {
      throw new Error('Missing credentials')
    }

    const user = await validateCredentials(token || apiKey)

    return {
      allowed_functions: [],
      forbidden_functions: user.role === 'readonly'
        ? ['api::users::delete', 'api::users::update']
        : [],
      allow_function_registration: user.role === 'admin',
      context: {
        user_id: user.id,
        role: user.role,
      },
    }
  },
)
```

## Security Considerations

- The main engine port (the first `iii-worker-manager` entry) should remain internal. Only RBAC-protected listeners belong on external networks. Use firewall rules or network policies to enforce this.
- Always set `auth_function_id` on listeners that face untrusted networks. A listener with no `rbac` block authenticates nothing.
- Prefer narrow `expose_functions` patterns over `match("*")`. Audit the list whenever a new namespace is added to the engine.
- `forbidden_functions` from the auth result is the hard-deny mechanism — use it for per-user/per-role denylists that the operator's `expose_functions` cannot override.
- The middleware is the right place for request validation, rate limiting, and audit logging. Keep it idempotent so retries do not double-charge or double-log.
- Functions and observers registered through an RBAC session are scoped to that session: its functions are removed on disconnect, and its observers are disposed when it does not reconnect within `observers.observer_grace_ms`.

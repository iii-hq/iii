# iii-worker-manager

Mandatory engine worker that opens WebSocket listeners for SDK workers to connect to. The first `iii-worker-manager` entry in `iii-config.yaml` sets the main engine port (default `49134`); additional entries start independent listeners — typically a public RBAC listener with its own auth, middleware, and registration hooks. Channel WebSocket endpoints are mounted on every listener at `/ws/channels/{channel_id}`.

## Install

```bash
iii worker add iii-worker-manager
```

Resolves from the worker registry at [workers.iii.dev](https://workers.iii.dev/).

## Skills

Install the `iii-worker-manager` agent skill for Claude Code, Cursor, and 30+ other agents:

```bash
npx skills add iii-hq/iii --skill iii-worker-manager
```

## Sample Configuration

```yaml
workers:
  # Main engine port — internal worker-to-worker traffic.
  - name: iii-worker-manager
    config:
      port: 49134

  # Public RBAC listener — auth, middleware, and gated registration.
  - name: iii-worker-manager
    config:
      host: 0.0.0.0
      port: 49135
      middleware_function_id: my-project::middleware-function
      rbac:
        auth_function_id: my-project::auth-function
        on_function_registration_function_id: my-project::on-function-reg
        on_trigger_registration_function_id: my-project::on-trigger-reg
        on_trigger_type_registration_function_id: my-project::on-trigger-type-reg
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

When `middleware_function_id` is set, every invocation routed through this listener is delivered to the middleware instead of the target function. The middleware decides whether to invoke the target (via `iii.trigger`) and what to return.

### MiddlewareFunctionInput

| Field | Type | Description |
|---|---|---|
| `function_id` | string | The function the worker wants to invoke. |
| `payload` | object | The payload the worker sent. |
| `action` | TriggerAction or omitted | Routing action (`enqueue`, `void`), if any. |
| `context` | object | Auth context from the session's `AuthResult.context`. Empty object when RBAC is not configured. |

## RBAC

When the `rbac` block is present, the listener authenticates every connection against `auth_function_id`, applies the resulting allow/deny rules to every invocation, and routes registration requests through optional hook functions.

### RBAC Configuration

| Field | Type | Description |
|---|---|---|
| `auth_function_id` | string | Function ID called once per WebSocket upgrade. Receives `AuthInput`, returns `AuthResult`. If unset, every connection is allowed and `expose_functions` alone gates access. |
| `expose_functions` | FunctionFilter[] | List of filters. A function is exposed if **any** filter matches. Empty list = no functions exposed (other than the infrastructure carve-out below). |
| `on_function_registration_function_id` | string | Hook called before each `registerFunction` from the worker. Returns mapped fields or throws to deny. |
| `on_trigger_registration_function_id` | string | Hook called before each `registerTrigger`. Returns mapped fields or throws to deny. |
| `on_trigger_type_registration_function_id` | string | Hook called before each `registerTriggerType`. Returns mapped fields or throws to deny. |

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
| `allowed_trigger_types` | string[] or omitted | omitted (permissive) | Trigger type IDs the worker may register triggers for. When omitted, all types are allowed. |
| `allow_trigger_type_registration` | boolean | `false` | Whether the worker may register new trigger types. |
| `allow_function_registration` | boolean | `true` | Whether the worker may register new functions. |
| `function_registration_prefix` | string or omitted | omitted | When set, function IDs registered by this worker are prefixed with `{prefix}::` and trigger registrations auto-prefix the `function_id` they reference. The prefix is stripped when invoking the worker, so the worker SDK never sees it. |
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

When `AuthResult.function_registration_prefix` is set, the engine transparently prefixes every function ID this session registers with `{prefix}::`. Trigger registrations also auto-prefix the `function_id` they reference. When the engine dispatches an invocation back to the worker, the prefix is stripped so the worker SDK finds the local handler.

This gives every authenticated session a private namespace without the worker code having to manage prefixes.

### Registration Hooks

Each hook receives the registration details plus `AuthResult.context`. Return a result object with the (possibly mapped) fields to allow the registration; throw to deny. Omitted result fields keep the original value.

#### OnFunctionRegistrationInput / Result

| Input field | Type | Description |
|---|---|---|
| `function_id` | string | ID being registered (after `function_registration_prefix`, if any). |
| `description` | string or omitted | Description supplied by the worker. |
| `metadata` | object or omitted | Metadata supplied by the worker. |
| `context` | object | Auth context for this session. |

| Result field | Type | Description |
|---|---|---|
| `function_id` | string or omitted | Mapped function ID. |
| `description` | string or omitted | Mapped description. |
| `metadata` | object or omitted | Mapped metadata. |

#### OnTriggerRegistrationInput / Result

| Input field | Type | Description |
|---|---|---|
| `trigger_id` | string | ID of the trigger being registered. |
| `trigger_type` | string | Trigger type identifier. |
| `function_id` | string | Function ID this trigger is bound to. |
| `config` | unknown | Trigger-specific configuration. |
| `context` | object | Auth context for this session. |

| Result field | Type | Description |
|---|---|---|
| `trigger_id` | string or omitted | Mapped trigger ID. |
| `trigger_type` | string or omitted | Mapped trigger type. |
| `function_id` | string or omitted | Mapped function ID. |
| `config` | unknown or omitted | Mapped configuration. |

#### OnTriggerTypeRegistrationInput / Result

| Input field | Type | Description |
|---|---|---|
| `trigger_type_id` | string | ID of the trigger type being registered. |
| `description` | string | Human-readable description of the trigger type. |
| `context` | object | Auth context for this session. |

| Result field | Type | Description |
|---|---|---|
| `trigger_type_id` | string or omitted | Mapped trigger type ID. |
| `description` | string or omitted | Mapped description. |

A worker can register a trigger type only when `allow_trigger_type_registration` is `true` AND the hook (if configured) returns a result. A worker can register a trigger only when its `trigger_type` is in `allowed_trigger_types` (or the field is omitted) AND the hook (if configured) returns a result. Triggers registered by a session are cleaned up automatically when the worker disconnects.

## Channels

Every `iii-worker-manager` listener mounts the channel WebSocket endpoint at `/ws/channels/{channel_id}` on the same port. SDK workers can use `createChannel()` without any extra configuration — channel data flows through whichever listener the worker is connected to. `engine::channels::create` is part of the always-allowed infrastructure carve-out so RBAC listeners can create channels even with empty `expose_functions`. Channel WebSocket access is independently validated by the `access_key` capability token returned with each `StreamChannelRef`.

## Sample Code

A TypeScript auth function that validates a bearer token, denies destructive ops to read-only roles, and gates trigger-type registration to admins:

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
      allowed_trigger_types: user.role === 'admin'
        ? ['cron', 'webhook']
        : undefined,
      allow_trigger_type_registration: user.role === 'admin',
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
- Triggers and functions registered through an RBAC session are scoped to that session and cleaned up on disconnect.

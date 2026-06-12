# iii-http

The HTTP Worker exposes registered functions as HTTP endpoints.

## Install

```bash
iii worker add iii-http
```

Resolves from the worker registry at [workers.iii.dev](https://workers.iii.dev/).

## Skills

Install the `iii-http` agent skill for Claude Code, Cursor, and 30+ other agents:

```bash
npx skills add iii-hq/iii --full-depth --skill iii-http
```

## Configure

Runtime settings live in the **`configuration` worker** under id **`iii-http`**. The worker registers its JSON Schema at startup, reads the live value via `configuration::get` (so `${VAR:default}` placeholders in string fields expand against the process env), and hot-applies changes when the value updates.

The config.yaml block below is **seed-only**: it is installed as the initial value on first boot, when no value is stored yet. After that, the configuration worker entry is the source of truth — change settings via `configuration::set` or by editing the persisted file (`./data/configuration/iii-http.yaml` with the default `fs` adapter); both propagate without an engine restart. Edits to the config.yaml block are ignored once a value is stored.

### Sample Seed Configuration

```yaml
- name: iii-http
  config:
    port: 3111
    host: 0.0.0.0
    cors:
      allowed_origins:
        - http://localhost:3000
        - http://localhost:5173
      allowed_methods:
        - GET
        - POST
        - PUT
        - DELETE
        - OPTIONS
```

### Hot Reload

When the `iii-http` configuration changes, the worker re-reads the authoritative value and applies it in place:

- `cors`, `default_timeout`, `concurrency_request_limit`, and `middleware` are swapped without dropping the listener.
- A `host`/`port` change binds the new address first and tears the old listener down only once the new one is live; in-flight requests on the old listener are aborted. If the new address cannot be bound, the change is rejected and the previous server keeps running.
- Invalid values are rejected by schema validation at `configuration::set` time; a stored value that fails to deserialize is logged and the previous config is kept.

Note: `${VAR:default}` placeholders only work in string fields (e.g. `host`) — integer fields like `port` are validated as integers against the schema.

## Configuration

| Field | Type | Description |
|---|---|---|
| `port` | number | The port to listen on. Defaults to `3111`. |
| `host` | string | The host to listen on. Defaults to `0.0.0.0`. |
| `default_timeout` | number | Default timeout in milliseconds for request processing. Defaults to `30000`. |
| `concurrency_request_limit` | number | Maximum number of concurrent requests. Must be ≥ 1 (the schema rejects 0). Defaults to `1024`. |
| `cors.allowed_origins` | string[] | Allowed CORS origins. An empty list allows **any** origin; list origins explicitly to restrict. |
| `cors.allowed_methods` | string[] | Allowed CORS methods. An empty list allows **any** method; list methods explicitly to restrict. |
| `middleware` | Middleware[] | Global middleware run on every route (see [Middleware](#middleware)). |

## Trigger Type: `http`

Register a trigger with type `http` to expose a function as an HTTP endpoint.

| Field | Type | Description |
|---|---|---|
| `api_path` | string | Required. The URL path. |
| `http_method` | string | Required. The HTTP method. |
| `condition_function_id` | string | Function ID for conditional execution. If it returns `false`, the handler is skipped. |
| `middleware_function_ids` | string[] | Per-route middleware function IDs, invoked in order before the handler. |

### Sample Code

```typescript
const fn = iii.registerFunction('api::getUsers', handler)
iii.registerTrigger({
  type: 'http',
  function_id: fn.id,
  config: {
    api_path: '/api/v1/users',
    http_method: 'GET',
  },
})
```

## Request & Response Objects

### ApiRequest

| Field | Type | Description |
|---|---|---|
| `path` | string | The request path. |
| `method` | string | The HTTP method (e.g., `GET`, `POST`). |
| `path_params` | Record\<string, string\> | Variables extracted from the URL path (e.g., `/users/:id`). |
| `query_params` | Record\<string, string\> | URL query string parameters. |
| `body` | any | The parsed request body (JSON). |
| `headers` | Record\<string, string\> | HTTP request headers. |
| `trigger` | object | Trigger metadata: `type`, `path`, `method`. |
| `context` | object | Populated by middleware, available to handler functions. |

### ApiResponse

| Field | Type | Description |
|---|---|---|
| `status_code` | number | HTTP status code. |
| `body` | any | The response payload. |
| `headers` | string[] \| Record\<string, string\> | HTTP response headers as `"Header-Name: value"` strings or an object such as `{ "Content-Type": "application/json" }`. Optional. |

### Error Envelope

Errors the server generates itself (handler invocation failure, middleware failure or timeout, unmet route condition) use one stable JSON shape, so clients and AI agents can parse it without guessing:

```json
{ "error": { "code": "HANDLER_ERROR", "message": "human-readable detail", "error_id": "a1b2c3d4e5f6" } }
```

- `code` — machine-readable identifier. Engine-generated codes include `MIDDLEWARE_TIMEOUT`, `CONDITION_NOT_MET`, `INTERNAL_ERROR`; handler/condition failures surface the function's own error `code`.
- `message` — human-facing detail.
- `error_id` — present on 5xx responses; correlates the response with server logs. Omitted where there is no log correlation (e.g. timeouts).
- Unmet conditions return `422` with `"skipped": true` alongside the `error` object.

Bodies you return from your own handler or middleware pass through unchanged — the envelope only wraps errors the server raises.

## Middleware

The HTTP module supports middleware functions that run before the handler.

- **Per-route middleware** — attached to a specific trigger via `middleware_function_ids`
- **Global middleware** — configured in `iii-config.yaml`, runs on all HTTP routes

### Global Middleware Configuration

```yaml
- name: iii-http
  config:
    port: 3111
    middleware:
      - function_id: "global::rate-limiter"
        phase: preHandler
        priority: 5
      - function_id: "global::auth"
        phase: preHandler
        priority: 10
```

| Field | Type | Description |
|---|---|---|
| `function_id` | string | Required. Function ID of the middleware to invoke. |
| `phase` | string | Lifecycle phase. Only `preHandler` is supported. Defaults to `preHandler`. |
| `priority` | number | Execution order. Lower values run first. Defaults to `0`. |

### Middleware Function Contract

Middleware functions receive a request object with `path_params`, `query_params`, `headers`, `method` (no `body`). They must return one of:

- `{ action: "continue" }` — proceed to the next middleware or handler.
- `{ action: "respond", response: { status_code, body, headers } }` — short-circuit and return a response immediately.

### Execution Order

```
1. Route match
2. Global middleware (from config, sorted by priority)
3. Condition check (if configured)
4. Per-route middleware (from trigger config, in order)
5. Body parsing
6. Handler function
```

## Example Handler

```typescript
import { registerWorker } from 'iii-sdk'
import type { ApiRequest, ApiResponse } from 'iii-sdk'

const iii = registerWorker('ws://localhost:49134')

async function getUser(req: ApiRequest): Promise<ApiResponse> {
  const userId = req.path_params?.id
  const user = await database.findUser(userId)
  return {
    status_code: 200,
    body: { user },
    headers: { 'Content-Type': 'application/json' },
  }
}

const fn = iii.registerFunction('api::getUser', getUser)
iii.registerTrigger({
  type: 'http',
  function_id: fn.id,
  config: {
    api_path: '/users/:id',
    http_method: 'GET',
  },
})
```

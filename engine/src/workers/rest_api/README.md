# iii-http

The HTTP Worker exposes registered functions as HTTP endpoints.

## Sample Configuration

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

## Configuration

| Field | Type | Description |
|---|---|---|
| `port` | number | The port to listen on. Defaults to `3111`. |
| `host` | string | The host to listen on. Defaults to `0.0.0.0`. |
| `default_timeout` | number | Default timeout in milliseconds for request processing. Defaults to `30000`. |
| `concurrency_request_limit` | number | Maximum number of concurrent requests. Defaults to `1024`. |
| `cors.allowed_origins` | string[] | Allowed CORS origins. |
| `cors.allowed_methods` | string[] | Allowed CORS methods. |
| `body_limit` | number | Maximum request body size in bytes. Defaults to `1048576` (1 MB). |
| `trust_proxy` | boolean | Trust proxy headers such as `X-Forwarded-For`. Defaults to `false`. |
| `request_id_header` | string | Header name for request ID propagation. Defaults to `x-request-id`. |
| `ignore_trailing_slash` | boolean | Treat routes with/without trailing slash as equivalent. Defaults to `false`. |
| `not_found_function` | string | Function ID to invoke when no route matches. |

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

---
title: Handler & Context Migration Guide
description: Migrating HTTP step handlers to the new MotiaHttpArgs-based signature, and migrating from context-based state/enqueue/logger/streams to standalone imports.
---

This guide covers two major migration areas:

1. **HTTP handler signature changes** -- moving from `(req, ctx)` to `MotiaHttpArgs`-based `{ request, response }` destructuring, including SSE support.
2. **Context API changes** -- `state`, `enqueue`, `logger`, and `streams` have been removed from `FlowContext` and are now standalone imports.

<Callout type="info">
  For a complete migration from Motia v0.17.x to 1.0-RC, see the [full migration guide](/docs/getting-started/migration-guide).
</Callout>

---

## Context API Changes

### What Changed

`FlowContext` (the second argument to handlers, commonly called `ctx`) no longer contains `state`, `enqueue`, `logger`, or `streams`. These are now standalone imports from `'motia'` or from stream files.

| Aspect | Old | New |
|---|---|---|
| Logger | `ctx.logger.info(...)` | `import { logger } from 'motia'` then `logger.info(...)` |
| Enqueue | `ctx.enqueue({ topic, data })` | `import { enqueue } from 'motia'` then `enqueue({ topic, data })` |
| State | `ctx.state.set(group, key, value)` | `import { stateManager } from 'motia'` then `stateManager.set(group, key, value)` |
| Streams | `ctx.streams.name.get(groupId, id)` | `import { myStream } from './my.stream'` then `myStream.get(groupId, id)` |

### New FlowContext Shape

After migration, `FlowContext` only contains:

```typescript
interface FlowContext<TEnqueueData = never, TInput = unknown> {
  traceId: string
  trigger: TriggerInfo
  is: {
    queue: (input: TInput) => input is ExtractQueueInput<TInput>
    http: (input: TInput) => input is ExtractApiInput<TInput>
    cron: (input: TInput) => input is never
    state: (input: TInput) => input is ExtractStateInput<TInput>
    stream: (input: TInput) => input is ExtractStreamInput<TInput>
  }
  getData: () => ExtractDataPayload<TInput>
  match: <TResult>(handlers: MatchHandlers<TInput, TEnqueueData, TResult>) => Promise<TResult | undefined>
}
```

### Logger

<Tabs items={['Old', 'New']}>
<Tab value="Old">

```typescript
import { type Handlers, type StepConfig } from 'motia'

export const handler: Handlers<typeof config> = async (input, { logger }) => {
  logger.info('Processing', { input })
}
```

</Tab>
<Tab value="New">

```typescript
import { type Handlers, logger, type StepConfig } from 'motia'

export const handler: Handlers<typeof config> = async (input) => {
  logger.info('Processing', { input })
}
```

</Tab>
</Tabs>

### Enqueue

<Tabs items={['Old', 'New']}>
<Tab value="Old">

```typescript
import { type Handlers, type StepConfig } from 'motia'

export const handler: Handlers<typeof config> = async ({ request }, { enqueue }) => {
  await enqueue({ topic: 'process-order', data: request.body })
  return { status: 200, body: { ok: true } }
}
```

</Tab>
<Tab value="New">

```typescript
import { enqueue, type Handlers, type StepConfig } from 'motia'

export const handler: Handlers<typeof config> = async ({ request }) => {
  await enqueue({ topic: 'process-order', data: request.body })
  return { status: 200, body: { ok: true } }
}
```

</Tab>
</Tabs>

### State Manager

<Tabs items={['Old', 'New']}>
<Tab value="Old">

```typescript
import { type Handlers, type StepConfig } from 'motia'

export const handler: Handlers<typeof config> = async (input, { state, logger }) => {
  logger.info('Saving order')
  await state.set('orders', input.orderId, input)
  const orders = await state.list<Order>('orders')
}
```

</Tab>
<Tab value="New">

```typescript
import { type Handlers, logger, type StepConfig, stateManager } from 'motia'

export const handler: Handlers<typeof config> = async (input) => {
  logger.info('Saving order')
  await stateManager.set('orders', input.orderId, input)
  const orders = await stateManager.list<Order>('orders')
}
```

</Tab>
</Tabs>

### Streams

Streams are no longer accessed via `ctx.streams`. Instead, create a `Stream` instance in a `.stream.ts` file and import it into your steps.

<Tabs items={['Old', 'New']}>
<Tab value="Old">

```typescript
import { type Handlers, type StepConfig } from 'motia'

export const handler: Handlers<typeof config> = async (input, { streams, logger }) => {
  const todo = await streams.todo.get('inbox', todoId)
  await streams.todo.set('inbox', todoId, newTodo)
  await streams.todo.delete('inbox', todoId)
  await streams.todo.update('inbox', todoId, [
    { type: 'set', path: 'status', value: 'done' },
  ])
}
```

</Tab>
<Tab value="New">

First, define your stream in a `.stream.ts` file:

```typescript title="todo.stream.ts"
import { Stream, type StreamConfig } from 'motia'
import { z } from 'zod'

const todoSchema = z.object({
  id: z.string(),
  description: z.string(),
  createdAt: z.string(),
})

export const config: StreamConfig = {
  baseConfig: { storageType: 'default' },
  name: 'todo',
  schema: todoSchema,
}

export const todoStream = new Stream(config)
export type Todo = z.infer<typeof todoSchema>
```

Then import and use it in your step:

```typescript title="create-todo.step.ts"
import { type Handlers, logger, type StepConfig } from 'motia'
import { todoStream } from './todo.stream'

export const handler: Handlers<typeof config> = async ({ request }) => {
  // todoId and newTodo come from request.body in practice
  const { todoId, ...newTodo } = request.body
  const todo = await todoStream.get('inbox', todoId)
  await todoStream.set('inbox', todoId, newTodo)
  await todoStream.delete('inbox', todoId)
  await todoStream.update('inbox', todoId, [
    { type: 'set', path: 'status', value: 'done' },
  ])
}
```

</Tab>
</Tabs>

### Multi-Trigger Steps with Context

When using `ctx.match()`, `logger`, `enqueue`, and `stateManager` are imports -- `ctx` is only used for `match()`, `traceId`, and `trigger`:

<Tabs items={['Old', 'New']}>
<Tab value="Old">

```typescript
export const handler: Handlers<typeof config> = async (_, ctx) => {
  return ctx.match({
    http: async ({ request }) => {
      ctx.logger.info('Processing via API')
      await ctx.state.set('orders', orderId, request.body)
      await ctx.enqueue({ topic: 'order.processed', data: request.body })
      return { status: 200, body: { ok: true } }
    },
    queue: async (input) => {
      ctx.logger.info('Processing from queue')
      await ctx.state.set('orders', orderId, input)
    },
    cron: async () => {
      const orders = await ctx.state.list('pending-orders')
      ctx.logger.info('Batch processing', { count: orders.length })
    },
  })
}
```

</Tab>
<Tab value="New">

```typescript
import { enqueue, type Handlers, logger, type StepConfig, stateManager } from 'motia'

export const handler: Handlers<typeof config> = async (_, ctx) => {
  return ctx.match({
    http: async ({ request }) => {
      logger.info('Processing via API')
      await stateManager.set('orders', orderId, request.body)
      await enqueue({ topic: 'order.processed', data: request.body })
      return { status: 200, body: { ok: true } }
    },
    queue: async (input) => {
      logger.info('Processing from queue')
      await stateManager.set('orders', orderId, input)
    },
    cron: async () => {
      const orders = await stateManager.list('pending-orders')
      logger.info('Batch processing', { count: orders.length })
    },
  })
}
```

</Tab>
</Tabs>

---

## HTTP Handler Changes

### What Changed

HTTP step handlers now receive a `MotiaHttpArgs` object as their first argument instead of a bare request object. This object contains both `request` and `response`, enabling streaming patterns like SSE alongside standard request/response flows.

| Aspect | Old | New |
|---|---|---|
| First arg (TS/JS) | `req` (request object directly) | `{ request, response }` (`MotiaHttpArgs`) |
| First arg (Python) | `req` (dict-like object) | `request: ApiRequest` or `args: MotiaHttpArgs` |
| Body access (TS/JS) | `req.body` | `request.body` |
| Path params (TS/JS) | `req.pathParams` | `request.pathParams` |
| Headers (TS/JS) | `req.headers` | `request.headers` |
| Body access (Python) | `req.get("body", {})` | `request.body` |
| Path params (Python) | `req.get("pathParams", {}).get("id")` | `request.path_params.get("id")` |
| Return type (Python) | `{"status": 200, "body": {...}}` | `ApiResponse(status=200, body={...})` |
| Middleware placement | Config root: `middleware: [...]` | Inside trigger: `{ type: 'http', ..., middleware: [...] }` |
| Middleware first arg | `req` | `{ request, response }` |

---

## TypeScript / JavaScript

### Standard HTTP Handler

<Tabs items={['Old', 'New']}>
<Tab value="Old">

```typescript
import { type Handlers, type StepConfig } from 'motia'

export const config = {
  name: 'GetUser',
  triggers: [
    { type: 'http', path: '/users/:id', method: 'GET' },
  ],
  enqueues: [],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async (req, { logger }) => {
  const userId = req.pathParams.id
  logger.info('Getting user', { userId })
  return { status: 200, body: { id: userId } }
}
```

</Tab>
<Tab value="New">

```typescript
import { type Handlers, logger, type StepConfig } from 'motia'

export const config = {
  name: 'GetUser',
  triggers: [
    { type: 'http', path: '/users/:id', method: 'GET' },
  ],
  enqueues: [],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async ({ request }) => {
  const userId = request.pathParams.id
  logger.info('Getting user', { userId })
  return { status: 200, body: { id: userId } }
}
```

</Tab>
</Tabs>

**Key changes:**

1. Import `logger` from `'motia'` instead of destructuring from `ctx`
2. Destructure `{ request }` (or `{ request, response }` for SSE) from the first argument
3. Access `request.body`, `request.pathParams`, `request.queryParams`, `request.headers`
4. Return value stays the same: `{ status, body, headers? }`

### Types

```typescript
interface MotiaHttpArgs<TBody = unknown> {
  request: MotiaHttpRequest<TBody>
  response: MotiaHttpResponse
}

interface MotiaHttpRequest<TBody = unknown> {
  pathParams: Record<string, string>
  queryParams: Record<string, string | string[]>
  body: TBody
  headers: Record<string, string | string[]>
  method: string
  requestBody: ChannelReader
}

type MotiaHttpResponse = {
  status: (statusCode: number) => void
  headers: (headers: Record<string, string>) => void
  stream: NodeJS.WritableStream
  close: () => void
}
```

### Multi-Trigger Steps

When using `ctx.match()`, the HTTP branch handler also receives `MotiaHttpArgs`:

<Tabs items={['Old', 'New']}>
<Tab value="Old">

```typescript
return ctx.match({
  http: async (request) => {
    const { userId } = request.body
    return { status: 200, body: { ok: true } }
  },
})
```

</Tab>
<Tab value="New">

```typescript
return ctx.match({
  http: async ({ request }) => {
    const { userId } = request.body
    return { status: 200, body: { ok: true } }
  },
})
```

</Tab>
</Tabs>

---

## Python

### Standard HTTP Handler

<Tabs items={['Old', 'New']}>
<Tab value="Old">

```python
config = {
    "name": "GetUser",
    "triggers": [
        {"type": "http", "path": "/users/:id", "method": "GET"}
    ],
    "enqueues": [],
}

async def handler(req, ctx):
    user_id = req.get("pathParams", {}).get("id")
    ctx.logger.info("Getting user", {"userId": user_id})
    return {"status": 200, "body": {"id": user_id}}
```

</Tab>
<Tab value="New">

```python
from typing import Any
from motia import ApiRequest, ApiResponse, http, logger

config = {
    "name": "GetUser",
    "triggers": [
        http("GET", "/users/:id"),
    ],
    "enqueues": [],
}

async def handler(request: ApiRequest[Any]) -> ApiResponse[Any]:
    user_id = request.path_params.get("id")
    logger.info("Getting user", {"userId": user_id})
    return ApiResponse(status=200, body={"id": user_id})
```

</Tab>
</Tabs>

**Key changes:**

1. Import `ApiRequest`, `ApiResponse`, `logger` from `motia`
2. Use `http()` helper for trigger definitions
3. `logger`, `enqueue`, and `stateManager` are standalone imports -- not accessed via `ctx`
4. Access typed properties: `request.body`, `request.path_params`, `request.query_params`, `request.headers`
5. Return `ApiResponse(status=..., body=...)` instead of a plain dict

### Python Types

```python
class ApiRequest(BaseModel, Generic[TBody]):
    path_params: dict[str, str]
    query_params: dict[str, str | list[str]]
    body: TBody | None
    headers: dict[str, str | list[str]]

class ApiResponse(BaseModel, Generic[TOutput]):
    status: int
    body: Any
    headers: dict[str, str] = {}
```

---

## Middleware

### Placement Change

Middleware has moved from the config root **into the HTTP trigger object**.

<Tabs items={['Old', 'New']}>
<Tab value="Old">

```typescript
export const config = {
  name: 'ProtectedEndpoint',
  triggers: [
    { type: 'http', path: '/protected', method: 'GET' },
  ],
  middleware: [authMiddleware],
  enqueues: [],
} as const satisfies StepConfig
```

</Tab>
<Tab value="New">

```typescript
export const config = {
  name: 'ProtectedEndpoint',
  triggers: [
    { type: 'http', path: '/protected', method: 'GET', middleware: [authMiddleware] },
  ],
  enqueues: [],
} as const satisfies StepConfig
```

</Tab>
</Tabs>

### Middleware Signature Change

<Tabs items={['Old', 'New']}>
<Tab value="Old">

```typescript
const authMiddleware: ApiMiddleware = async (req, ctx, next) => {
  if (!req.headers.authorization) {
    return { status: 401, body: { error: 'Unauthorized' } }
  }
  return next()
}
```

</Tab>
<Tab value="New">

```typescript
const authMiddleware: ApiMiddleware = async ({ request }, ctx, next) => {
  if (!request.headers.authorization) {
    return { status: 401, body: { error: 'Unauthorized' } }
  }
  return next()
}
```

</Tab>
</Tabs>

---

## Server-Sent Events (SSE)

SSE is enabled by the `response` object in `MotiaHttpArgs`. Instead of returning a response, you write directly to the stream.

<Tabs items={['TypeScript', 'Python']}>
<Tab value='TypeScript'>

```typescript title="src/sse-example.step.ts"
import { type Handlers, http, logger, type StepConfig } from 'motia'

export const config = {
  name: 'SSE Example',
  description: 'Streams data back to the client as SSE',
  flows: ['sse-example'],
  triggers: [http('POST', '/sse')],
  enqueues: [],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async ({ request, response }) => {
  logger.info('SSE request received')

  response.status(200)
  response.headers({
    'content-type': 'text/event-stream',
    'cache-control': 'no-cache',
    connection: 'keep-alive',
  })

  const chunks: string[] = []
  for await (const chunk of request.requestBody.stream) {
    chunks.push(Buffer.from(chunk).toString('utf-8'))
  }

  const items = ['alpha', 'bravo', 'charlie']
  for (const item of items) {
    response.stream.write(`event: item\ndata: ${JSON.stringify({ item })}\n\n`)
    await new Promise((resolve) => setTimeout(resolve, 500))
  }

  response.stream.write(`event: done\ndata: ${JSON.stringify({ total: items.length })}\n\n`)
  response.close()
}
```

</Tab>
<Tab value='Python'>

```python title="src/sse_example_step.py"
import asyncio
import json
from typing import Any

from motia import MotiaHttpArgs, http, logger

config = {
    "name": "SSE Example",
    "description": "Streams data back to the client as SSE",
    "flows": ["sse-example"],
    "triggers": [
        http("POST", "/sse"),
    ],
    "enqueues": [],
}

async def handler(args: MotiaHttpArgs[Any]) -> None:
    request = args.request
    response = args.response

    logger.info("SSE request received")

    await response.status(200)
    await response.headers({
        "content-type": "text/event-stream",
        "cache-control": "no-cache",
        "connection": "keep-alive",
    })

    raw_chunks: list[str] = []
    async for chunk in request.request_body.stream:
        if isinstance(chunk, bytes):
            raw_chunks.append(chunk.decode("utf-8", errors="replace"))
        else:
            raw_chunks.append(str(chunk))

    items = ["alpha", "bravo", "charlie"]
    for item in items:
        response.writer.stream.write(
            f"event: item\ndata: {json.dumps({'item': item})}\n\n".encode("utf-8")
        )
        await asyncio.sleep(0.5)

    response.writer.stream.write(
        f"event: done\ndata: {json.dumps({'total': len(items)})}\n\n".encode("utf-8")
    )
    response.close()
```

</Tab>
</Tabs>

**SSE key points:**

- Destructure **both** `request` and `response` from the first argument
- Use `response.status()` and `response.headers()` to configure the response
- Write SSE-formatted data to `response.stream` (TS/JS) or `response.writer.stream` (Python)
- Call `response.close()` when done streaming
- Do **not** return a response object

---

## Migration Checklist

### Context API

- Replace `ctx.logger` / `context.logger` with `import { logger } from 'motia'`
- Replace `ctx.enqueue` / `context.enqueue` with `import { enqueue } from 'motia'`
- Replace `ctx.state` / `context.state` with `import { stateManager } from 'motia'`
- Replace `ctx.streams.name` / `context.streams.name` with `import { myStream } from './my.stream'`
- Create `.stream.ts` files with `new Stream(config)` for each stream used
- Remove `state`, `enqueue`, `logger`, `streams` from handler destructuring of `ctx`
- Update handler signatures: if `ctx` is only used for destructuring those removed properties, the second argument can be omitted entirely

### TypeScript / JavaScript (HTTP)

- Change handler first argument from `(req, ctx)` to `({ request }, ctx)` for all HTTP steps
- Replace `req.body` with `request.body`
- Replace `req.pathParams` with `request.pathParams`
- Replace `req.queryParams` with `request.queryParams`
- Replace `req.headers` with `request.headers`
- Move `middleware` arrays from config root into HTTP trigger objects
- Update middleware functions: change `(req, ctx, next)` to `({ request }, ctx, next)`
- Update `ctx.match()` HTTP handlers: change `(request) =>` to `({ request }) =>`

### Python

- Add imports: `from motia import ApiRequest, ApiResponse, http` (and `FlowContext` only if your handler needs `ctx`)
- Use `http()` helper in trigger definitions
- Change handler signature to `handler(request: ApiRequest[Any]) -> ApiResponse[Any]` (or include `ctx: FlowContext[Any]` when needed)
- Replace `req.get("body", {})` with `request.body`
- Replace `req.get("pathParams", {}).get("id")` with `request.path_params.get("id")`
- Replace `req.get("queryParams", {})` with `request.query_params`
- Replace `req.get("headers", {})` with `request.headers`
- Return `ApiResponse(status=..., body=...)` instead of plain dicts
- For SSE: use `MotiaHttpArgs` instead of `ApiRequest`

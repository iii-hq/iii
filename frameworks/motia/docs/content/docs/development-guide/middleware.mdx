---
title: Middleware
description: Run code before and after your HTTP handlers
---

## What is Middleware?

Middleware runs before your HTTP handler. Use it for authentication, logging, error handling, or any logic that applies to multiple endpoints.

---

## How It Works

A middleware is a function that receives three arguments:

```typescript
middleware({ request, response }, ctx, next)
```

- **`{ request, response }`** - The `MotiaHttpArgs` object (same as handler)
- **ctx** - The context object (same as handler)
- **next()** - Call this to continue to the handler

If you don't call `next()`, the request stops. The handler never runs.

---

## Simple Example

<Tabs items={['TypeScript', 'JavaScript', 'Python']}>
  <Tab value="TypeScript">
    ```typescript
    import { ApiMiddleware, type Handlers, type StepConfig } from 'motia'

    const authMiddleware: ApiMiddleware = async ({ request }, ctx, next) => {
      if (!request.headers.authorization) {
        return { status: 401, body: { error: 'Unauthorized' } }
      }
      return next()
    }

    export const config = {
      name: 'ProtectedEndpoint',
      description: 'A protected API endpoint',
      triggers: [
        { type: 'http', path: '/protected', method: 'GET', middleware: [authMiddleware] },
      ],
      enqueues: [],
    } as const satisfies StepConfig

    export const handler: Handlers<typeof config> = async ({ request }, ctx) => {
      return { status: 200, body: { message: 'Success' } }
    }
    ```
  </Tab>
  <Tab value="JavaScript">
    ```javascript
    const authMiddleware = async ({ request }, ctx, next) => {
      if (!request.headers.authorization) {
        return { status: 401, body: { error: 'Unauthorized' } }
      }
      return next()
    }

    export const config = {
      name: 'ProtectedEndpoint',
      description: 'A protected API endpoint',
      triggers: [
        { type: 'http', path: '/protected', method: 'GET', middleware: [authMiddleware] },
      ],
      enqueues: [],
    }

    export const handler = async ({ request }, ctx) => {
      return { status: 200, body: { message: 'Success' } }
    }
    ```
  </Tab>
  <Tab value="Python">
    ```python
    from typing import Any
    from motia import ApiRequest, ApiResponse, FlowContext, http

    async def auth_middleware(request, context, next_fn):
        if not request.headers.get("authorization"):
            return {"status": 401, "body": {"error": "Unauthorized"}}
        return await next_fn()

    config = {
        "name": "ProtectedEndpoint",
        "description": "A protected API endpoint",
        "triggers": [
            http("GET", "/protected", middleware=[auth_middleware]),
        ],
        "enqueues": [],
    }

    async def handler(request: ApiRequest[Any], ctx: FlowContext[Any]) -> ApiResponse[Any]:
        return ApiResponse(status=200, body={"message": "Success"})
    ```
  </Tab>
</Tabs>

---

## Execution Order

Middleware runs in the order you list them:

```typescript
export const config = {
  name: 'MyEndpoint',
  description: 'Endpoint with multiple middleware',
  triggers: [
    {
      type: 'http',
      path: '/endpoint',
      method: 'POST',
      middleware: [loggingMiddleware, authMiddleware, errorMiddleware],
    },
  ],
  enqueues: [],
} as const satisfies StepConfig
```

---

## Modifying Responses

Await `next()` to get the response, then modify it:

<Tabs items={['TypeScript', 'JavaScript', 'Python']}>
  <Tab value="TypeScript">
    ```typescript
    const addHeadersMiddleware = async ({ request }, ctx, next) => {
      const response = await next()

      return {
        ...response,
        headers: {
          ...response.headers,
          'X-Request-Id': ctx.traceId
        }
      }
    }
    ```
  </Tab>
  <Tab value="JavaScript">
    ```javascript
    const addHeadersMiddleware = async ({ request }, ctx, next) => {
      const response = await next()

      return {
        ...response,
        headers: {
          ...response.headers,
          'X-Request-Id': ctx.traceId
        }
      }
    }
    ```
  </Tab>
  <Tab value="Python">
    ```python
    async def add_headers_middleware(request, context, next_fn):
        response = await next_fn()
        if response is None:
            return None

        headers = dict(response.headers) if response.headers else {}
        headers["X-Request-Id"] = context.trace_id

        return ApiResponse(status=response.status, body=response.body, headers=headers)
    ```
  </Tab>
</Tabs>

---

## Passing Data to Handlers

Middleware can attach data to the `req` object, making it available to your handler. This is perfect for authentication - verify the user once in middleware, then use their details in the handler.

<Tabs items={['TypeScript', 'JavaScript', 'Python']}>
  <Tab value="TypeScript">
    First, extend the request type in `api.d.ts`:

    ```typescript
    import 'motia'

    declare module 'motia' {
      interface ApiRequest {
        user?: { id: string; role: string }
      }
    }
    ```

    Then attach the data in your middleware:

    ```typescript
    const authMiddleware: ApiMiddleware = async ({ request }, ctx, next) => {
      request.user = { id: '123', role: 'admin' }
      return next()
    }
    ```

    Now use it in your handler:

    ```typescript
    export const handler: Handlers<typeof config> = async ({ request }, ctx) => {
      if (request.user?.role === 'admin') {
        return { status: 200, body: { message: 'Welcome Admin' } }
      }
      return { status: 403, body: { error: 'Forbidden' } }
    }
    ```
  </Tab>
  <Tab value="JavaScript">
    Attach data to `request` in middleware:

    ```javascript
    const authMiddleware = async ({ request }, ctx, next) => {
      request.user = { id: '123', role: 'admin' }
      return next()
    }
    ```

    Access it in your handler:

    ```javascript
    export const handler = async ({ request }, ctx) => {
      const { user } = request
      return { status: 200, body: { user } }
    }
    ```
  </Tab>
  <Tab value="Python">
    Store auth data on the context object:

    ```python
    async def auth_middleware(request, context, next_fn):
        context.user = {"id": "123", "role": "admin"}
        return await next_fn()

    async def handler(request, context):
        user = getattr(context, "user", None)
        return ApiResponse(status=200, body={"user": user})
    ```
  </Tab>
</Tabs>

> **Learn more:** Check out the [Middleware Auth Handler Example](https://github.com/MotiaDev/motia-examples/tree/main/examples/middleware-auth-handler-example) to see a complete project with JWT validation and type safety.

---

## Error Handling

Catch errors from handlers:

<Tabs items={['TypeScript', 'JavaScript', 'Python']}>
  <Tab value="TypeScript">
    ```typescript
    import { ZodError } from 'zod'
    import { logger } from 'motia'

    const errorMiddleware = async ({ request }, _, next) => {
      try {
        return await next()
      } catch (error: any) {
        if (error instanceof ZodError) {
          logger.error('Validation error', { errors: error.errors })
          return { status: 400, body: { error: 'Validation failed' } }
        }

        logger.error('Unexpected error', { error: error.message })
        return { status: 500, body: { error: 'Internal server error' } }
      }
    }
    ```
  </Tab>
  <Tab value="JavaScript">
    ```javascript
    import { ZodError } from 'zod'
    import { logger } from 'motia'

    const errorMiddleware = async ({ request }, _, next) => {
      try {
        return await next()
      } catch (error) {
        if (error instanceof ZodError) {
          logger.error('Validation error', { errors: error.errors })
          return { status: 400, body: { error: 'Validation failed' } }
        }

        logger.error('Unexpected error', { error: error.message })
        return { status: 500, body: { error: 'Internal server error' } }
      }
    }
    ```
  </Tab>
  <Tab value="Python">
    ```python
    from pydantic import ValidationError
    from motia import logger

    async def error_middleware(request, _, next_fn):
        try:
            return await next_fn()
        except ValidationError as e:
            logger.error("Validation error", {"errors": str(e)})
            return {"status": 400, "body": {"error": "Validation failed"}}
        except Exception as e:
            logger.error("Unexpected error", {"error": str(e)})
            return {"status": 500, "body": {"error": "Internal server error"}}
    ```
  </Tab>
</Tabs>

---

## Reusing Middleware

Create middleware files in a shared location:

```typescript title="middlewares/core.middleware.ts"
import { logger } from 'motia'

export const coreMiddleware = async ({ request }, _, next) => {
  try {
    return await next()
  } catch (error) {
    logger.error('Error', { error })
    return { status: 500, body: { error: 'Internal server error' } }
  }
}
```

Import and use across steps:

```typescript title="src/user.step.ts"
import { coreMiddleware } from '../middlewares/core.middleware'
import { type Handlers, type StepConfig } from 'motia'

export const config = {
  name: 'GetUser',
  description: 'Get user by ID',
  triggers: [
    { type: 'http', path: '/users/:id', method: 'GET', middleware: [coreMiddleware] },
  ],
  enqueues: [],
} as const satisfies StepConfig
```

---

## What's Next?

<Cards>
  <Card href="/docs/concepts/steps#triggers" title="Triggers">
    Learn more about Triggers
  </Card>

  <Card href="https://github.com/MotiaDev/motia-examples" title="Examples">
    Explore real-world examples and patterns
  </Card>
</Cards>

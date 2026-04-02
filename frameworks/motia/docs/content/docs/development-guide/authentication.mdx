---
title: Authentication
description: How to handle authentication in Motia for HTTP endpoints and real-time streams
---

Motia provides two authentication patterns: **stream authentication** configured via the iii engine's `config.yaml`, and **HTTP endpoint authentication** handled within your Step handlers.

## Stream Authentication

Stream authentication operates at two levels: **connection-level** authentication when the WebSocket connects, and **subscription-level** authorization when a client joins a specific stream.

### Connection-Level Authentication

Configure the `auth_function` on the Stream module in `config.yaml`. The value uses `<module>.<function>` format — `stream.authenticate` means the iii engine looks for a registered Step whose exported handler is named `authenticate` under the `stream` module namespace. This function runs on every new WebSocket connection before the upgrade completes.

```yaml title="config.yaml"
  - class: modules::stream::StreamModule
    config:
      port: 3112
      host: 0.0.0.0
      auth_function: stream.authenticate
      adapter:
        class: modules::stream::adapters::KvStore
        config:
          store_method: file_based
          file_path: ./data/stream_store
```

Create a Step file that exports the handler referenced by `auth_function`. The handler receives a stream connection object (not an Express-like `req`) with the WebSocket handshake's `headers`, `path`, `query_params`, and `addr`. It should return a `context` object that gets attached to the connection and passed to all subsequent stream join/leave trigger handlers:

```typescript title="src/stream-auth.step.ts"
import type { StepConfig } from 'motia'

export const config = {
  name: 'StreamAuthenticate',
  description: 'Authenticates WebSocket connections',
  triggers: [],
  enqueues: [],
  flows: ['auth'],
} as const satisfies StepConfig

export async function authenticate(input: {
  headers: Record<string, string>
  path: string
  query_params: Record<string, string>
  addr: string
}) {
  const token = input.headers?.['authorization']

  if (!token) {
    throw new Error('No authorization token')
  }

  const user = await validateToken(token)
  return { context: { userId: user.id, role: user.role } }
}
```

If the function throws or returns no result, the connection proceeds without an auth context. The returned `context` is attached to the connection and passed to stream join/leave trigger handlers.

### Subscription-Level Authorization

When a client joins a specific stream, a `stream` trigger handler can reject the subscription by returning `{ unauthorized: true }`. This allows per-stream, per-group access control using the auth context from the connection:

```typescript title="src/stream-guard.step.ts"
import { logger, type Handlers, type StepConfig } from 'motia'

export const config = {
  name: 'StreamGuard',
  description: 'Controls access to specific streams',
  triggers: [
    {
      type: 'stream',
      streamName: 'private-data',
      condition: (input) => true,
    },
  ],
  enqueues: [],
  flows: ['auth'],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async (input) => {
  // input includes context from connection-level auth
  if (!input.context?.userId) {
    return { unauthorized: true }
  }

  logger.info('Stream access granted', { userId: input.context.userId })
  return { unauthorized: false }
}
```

---

## HTTP Endpoint Authentication

For HTTP endpoints, handle authentication directly in your Step handler using shared utility functions:

```typescript title="src/utils/auth.ts"
import type { ApiRequest } from 'motia'
import jwt from 'jsonwebtoken'

type TokenData = { userId: string; role: string }

export async function requireAuth(request: ApiRequest<any>): Promise<TokenData> {
  const authHeaderEntry = Object.entries(request.headers).find(
    ([key]) => key.toLowerCase() === 'authorization'
  )
  const authHeader = authHeaderEntry?.[1]

  if (typeof authHeader !== 'string') {
    throw new HttpError(401, 'Missing authorization header')
  }

  const [scheme, token] = authHeader.split(' ')
  if (!token || scheme.toLowerCase() !== 'bearer') {
    throw new HttpError(401, 'Invalid authorization header')
  }

  return jwt.verify(token, process.env.JWT_SECRET!) as TokenData
}

export class HttpError extends Error {
  constructor(public status: number, message: string) {
    super(message)
  }
}
```

Then use it in your Step handlers:

```typescript title="src/get-profile.step.ts"
import { logger, stateManager, type Handlers, type StepConfig } from 'motia'
import { requireAuth, HttpError } from './utils/auth'

export const config = {
  name: 'GetProfile',
  description: 'Get authenticated user profile',
  triggers: [{ type: 'http', method: 'GET', path: '/profile' }],
  enqueues: [],
  flows: ['users'],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async ({ request }) => {
  try {
    const tokenData = await requireAuth(request)
    const user = await stateManager.get('users', tokenData.userId)

    return { status: 200, body: user }
  } catch (error) {
    if (error instanceof HttpError) {
      return { status: error.status, body: { error: error.message } }
    }
    return { status: 500, body: { error: 'Internal server error' } }
  }
}
```

---

## Error Handling

Wrap your handler logic in try/catch blocks for error handling. This replaces the previous middleware-based `coreMiddleware` pattern:

```typescript
import { logger } from 'motia'

export const handler: Handlers<typeof config> = async ({ request }) => {
  try {
    const result = await processRequest(request)
    return { status: 200, body: result }
  } catch (error) {
    logger.error('Request failed', { error: error.message })

    if (error instanceof HttpError) {
      return { status: error.status, body: { error: error.message } }
    }

    return { status: 500, body: { error: 'Internal server error' } }
  }
}
```

---

## Reusable Auth Wrappers

For a cleaner pattern across many Steps, create a wrapper function:

```typescript title="src/utils/with-auth.ts"
import type { ApiRequest, ApiResponse } from 'motia'
import { requireAuth, HttpError, type TokenData } from './auth'

export function withAuth<TBody>(
  fn: (request: ApiRequest<TBody>, tokenData: TokenData) => Promise<ApiResponse<any, any>>
) {
  return async ({ request }: { request: ApiRequest<TBody> }) => {
    try {
      const tokenData = await requireAuth(request)
      return await fn(request, tokenData)
    } catch (error) {
      if (error instanceof HttpError) {
        return { status: error.status, body: { error: error.message } }
      }
      return { status: 500, body: { error: 'Internal server error' } }
    }
  }
}
```

```typescript title="src/protected-endpoint.step.ts"
import { withAuth } from './utils/with-auth'
import { logger } from 'motia'

export const handler = withAuth(async (request, tokenData) => {
  logger.info('Authenticated request', { userId: tokenData.userId })
  return { status: 200, body: { message: 'Protected content' } }
})
```

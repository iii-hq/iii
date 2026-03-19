---
title: React Stream Client
description: Use Motia streams in React with MotiaStreamProvider, useMotiaStream, useStreamGroup, and useStreamItem.
---

Use `@motiadev/stream-client-react` to subscribe to Motia streams from React.

<Callout type="info" title="Package name">
  The React stream client is published as <code>@motiadev/stream-client-react</code>. It is not imported from <code>motia</code>.
</Callout>

## Install

```bash
npm install @motiadev/stream-client-react
```

## MotiaStreamProvider

Wrap your app (or a subtree) so hooks can access the stream connection.

```tsx title="App.tsx"
import { useMemo } from 'react'
import { MotiaStreamProvider } from '@motiadev/stream-client-react'

export function AppShell({ token }: { token?: string }) {
  const protocols = useMemo(() => (token ? ['Authorization', token] : undefined), [token])

  return (
    <MotiaStreamProvider address="ws://localhost:3112" protocols={protocols}>
      <App />
    </MotiaStreamProvider>
  )
}
```

### Props

- `address: string` - Stream server WebSocket URL
- `protocols?: string | string[]` - Optional WebSocket protocols (for auth tokens, etc.)

## useMotiaStream

Access the underlying stream client instance from context.

```tsx
import { useMotiaStream } from '@motiadev/stream-client-react'

function DebugConnection() {
  const { stream } = useMotiaStream()
  return <pre>{stream ? 'connected' : 'connecting...'}</pre>
}
```

<Callout type="warn">
  `useMotiaStream`, `useStreamGroup`, and `useStreamItem` must be used inside `MotiaStreamProvider`.
</Callout>

## useStreamGroup

Subscribe to all items in a group.

```tsx
import { useStreamGroup } from '@motiadev/stream-client-react'

type Todo = { id: string; description: string }

function InboxTodos() {
  const { data: todos } = useStreamGroup<Todo>({
    streamName: 'todo',
    groupId: 'inbox',
  })

  return (
    <ul>
      {todos.map((todo) => (
        <li key={todo.id}>{todo.description}</li>
      ))}
    </ul>
  )
}
```

### Args

- `streamName: string` - Stream name from your `.stream.ts` config
- `groupId: string` - Group to subscribe to
- `sortKey?: keyof TData` - Optional sort field
- `setData?: (data: TData[]) => void` - Optional callback on updates

## useStreamItem

Subscribe to one specific stream item.

```tsx
import { useStreamItem } from '@motiadev/stream-client-react'

type Todo = { id: string; description: string; completedAt?: string }

function TodoDetails({ id }: { id: string }) {
  const { data: todo } = useStreamItem<Todo>({
    streamName: 'todo',
    groupId: 'inbox',
    id,
  })

  if (!todo) return <p>Loading...</p>
  return <p>{todo.description}</p>
}
```

### Args

- `streamName: string`
- `groupId: string`
- `id?: string`

## Return values

- `useStreamGroup<T>()` returns `{ data: T[], event }`
- `useStreamItem<T>()` returns `{ data: T | null | undefined, event }`
- `event` is the subscription object used by `useStreamEventHandler`

## Related

- [Real-time Streams](/docs/development-guide/streams)
- [API Reference](/docs/api-reference)

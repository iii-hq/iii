# @motiadev/stream-client-react

Motia Stream Client React Package – Responsible for managing streams of data in React applications.

For more information about Motia Streams, please refer to the [Motia Streams documentation](https://motia.dev/docs).

## Overview

`@motiadev/stream-client-react` provides a set of React utilities and hooks for integrating Motia's real-time streaming capabilities into your React applications. It enables you to subscribe to individual stream items, groups, and handle real-time events with idiomatic React patterns.

---

## Installation

```bash
npm install @motiadev/stream-client-react
```

## Exports

- **Stream, StreamItemSubscription, StreamGroupSubscription**  
  (Re-exported from `@motiadev/stream-client-browser`)
- **MotiaStreamProvider**  
  React provider for initializing and supplying the stream context.
- **useMotiaStream**  
  Hook to access the current stream instance from context.
- **useStreamItem**  
  Hook to subscribe to a single stream item.
- **useStreamGroup**  
  Hook to subscribe to a group of stream items.
- **useStreamEventHandler**  
  Hook to attach event listeners to stream subscriptions.

## Usage

### 1. MotiaStreamProvider

Wrap your application (or a subtree) with `MotiaStreamProvider` to initialize the stream and provide it via context.

```tsx
import { MotiaStreamProvider } from '@motiadev/stream-client-react'

const App = () => {
  const token = useMemo(() => window.sessionStorage.getItem('motia.streamToken') ?? undefined, [])
  const protocols = useMemo(() => (token ? ['Authorization', token] : undefined), [token])

  return (
    <MotiaStreamProvider address="wss://your-stream-server" protocols={protocols}>
      <App />
    </MotiaStreamProvider>
  )
}
```

**Props:**

- `address` (string): The WebSocket address of your Motia stream server.
- `protocols` (string | string[] | undefined): Optional WebSocket subprotocols (use `['Authorization', token]` to send tokens via `Sec-WebSocket-Protocol`).

---

### 2. useMotiaStream

Access the current stream instance anywhere within the provider.

```tsx
import { useMotiaStream } from '@motiadev/stream-client-react'

const { stream } = useMotiaStream()
```

---

### 3. useStreamItem

Subscribe to a single item in a stream and receive real-time updates.

```tsx
import { useStreamItem } from '@motiadev/stream-client-react'

const { data, event } = useStreamItem<{ name: string }>({
  /**
   * The stream name from motia Server
   */
  streamName: 'users',
  /**
   * The id of the item to subscribe to
   */
  id: 'user-123',
})
```

- `data`: The current value of the item (typed).
- `event`: The subscription object to subscribe to custom events. Check `useStreamEventHandler` for more information.

---

### 4. useStreamGroup

Subscribe to a group of items in a stream.

```tsx
import { useStreamGroup } from '@motiadev/stream-client-react'

const { data, event } = useStreamGroup<{ name: string }>({
  /**
   * The stream name from motia Server
   */
  streamName: 'users',
  /**
   * The group id to subscribe to
   */
  groupId: 'admins',
})
```

- `data`: Array of current group items.
- `event`: The group subscription object.

---

### 5. useStreamEventHandler

Attach custom event listeners to a stream subscription.

```tsx
import { useStreamEventHandler } from '@motiadev/stream-client-react'

useStreamEventHandler(
  {
    event, // from useStreamItem or useStreamGroup
    type: 'custom-event',
    listener: (eventData) => {
      // handle event
    },
  },
  [
    /* dependencies */
  ],
)
```

---

## API Reference

### MotiaStreamProvider

- **Props:**
  - `address: string` – WebSocket address for the stream server.
  - `children: React.ReactNode`

### useMotiaStream

- Returns `{ stream }` from context.

### useStreamItem<TData>

- **Args:** `{ streamName: string, id: string }`
- **Returns:** `{ data: TData | null, event: StreamSubscription | null }`

### useStreamGroup<TData>

- **Args:** `{ streamName: string, groupId: string }`
- **Returns:** `{ data: TData[], event: StreamSubscription | null }`

### useStreamEventHandler

- **Args:**
  - `{ event, type, listener }`
  - `dependencies` (array): React dependency list

---

## Example

```tsx
import { MotiaStreamProvider, useStreamItem, useStreamEventHandler } from '@motiadev/stream-client-react'

function UserComponent({ userId }) {
  const { data, event } = useStreamItem<{ name: string }>({
    streamName: 'users',
    id: userId,
  })

  useStreamEventHandler(
    {
      event,
      type: 'user-updated',
      listener: (e) => alert('User updated!'),
    },
    [event],
  )

  if (!data) return <div>Loading...</div>
  return <div>{data.name}</div>
}

export default function App() {
  const token = window.sessionStorage.getItem('motia.streamToken') ?? undefined
  const protocols = token ? ['Authorization', token] : undefined

  return (
    <MotiaStreamProvider address="wss://your-stream-server" protocols={protocols}>
      <UserComponent userId="user-123" />
    </MotiaStreamProvider>
  )
}
```

---

## Notes

- All hooks must be used within a `MotiaStreamProvider`.
- The library is designed to work seamlessly with Motia's event-driven architecture.
- For advanced stream management, refer to the [@motiadev/stream-client-browser](https://www.npmjs.com/package/@motiadev/stream-client-browser) documentation.

## License

[Apache License 2.0](https://github.com/MotiaDev/motia/blob/main/LICENSE)

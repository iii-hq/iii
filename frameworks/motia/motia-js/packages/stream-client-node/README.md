# @motiadev/stream-client-node

Motia Stream Client Package – Responsible for managing real-time [Motia](https://motia.dev) streams of data in node environments. This package provides a simple, type-safe interface for subscribing to item and group streams over WebSockets, handling live updates, and managing stream state in your web applications.

## Features

- **WebSocket-based streaming** for real-time data updates
- **Item and group subscriptions** with automatic state management
- **Change listeners** for reactive UI updates
- **Custom event handling** for extensibility
- **TypeScript support** for strong typing and autocompletion

---

## Installation

```bash
npm install @motiadev/stream-client-node
```

---

## Usage

### 1. Creating a Stream Connection

```typescript
import { Stream } from '@motiadev/stream-client-node'

const token = process.env.MOTIA_STREAM_TOKEN
const protocols = token ? ['Authorization', token] : undefined

const stream = new Stream('wss://your-stream-server', { protocols })
```

### 2. Subscribing to an Item Stream

```typescript
const itemSubscription = stream.subscribeItem<{ id: string; name: string }>('users', 'user-123')

itemSubscription.addChangeListener((user) => {
  console.log('User updated:', user)
})

// Listen for custom events
itemSubscription.onEvent('custom-event', (data) => {
  console.log('Received custom event:', data)
})
```

### 3. Subscribing to a Group Stream

```typescript
const groupSubscription = stream.subscribeGroup<{ id: string; name: string }>('users', 'group-abc')

groupSubscription.addChangeListener((users) => {
  console.log('Group users updated:', users)
})
```

### 4. Cleaning Up

```typescript
itemSubscription.close()
groupSubscription.close()
stream.close()
```

---

## API Reference

### `Stream`

- **constructor(address: string, options?: { protocols?: string | string[] })**

  - Establishes a WebSocket connection to the given address.
  - Passes any `protocols` directly to the underlying `WebSocket` constructor (useful for sending `Sec-WebSocket-Protocol: Authorization,<token>`).

- **subscribeItem<T>(streamName: string, id: string): StreamItemSubscription<T>**

  - Subscribes to a single item in a stream.
  - Returns a `StreamItemSubscription` instance.

- **subscribeGroup<T>(streamName: string, groupId: string): StreamGroupSubscription<T>**

  - Subscribes to a group of items in a stream.
  - Returns a `StreamGroupSubscription` instance.

- **close()**
  - Closes the WebSocket connection.

---

### `StreamItemSubscription<T>`

- **addChangeListener(listener: (item: T | null) => void)**

  - Registers a callback for when the item changes.

- **removeChangeListener(listener)**

  - Unregisters a change listener.

- **onEvent(type: string, listener: (event: any) => void)**

  - Registers a custom event listener.

- **offEvent(type: string, listener)**

  - Unregisters a custom event listener.

- **getState(): T | null**

  - Returns the current state of the item.

- **close()**
  - Unsubscribes from the item stream and cleans up listeners.

---

### `StreamGroupSubscription<T>`

- **addChangeListener(listener: (items: T[]) => void)**

  - Registers a callback for when the group changes.

- **removeChangeListener(listener)**

  - Unregisters a change listener.

- **onEvent(type: string, listener: (event: any) => void)**

  - Registers a custom event listener.

- **offEvent(type: string, listener: (event: any) => void)**

  - Unregisters a custom event listener.

- **getState(): T[]**

  - Returns the current state of the group.

- **close()**
  - Unsubscribes from the group stream and cleans up listeners.

---

### `StreamSubscription` (Base Class)

- **onEvent(type: string, listener: (event: any) => void)**
- **offEvent(type: string, listener: (event: any) => void)**

---

## Types

All types are exported from `stream.types.ts` for advanced usage and type safety.

---

## Example

```typescript
import { Stream } from '@motiadev/stream-client-node'

const token = process.env.MOTIA_STREAM_TOKEN
const protocols = token ? ['Authorization', token] : undefined

const stream = new Stream('wss://example.com', { protocols })
const userSub = stream.subscribeItem<{ id: string; name: string }>('users', 'user-1')

userSub.addChangeListener((user) => {
  // React to user changes
})

const groupSub = stream.subscribeGroup<{ id: string; name: string }>('users', 'group-1')

groupSub.addChangeListener((users) => {
  // React to group changes
})
```

---

## License

[Apache License 2.0](https://github.com/MotiaDev/motia/blob/main/LICENSE)

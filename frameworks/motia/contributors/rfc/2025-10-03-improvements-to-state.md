## Improvements to State Management

The current way of working with State and Streams is fine, but we need to add some more features to it.

1. Allow developers to increment/decrement values without fetching first.
2. Allow developers to make multiple updates to State/Streams with a single Transaction and be able to rollback the changes if one of the updates fails.

```typescript
type Options = {
  /**
   * The time to live in seconds
   */
  ttl?: number
}

type StateManager = {
  // Existing methods will continue to work as they do now
  get<T>(groupId: string, key: string): Promise<T | null>
  delete<T>(groupId: string, key: string): Promise<T | null>
  getGroup<T>(groupId: string): Promise<T[]>
  clear(groupId: string): Promise<void>

  set<T>(groupId: string, key: string, value: T, options?: Options): Promise<T>

  /**
   * Updates a single item in the state
   *
   * @param groupId - The group id of the state
   * @param key - The key of the item to update
   * @param fields - The fields to update, only the fields that are provided will be updated
   * @returns The item
   */
  updateFields<T>(groupId: string, key: string, fields: Partial<T>): Promise<T>

  // New methods will be added

  /**
   * Increments a single property in the state
   *
   * @param groupId - The group id of the state
   * @param key - The key of the item to increment
   * @param property - The property to increment
   * @param value - The value to increment
   * @returns The item
   */
  incrementNumber<T>(groupId: string, key: string, property: string, value: number): Promise<T>

  /**
   * Decrements a single property in the state
   *
   * @param groupId - The group id of the state
   * @param key - The key of the item to decrement
   * @param property - The property to decrement
   * @param value - The value to decrement
   * @returns The item
   */
  decrementNumber<T>(groupId: string, key: string, property: string, value: number): Promise<T>
}
```

## Increment/Decrement

Usage

```typescript
type StateType = {
  name: string
  count: number
}

const groupId = 'my-group'
const key = 'my-key'

await state.set<StateType>(groupId, key, { name: 'John', count: 1 })

await state.incrementNumber<StateType>(groupId, key, 'count', 2)
await state.decrementNumber<StateType>(groupId, key, 'count', 1)
```

This means that there's no need to fetch the state first, and then update it, we can just update it directly.

## Set TTL to Items

```typescript
type StateManager = {
  /**
   * Sets a single item in the state
   *
   * @param groupId - The group id of the state
   * @param key - The key of the item to set
   * @param value - The value of the item to set
   * @param options - The options for the item
   * @returns The item
   */
  set<T>(groupId: string, key: string, value: T, options?: Options): Promise<T>
}
```

### Usage

```typescript
type StateType = {
  name: string
  count: number
}

const groupId = 'my-group'
const key = 'my-key'
const value = { name: 'John', count: 1 }

// This will set the item with a TTL of 3600 seconds (1 hour)
await state.set<StateType>(groupId, key, value, { ttl: 3600 })
```

## Updating subset of fields

```typescript
type StateType = {
  name: string
  count: number
}

const groupId = 'my-group'
const key = 'my-key'

// This will update only the name field, count will remain the same
await state.updateFields<StateType>(groupId, key, { name: 'New name' })
```

## Streams

Same logic should be applied to Streams: increment/decrement, set TTL to items.

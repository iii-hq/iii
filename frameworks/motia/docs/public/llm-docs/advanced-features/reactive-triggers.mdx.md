---
title: State & Stream Triggers
description: Build reactive workflows that respond to state changes and stream events automatically
---

State and stream triggers enable reactive patterns where Steps execute automatically in response to data changes — no polling, no manual coordination required.

## State Triggers

State triggers fire when data in Motia's state store changes. The `condition` function determines which changes should activate the Step.

### Parallel Merge Pattern

A common use case is triggering a Step when all parallel tasks complete:

```typescript
import type { Handlers, StepConfig, StateTriggerInput } from 'motia'

type TaskProgress = {
  totalSteps: number
  completedSteps: number
  results: Record<string, any>
}

export const config = {
  name: 'OnAllStepsComplete',
  description: 'Triggers when all parallel steps finish',
  triggers: [
    {
      type: 'state',
      condition: (input: StateTriggerInput<TaskProgress>) => {
        return (
          input.group_id === 'tasks' &&
          !!input.new_value &&
          input.new_value.totalSteps === input.new_value.completedSteps
        )
      },
    },
  ],
  enqueues: ['all-tasks-done'],
  flows: ['parallel-merge'],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async (input, { logger, enqueue }) => {
  logger.info('All parallel steps complete', {
    taskId: input.item_id,
    results: input.new_value.results,
  })

  await enqueue({
    topic: 'all-tasks-done',
    data: { taskId: input.item_id, results: input.new_value.results },
  })
}
```

### StateTriggerInput Type

The handler receives a `StateTriggerInput` object:

```typescript
type StateTriggerInput<T> = {
  group_id: string
  item_id: string
  new_value: T | null
  old_value: T | null
}
```

| Field | Description |
|---|---|
| `group_id` | The state namespace (e.g., `'tasks'`, `'orders'`) |
| `item_id` | The key of the changed item |
| `new_value` | The value after the change (`null` on delete) |
| `old_value` | The value before the change (`null` on create) |

### State Trigger Use Cases

- **Parallel merge** — Wait for all parallel tasks to complete before proceeding
- **Threshold alerts** — Trigger when a counter reaches a threshold
- **Status transitions** — React to status field changes (e.g., `pending` -> `approved`)
- **Data validation** — Validate data after it's written and trigger corrections

---

## Stream Triggers

Stream triggers fire when stream items are created, updated, deleted, or when custom events are sent. They enable Steps to react to real-time data changes.

### Basic Stream Trigger

```typescript
import type { Handlers, StepConfig, StreamWrapperMessage } from 'motia'

export const config = {
  name: 'OnDeploymentUpdate',
  description: 'Reacts to deployment stream updates',
  triggers: [
    {
      type: 'stream',
      streamName: 'deployment',
      groupId: 'data',
      condition: (input: StreamWrapperMessage) => input.event.type === 'update',
    },
  ],
  enqueues: ['deployment-changed'],
  flows: ['deployments'],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async (input, { logger, enqueue }) => {
  logger.info('Deployment updated', {
    streamName: input.streamName,
    groupId: input.groupId,
    eventType: input.event.type,
  })

  await enqueue({ topic: 'deployment-changed', data: input.event.data })
}
```

### StreamWrapperMessage Type

```typescript
type StreamWrapperMessage<TStreamData> = {
  type: 'stream'
  timestamp: number
  streamName: string
  groupId: string
  id?: string
  event: StreamCreate<TStreamData> | StreamUpdate<TStreamData> | StreamDelete<TStreamData> | StreamEvent
}
```

The `event` field contains one of:

| Event Type | Description | Data |
|---|---|---|
| `create` | A new item was created | `{ type: 'create', data: TStreamData }` |
| `update` | An existing item was updated | `{ type: 'update', data: TStreamData }` |
| `delete` | An item was deleted | `{ type: 'delete', data: TStreamData }` |
| `event` | A custom event was sent | `{ type: 'event', data: { type: string, data: TEventData } }` |

### Stream Trigger Configuration

| Property | Description |
|---|---|
| `streamName` | Name of the stream to watch (required) |
| `groupId` | Filter to a specific group (optional) |
| `itemId` | Filter to a specific item (optional) |
| `condition` | Function to filter events (optional) |

### Stream Trigger Use Cases

- **Notifications** — Send notifications when stream data changes
- **Audit logging** — Log all changes to a stream for compliance
- **Cascade updates** — Update related data when a stream item changes
- **Real-time analytics** — Process stream events for dashboards and metrics

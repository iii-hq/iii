# Infrastructure Config in Steps

This proposal we want to make Steps the single source of truth of the code and the infrastructure.

## Configuring Queues

Right now all Event Steps are converted to a Queue in a way. But we want to ensure when a developer
is creating a Step, they can configure what type of Queue system they want to use in each step.

```typescript
import { z } from 'zod'
import { EventConfig, Handlers } from 'motia'
import { ParallelMergeStep } from './parallelMerge.types'

export const config: EventConfig = {
  type: 'event',
  name: 'stepA',
  description: 'Hello from Step A',
  subscribes: ['pms.start'],
  emits: ['pms.stepA.done'],
  input: z.object({ traceId: z.string() }),
  flows: ['parallel-merge'],

  infrastructure: {
    handler: {
      ram: 2048, // ram in MB for the handler
      cpu: 2, // cpu in vCPU for the handler
      machineType: 'gpu', // GPU, CPU or Memory optimized
      timeout: 30, // timeout in seconds for the handler
    },
    queue: {
      type: 'fifo', // fifo or standard

      /**
       * will ensure all messages in the queue will be
       * processed in order when have the same messageGroupId
       */
      messageGroupId: 'traceId', // required for fifo
      maxRetries: 5, // max number of retries for a single message before moved to a dead letter queue
      visibilityTimeout: 31, // needs to be higher than the handler timeout
      delaySeconds: 10, // delay in seconds for the message to be processed
    },
  },
}

export const handler: Handlers['stepA'] = async (_, { emit, traceId, state, logger }) => {
  logger.info('[stepA] received pms.start')

  await new Promise((resolve) => setTimeout(resolve, 300))

  const partialResultA: ParallelMergeStep = { msg: 'Hello from Step A', timestamp: Date.now() }
  await state.set<ParallelMergeStep>(traceId, 'stepA', partialResultA)

  await emit({
    topic: 'pms.stepA.done',
    data: partialResultA,
  })
}
```

In the example above, we're configuring a FIFO Queue to run in 30 seconds, if it fails, the message will be back to the
queue 31 seconds later. So if the handler takes 10 seconds and fails, 21 seconds later it will be processed again.

But if it times out after 30 seconds, it will be moved back to the queue after 1 second.

## Queue Types

Queue types can be either:

- fifo
- standard

When FIFO, it's required to provide a messageGroupId which should be a key to the input data. So all messages with the same messageGroupId will be processed in order.

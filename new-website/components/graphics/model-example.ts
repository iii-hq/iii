export type Primitive = 'worker' | 'function' | 'trigger'
export type ExamplePhase = 'idle' | 'routing' | 'executing' | 'complete'

export const primitives = [
  {
    id: 'worker',
    name: 'Worker',
    summary: 'A process connected to the engine over one WebSocket.',
    heading: 'An API, a Python script, a Rust binary, a browser tab, an agent.',
    description: 'The queue, the scheduler, and the ADE are workers too.',
    code: `import { registerWorker } from 'iii-sdk'

const iii = registerWorker(
  'ws://localhost:49134',
  { workerName: 'reports' },
)`,
  },
  {
    id: 'function',
    name: 'Function',
    summary: 'Work with a stable name and a JSON schema for its input and output.',
    heading: 'Any worker calls it by name, without knowing where it runs.',
    description: 'The engine holds the live registry of every worker, function, and trigger, and routes every call.',
    code: `iii.registerFunction(
  'reports::daily',
  async () => ({
    status: 'ready',
  }),
)`,
  },
  {
    id: 'trigger',
    name: 'Trigger',
    summary: 'What runs a function.',
    heading: 'A direct call, an HTTP request, a cron schedule, a queue message, a state change.',
    description: 'Or any event type a worker defines.',
    code: `iii.registerTrigger({
  type: 'cron',
  function_id: 'reports::daily',
  config: {
    expression: '0 0 9 * * * *',
  },
})`,
  },
] as const

export const completeExample = primitives.map(({ code }) => code).join('\n\n')

/**
 * Type-level regression test for `useNamespace` and namespace-carrying refs.
 * Not executed at runtime (vitest only runs `*.test.ts`); `tsc --noEmit` (CI)
 * compiles it. Guards that:
 *   - `useNamespace(ns)` returns an `IIIClient`,
 *   - a `FunctionRef` exposes `function_id` and `namespace`,
 *   - a ref can be passed directly as `function_id` to `trigger` / `registerTrigger`.
 */
import type { IIIClient } from '../src/index'
import { registerWorker } from '../src/index'

// biome-ignore lint/correctness/noUnusedVariables: compile-only assertions
async function useNamespaceAssertions() {
  const worker = registerWorker('ws://localhost:49134')

  // useNamespace returns a worker view (IIIClient) bound to the namespace.
  const agent: IIIClient = worker.useNamespace('my-agent')

  // registerFunction returns a ref carrying its id and namespace.
  const ref = agent.registerFunction('run', async () => ({ ok: true }))
  const id: string = ref.function_id
  const ns: string | undefined = ref.namespace
  void id
  void ns

  // A ref can be passed as function_id to trigger (routes to ref.namespace).
  await agent.trigger({ function_id: ref, payload: {} })

  // A ref can be passed as function_id to registerTrigger.
  agent.registerTrigger({ type: 'cron', function_id: ref, config: {} })

  // A bare string still works, and an explicit namespace still overrides.
  await agent.trigger({ function_id: 'greet', payload: {}, namespace: 'default' })
}

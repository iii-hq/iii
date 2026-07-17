/**
 * Type-level regression test for MOT-4076: `worker.trigger` must type-check
 * without explicit type arguments. Not executed at runtime — vitest only picks
 * up `*.test.ts` — but `tsc --noEmit` (run in CI) compiles it.
 */
import { registerWorker } from '../src/index'

// biome-ignore lint/correctness/noUnusedVariables: compile-only assertions
async function triggerTypingAssertions() {
  const worker = registerWorker('ws://localhost:49134')

  // No type args: TOutput defaults to any, so property access compiles.
  const { workers } = await worker.trigger({
    function_id: 'engine::workers::list',
    payload: {},
  })
  void workers

  // Explicit <any, any> keeps compiling.
  // biome-ignore lint/suspicious/noExplicitAny: exercising the explicit-any call style
  const { functions } = await worker.trigger<any, any>({
    function_id: 'engine::functions::list',
    payload: { include_internal: false },
  })
  void functions

  // Fully typed style keeps compiling.
  const typed = await worker.trigger<{ name: string }, { message: string }>({
    function_id: 'greet',
    payload: { name: 'World' },
  })
  void typed.message

  // Input type alone now also works (output defaults to any).
  const { triggers } = await worker.trigger<{ include_internal: boolean }>({
    function_id: 'engine::triggers::list',
    payload: { include_internal: false },
  })
  void triggers
}

/**
 * Type-level regression test for MOT-4076: `worker.call` must type-check
 * without explicit type arguments. Not executed at runtime — vitest only picks
 * up `*.test.ts` — but `tsc --noEmit` (run in CI) compiles it.
 */
import type { Call, FunctionRef, MatchHook, Observer, TriggerRequest } from '../src/index'
import { registerWorker } from '../src/index'

// biome-ignore lint/correctness/noUnusedVariables: compile-only assertions
async function callTypingAssertions() {
  const worker = registerWorker('ws://localhost:49134')

  // No type args: TOutput defaults to any, so property access compiles.
  const { workers } = await worker.call({
    function_id: 'engine::workers::list',
    payload: {},
  })
  void workers

  // Explicit <any, any> keeps compiling.
  // biome-ignore lint/suspicious/noExplicitAny: exercising the explicit-any call style
  const { functions } = await worker.call<any, any>({
    function_id: 'engine::functions::list',
    payload: { include_internal: false },
  })
  void functions

  // Fully typed style keeps compiling.
  const typed = await worker.call<{ name: string }, { message: string }>({
    function_id: 'greet',
    payload: { name: 'World' },
  })
  void typed.message

  // Input type alone now also works (output defaults to any).
  const { observers } = await worker.call<{ include_internal: boolean }>({
    function_id: 'engine::observers::list',
    payload: { include_internal: false },
  })
  void observers

  // The trigger alias takes the same request.
  const request: TriggerRequest<{ id: string }> = { function_id: 'orders::get', payload: { id: '1' } }
  await worker.trigger(request)

  // A call through an observer takes no function_id, namespace or metadata.
  const observer = worker.observers('http::on-request')[0] as Observer
  await worker.call({ observer, payload: { path: '/' } })
  // @ts-expect-error: an observer call names no function
  await worker.call({ observer, function_id: 'x::y', payload: {} })
  // @ts-expect-error: an observer call takes no metadata (its target receives the observer's)
  await worker.call({ observer, payload: {}, metadata: { tenant: 'acme' } })

  // The handler's types reach the hooks of its FunctionRef, in both match forms.
  type RowChange = { db: string; table: string; op: string }
  const rowChanged: FunctionRef<RowChange, RowChange> = worker
    .registerFunction('database::on-row-changed', (change: RowChange) => change, { private: true })
    .onObserverMatch({
      observer: (obs) => obs.args.db as string,
      call: (call) => [call.input.db, `${call.input.db}|${call.input.table}`],
    })
    .onObserverMatch((obs, call) => obs.args.table === call.input.table)
  rowChanged.isObserved({ input: { db: 'primary', table: 'orders', op: 'insert' } })
  const keys: MatchHook<RowChange, RowChange> = { observer: () => 'k', call: () => ['k'] }
  rowChanged.onObserverMatch(keys).onObserverMatch(null)

  // An async handler's resolved type is the output observers observe; the deliver hook's
  // return type is what the caller gets back.
  worker
    .registerFunction('orders::create', async (order: { qty: number }) => ({ id: 'o-1', qty: order.qty }))
    .onObserverDeliver((call: Call<{ qty: number }, { id: string; qty: number }>, observers) =>
      observers.length > 0 ? (call.output?.id ?? 'none') : 'unobserved',
    )

  // Untyped hooks by id read their call's input.
  worker
    .onObserverMatch('cron::on-tick', (obs, call) => obs.args.expression === call.input.expression)
    .onObserverMatch('database::on-row-changed', { observer: (obs) => obs.id, call: (call) => call.input.table })
    .onObserverDeliver('http::on-request', async (call, [route]) => (route ? route.matches(call) : false))
  worker.onObserverAdded(() => undefined).onObserverRemoved('cron::on-tick', null)
}

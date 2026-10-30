/**
 * Type-level regression test for MOT-4076: `worker.call` must type-check
 * without explicit type arguments. Not executed at runtime — vitest only picks
 * up `*.test.ts` — but `tsc --noEmit` (run in CI) compiles it.
 */
import type { Call, CallRequest, FunctionRef, MatchHook, Observer, TriggerRequest } from '../src/index'
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

  // Input type alone works too (output defaults to any).
  const { observers } = await worker.call<{ include_internal: boolean }>({
    function_id: 'engine::observers::list',
    payload: { include_internal: false },
  })
  void observers

  // The trigger alias takes the same request, the browser's trace context included.
  const request: TriggerRequest<{ id: string }> = {
    function_id: 'orders::get',
    payload: { id: '1' },
    traceparent: '00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01',
  }
  await worker.trigger(request)

  // A call through an observer takes no function_id, namespace or metadata.
  const observer = worker.observers('ui::on-command')[0] as Observer
  await worker.call({ observer, payload: { name: 'save' } })
  // @ts-expect-error: an observer call names no function
  await worker.call({ observer, function_id: 'x::y', payload: {} })
  // @ts-expect-error: an observer call takes no metadata (its target receives the observer's)
  await worker.call({ observer, payload: {}, metadata: { tenant: 'acme' } })
  // A request built before the call gets no excess-property check; `metadata?: never` refuses it.
  const viaObserverWithMetadata = { observer, payload: {}, metadata: { tenant: 'acme' } }
  // @ts-expect-error: an observer call takes no metadata (its target receives the observer's)
  await worker.call(viaObserverWithMetadata)
  // On the observer variant, metadata reads as undefined.
  const viaObserver: Extract<CallRequest, { observer: Observer }> = { observer, payload: {} }
  const noMetadata: undefined = viaObserver.metadata
  void noMetadata

  // The handler's types reach the hooks of its FunctionRef, in both match forms.
  type Click = { selector: string; x: number; y: number }
  const clicks: FunctionRef<Click, Click> = worker
    .registerFunction('ui::on-click', (click: Click) => click, { private: true })
    .onObserverMatch({
      observer: (obs) => obs.args.selector as string,
      call: (call) => [call.input.selector, '*'],
    })
    .onObserverMatch((obs, call) => obs.args.selector === call.input.selector)
  clicks.isObserved({ input: { selector: '#buy', x: 0, y: 0 } })
  const keys: MatchHook<Click, Click> = { observer: () => 'k', call: () => ['k'] }
  clicks.onObserverMatch(keys).onObserverMatch(null)

  // An async handler's resolved type is the output observers observe; the deliver hook's
  // return type is what the caller gets back.
  worker
    .registerFunction('orders::create', async (order: { qty: number }) => ({ id: 'o-1', qty: order.qty }))
    .onObserverDeliver((call: Call<{ qty: number }, { id: string; qty: number }>, observers) =>
      observers.length > 0 ? (call.output?.id ?? 'none') : 'unobserved',
    )

  // Untyped hooks by id read their call's input.
  worker
    .onObserverMatch('ui::on-click', (obs, call) => obs.args.selector === call.input.selector)
    .onObserverMatch('ui::on-key', { observer: (obs) => obs.id, call: (call) => call.input.key })
    .onObserverDeliver('ui::on-command', async (call, [route]) => (route ? route.matches(call) : false))
  worker.onObserverAdded(() => undefined).onObserverRemoved('ui::on-click', null)
  worker.observe('ui::on-click', 'ui::highlight')
}

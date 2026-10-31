/**
 * observe hot paths on a Node host: the rows of the observe v2 performance
 * budget (spec §13) that run in the SDK, for the Node baseline (I1). The
 * names follow `sdk/packages/rust/iii/benches/observe_bench.rs`.
 *
 * - `isObserved()` with nobody observing and with an observer: one load;
 * - `isObserved(call)` on a function matched by keys whose 10,000 observers
 *   are filed under other keys: one lookup per call key;
 * - `handler_alone/echo`: what the echo handler costs by itself;
 * - a local `call()` with no observers (`call_local/no_observers`): one
 *   copy of the input for the handler and one of the result for the caller,
 *   nothing on the wire; of a private function (`call_local/private`): no
 *   copy at all;
 * - a local call with 10 observers, none matching; with 1 matching observer
 *   (one report item, batched); matching by keys, 1 matching observer among
 *   10,000;
 * - a local call of a function whose 1,000 observers each filter on their own
 *   `input.tenant` (`$eq`), with 1 of them matching and with none: the SDK
 *   matches the call through an index of the filters' equality conditions;
 * - an observed event source (`call_local/event_source_observed`, a private
 *   function that returns its input) and an observed echo that is not private
 *   (`call_local/echo_observed`): each report item carries the value once
 *   (`output_is_input`);
 * - a routed call (an `invokefunction` from the engine, answered with
 *   `invocationresult`) of a function nobody observes, of one with a
 *   matching observer (its id rides the answer, no extra frame), and of the
 *   function with 1,000 equality-filtered observers.
 *
 * The fake engine (tests/observe-fake-engine.ts) confirms the functions with
 * the observers each case needs; the socket counts what the SDK sends, and
 * the totals are printed at the end.
 *
 * `pnpm --filter iii-sdk bench`
 */
import { afterAll, bench, describe, vi } from 'vitest'

vi.mock('ws', async () => (await import('../tests/observe-fake-engine')).wsModule())
vi.resetModules()
const fake = await import('../tests/observe-fake-engine')
const { registerWorker } = await import('../src/index')
const { functionRegistered, observerInfo, settle } = fake

const KEYED_OBSERVERS = 10_000
const FILTERED_OBSERVERS = 1_000
const TIME_MS = 1000

/** What the SDK put on the wire, counted without keeping the frames. */
const wire = { reportFrames: 0, results: 0, observedResults: 0, other: 0 }
/** Report items per function, and how many carried the value once (`output_is_input`, no `output`). */
const items = new Map<string, { items: number; once: number }>()
let answered: (() => void) | undefined

/** Counts the items of one `invocationsobserved` frame by function (each item starts with its `function_id`). */
function countItems(frame: string): void {
  for (const item of frame.split('{"function_id":"').slice(1)) {
    const fn = item.slice(0, item.indexOf('"'))
    const counts = items.get(fn) ?? { items: 0, once: 0 }
    counts.items++
    if (item.includes('"output_is_input":true') && !item.includes('"output":')) counts.once++
    items.set(fn, counts)
  }
}

const iii = registerWorker('ws://bench.test', {
  otel: { enabled: false },
  enableMetricsReporting: false,
  workerName: 'bench',
  reconnectionConfig: { initialDelayMs: 10, jitterFactor: 0 },
})
const socket = fake.lastSocket()
socket.send = (data: Buffer | string, callback?: (err?: Error | null) => void) => {
  const text = String(data)
  if (text.startsWith('{"type":"invocationsobserved"')) {
    wire.reportFrames++
    countItems(text)
  } else if (text.includes('"type":"invocationresult"')) {
    wire.results++
    if (text.includes('"observed":')) wire.observedResults++
    answered?.()
  } else {
    wire.other++
  }
  callback?.()
}
socket.open()

type Input = { n: number }
const echo = (input: Input) => input

const plain = iii.registerFunction('bench::plain', echo)
iii.registerFunction('bench::private', echo, { private: true })
const tenNone = iii.registerFunction('bench::ten-none', echo)
const oneMatch = iii.registerFunction('bench::one-match', (input: Input) => ({ seen: input.n }))
const keyed = iii
  .registerFunction('bench::keyed', (input: { expression: string; n: number }) => input, { private: true })
  .onObserverMatch({
    observer: (obs) => obs.args.expression as string,
    call: (call) => call.input.expression,
  })
const source = iii.registerFunction('bench::source', echo, { private: true })
const echoObserved = iii.registerFunction('bench::echo-observed', echo)
const tenants = iii.registerFunction('bench::tenants', (input: { tenant: string; n: number }) => ({ seen: input.n }))

const capture = { capture: ['input', 'output'] }
socket.deliver(functionRegistered('bench::plain'))
socket.deliver(functionRegistered('bench::private'))
socket.deliver(
  functionRegistered(
    'bench::ten-none',
    Array.from({ length: 10 }, (_, i) =>
      observerInfo(`none-${i}`, 'bench::target', {
        filters: { $and: [{ 'input.n': { $eq: -1 - i } }] },
        capture: ['input'],
      }),
    ),
  ),
)
socket.deliver(
  functionRegistered('bench::one-match', [
    observerInfo('all', 'bench::target', { filters: { $and: [{ 'input.n': { $gte: 0 } }] }, ...capture }),
  ]),
)
socket.deliver(
  functionRegistered(
    'bench::keyed',
    Array.from({ length: KEYED_OBSERVERS }, (_, i) =>
      observerInfo(`keyed-${i}`, 'bench::target', {
        args: { expression: i === 0 ? 'hit' : `expr-${i}` },
        capture: ['input'],
      }),
    ),
  ),
)
socket.deliver(functionRegistered('bench::source', [observerInfo('source', 'bench::target', capture)]))
socket.deliver(functionRegistered('bench::echo-observed', [observerInfo('echo', 'bench::target', capture)]))
socket.deliver(
  functionRegistered(
    'bench::tenants',
    Array.from({ length: FILTERED_OBSERVERS }, (_, i) =>
      observerInfo(`tenant-${i}`, 'bench::target', { filters: { 'input.tenant': { $eq: `t-${i}` } }, ...capture }),
    ),
  ),
)
const accepted = () =>
  tenNone.observers().length === 10 &&
  oneMatch.observers().length === 1 &&
  keyed.observers().length === KEYED_OBSERVERS &&
  source.observers().length === 1 &&
  echoObserved.observers().length === 1 &&
  tenants.observers().length === FILTERED_OBSERVERS
for (let round = 0; round < 1000 && !accepted(); round++) await settle()
if (!accepted()) throw new Error('the fake engine did not get every observer accepted')

afterAll(async () => {
  console.log('fake engine frames:', JSON.stringify(wire))
  console.log('fake engine report items:', JSON.stringify(Object.fromEntries(items)))
  await iii.shutdown()
})

let i = 0
const numbered = () => ({ n: i++ })
const call = (function_id: string, payload: unknown) => iii.call({ function_id, payload })

/** One routed call: the engine's `invokefunction`, resolved when the SDK answers it. */
function routed(text: string): Promise<void> {
  return new Promise<void>((resolve) => {
    answered = resolve
    socket.emit('message', text)
  })
}
const invoke = (function_id: string) =>
  JSON.stringify({ type: 'invokefunction', invocation_id: 'bench-routed', function_id, data: { n: 1 } })
const routedPlain = invoke('bench::plain')
const routedOneMatch = invoke('bench::one-match')
const routedTenants = JSON.stringify({
  type: 'invokefunction',
  invocation_id: 'bench-routed',
  function_id: 'bench::tenants',
  data: { tenant: 't-7', n: 1 },
})

const miss = { input: { expression: 'never-filed', n: 0 } }

describe('isObserved', () => {
  bench('is_observed/false', () => {
    if (plain.isObserved()) throw new Error('bench::plain is observed')
  })
  bench('is_observed/true', () => {
    if (!oneMatch.isObserved()) throw new Error('bench::one-match is not observed')
  })
  bench('is_observed_for/key_miss_of_10000', () => {
    if (keyed.isObserved(miss)) throw new Error('the miss is observed')
  })
})

describe('local calls', () => {
  bench('handler_alone/echo', () => {
    echo(numbered())
  })
  bench('call_local/no_observers', () => call('bench::plain', numbered()), { time: TIME_MS })
  bench('call_local/private', () => call('bench::private', numbered()), { time: TIME_MS })
  bench('call_local/10_observers_none_matching', () => call('bench::ten-none', numbered()), { time: TIME_MS })
  bench('call_local/1_observer_matching', () => call('bench::one-match', numbered()), { time: TIME_MS })
  bench('call_local/match_keys_1_of_10000', () => call('bench::keyed', { expression: 'hit', n: i++ }), {
    time: TIME_MS,
  })
  bench('call_local/event_source_observed', () => call('bench::source', numbered()), { time: TIME_MS })
  bench('call_local/echo_observed', () => call('bench::echo-observed', numbered()), { time: TIME_MS })
  bench('call_local/eq_filters_1_of_1000', () => call('bench::tenants', { tenant: 't-7', n: i++ }), {
    time: TIME_MS,
  })
  bench('call_local/eq_filters_none_of_1000', () => call('bench::tenants', { tenant: 'nobody', n: i++ }), {
    time: TIME_MS,
  })
})

describe('routed calls', () => {
  bench('call_routed/no_observers', () => routed(routedPlain), { time: TIME_MS })
  bench('call_routed/1_observer_matching', () => routed(routedOneMatch), { time: TIME_MS })
  bench('call_routed/eq_filters_1000', () => routed(routedTenants), { time: TIME_MS })
})

// Derived from sdk/packages/node/iii/tests/observe-protocol-fixtures.test.ts by scripts/sync-browser-observe.py; edit those, not this file.
import { readdirSync, readFileSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { ObserveRejection } from '../src/errors'
import { CallAction, registerWorker } from '../src/index'
import type { Call, Observer } from '../src/observe-types'
import * as wire from '../src/observe-wire'
import { buildLocalInvocationStats, encodeReportItem } from '../src/reporter'
import type { ISdk } from '../src/types'
import * as fake from './observe-fake-engine'

/**
 * The observe wire contract (`sdk/fixtures/protocol/observe`, CONTRACT D5):
 * every worker → engine message the SDK builds equals its fixture as a JSON
 * value given the same inputs, and every engine → worker message decodes
 * without loss and encodes back to the same value.
 */
const DIR = resolve(__dirname, '../../../../fixtures/protocol/observe')
type Json = Record<string, unknown>

function fixture(name: string): Json {
  return JSON.parse(readFileSync(join(DIR, name), 'utf8')) as Json
}

/** JSON value equality: key order and whitespace do not matter, keys and values do. */
function asJson(value: unknown): unknown {
  return JSON.parse(JSON.stringify(value))
}

function body(frame: Json): Json {
  const { type: _, ...rest } = frame
  return rest
}

let sdk: ISdk | undefined
afterEach(async () => {
  vi.restoreAllMocks()
  await sdk?.shutdown()
  sdk = undefined
})

function connect(opts: { workerName?: string; namespace?: string } = {}) {
  sdk = registerWorker('ws://fixtures.test', {
    reconnectionConfig: { maxRetries: 0 },
    ...opts,
  } as never)
  const socket = fake.lastSocket()
  socket.open()
  socket.clear()
  return { sdk, socket }
}

/** What an item carries that depends on the clock: replaced by the fixture's values. */
function timed(item: Json, like: Json): Json {
  return { ...item, started_at_ms: like.started_at_ms, duration_ms: like.duration_ms }
}

/**
 * Every fixture this file tests; the coverage test below fails when a new
 * fixture file is not listed (and handled) here.
 */
const HANDLED = {
  workerToEngine: [
    'observe.json',
    'observe-minimal.json',
    'observe-on-error.json',
    'unobserve.json',
    'observerack.json',
    'observerack-error.json',
    'invocationsobserved.json',
    'invocationsobserved-event-source.json',
    'invocationsobserved-routed.json',
    'invocationsobserved-routed-items.json',
    'localinvocationstats.json',
    'registerfunction.json',
    'registerfunction-private.json',
    'registerfunction-not-observable.json',
    'invokefunction-observer-call.json',
    'invokefunction-observer-call-namespace.json',
    'invokefunction-observer-call-void.json',
    'invokefunction-legacy.json',
    'invokefunction-workers-register.json',
    'invocationresult-observed.json',
    'invocationresult-observed-error.json',
    'invocationresult-hook-result.json',
  ],
  engineToWorker: [
    'workerregistered.json',
    'workerregistered-v1.json',
    'workerregistered-legacy.json',
    'registrationrejected-unsupported.json',
    'registrationrejected-invalid-args-format.json',
    'invocationresult.json',
    'invocationresult-not-function-owner.json',
    'observeresult.json',
    'observeresult-error.json',
    'observerstatechanged.json',
    'observerstatechanged-reason.json',
    'functionregistered.json',
    'functionregistered-empty.json',
    'observeradded.json',
    'observerremoved.json',
    'invokefunction-activation.json',
    'invokefunction-activation-engine-source.json',
    'invokefunction-caller.json',
    'invokefunction-observer-call-forwarded.json',
  ],
  /** The 0.25 `observe` object: no v2 SDK builds or decodes it (README, engine-only). */
  engineOnly: ['registerfunction-v1-owner.json', 'registerfunction-v1-not-observable.json'],
}

describe('worker → engine: what the SDK builds equals the fixture', () => {
  it.each([
    [
      'observe.json',
      {
        source: {
          function_id: 'cron::on-tick',
          namespace: 'default',
          args: { expression: '0 0 9 * * Mon-Fri' },
          filters: { 'input.scheduled_time': { $gte: '2026-11-01T00:00:00Z' } },
          capture: ['input'],
          on: 'success',
          deliver: 'envelope',
        },
        target: { function_id: 'local::cron-handler', namespace: 'default' },
        options: {
          id: '5b0c3e1a-8f2d-4c6b-9a7e-1d2f3a4b5c6d',
          metadata: { __binding: 'weekday-digest' },
          max_fires: 1,
          expires_in_ms: 600000,
          max_in_flight: 64,
        },
      },
    ],
    [
      'observe-minimal.json',
      {
        // the string shorthand: observe('fn::id', target)
        source: 'orders::archive',
        target: 'search::reindex-order',
        options: { id: 'd94f3a5b-6c7d-4e8f-a01b-2c3d4e5f6a7b' },
      },
    ],
    [
      'observe-on-error.json',
      {
        source: {
          function_id: 'payments::charge',
          filters: { 'input.amount': { $gte: 500 } },
          capture: ['input', 'error', 'caller'],
          on: 'error',
        },
        target: 'alerts::payment-failed',
        options: {
          id: 'c81e2f4a-5b6c-4d7e-9f0a-1b2c3d4e5f60',
          metadata: { team: 'payments-oncall' },
          max_in_flight: 4,
        },
      },
    ],
  ] as const)('%s (iii.observe)', (name, input) => {
    const { sdk, socket } = connect()
    sdk.observe(input.source as never, input.target as never, input.options as never)
    expect(socket.framesOf('observe')).toEqual([fixture(name)])
  })

  it('unobserve.json (ObserverHandle.dispose)', async () => {
    const { sdk, socket } = connect()
    const handle = sdk.observe('orders::archive', 'search::reindex-order', {
      id: '5b0c3e1a-8f2d-4c6b-9a7e-1d2f3a4b5c6d',
    })
    await handle.dispose()
    expect(socket.framesOf('unobserve')).toEqual([fixture('unobserve.json')])
  })

  it('observerack.json and observerack-error.json (the host acks observeradded.json)', async () => {
    for (const [name, revision, hook] of [
      ['observerack.json', 13, () => undefined],
      [
        'observerack-error.json',
        15,
        () => {
          throw new ObserveRejection(
            'INVALID_CRON_EXPRESSION',
            "invalid day-of-week field 'Mon-Fry' in '0 0 9 * * Mon-Fry'",
          )
        },
      ],
    ] as const) {
      const { sdk, socket } = connect()
      const expected = fixture(name)
      sdk.registerFunction('cron::on-tick', (tick: unknown) => tick).onObserverAdded(hook)
      socket.deliver(fake.functionRegistered('cron::on-tick', [], { revision: 12 }))
      const added = fixture('observeradded.json')
      socket.deliver({ ...added, revision, observer: { ...(added.observer as Json), id: expected.id } })
      await fake.settle()
      expect(socket.framesOf('observerack')).toEqual([expected])
      await sdk.shutdown()
    }
  })

  it('invocationsobserved.json (report items of local calls)', () => {
    const expected = fixture('invocationsobserved.json')
    const [failed, nullOutput, created] = expected.items as Json[]
    const items = [
      encodeReportItem({
        function_id: 'payments::charge',
        namespace: 'default',
        invocation_id: failed.invocation_id as string,
        matched: failed.matched as string[],
        status: 'error',
        values: {
          input: failed.input,
          error: failed.error,
          caller: failed.caller as never,
          metadata: failed.metadata,
        },
        captures: ['input', 'error', 'caller', 'metadata'],
        started_at_ms: 1760350000123,
        duration_ms: 12.75,
        observe_depth: 0,
        traceparent: failed.traceparent as string,
      }),
      encodeReportItem({
        function_id: 'orders::archive',
        namespace: 'default',
        invocation_id: nullOutput.invocation_id as string,
        matched: nullOutput.matched as string[],
        status: 'success',
        // a function that returned null: `output: null` is a value, not absent
        values: { input: nullOutput.input, output: null },
        captures: ['input', 'output', 'caller', 'metadata'],
        started_at_ms: 1760350000180,
        duration_ms: 0.42,
        observe_depth: 0,
      }),
      encodeReportItem({
        function_id: 'orders::create',
        namespace: 'default',
        invocation_id: created.invocation_id as string,
        matched: created.matched as string[],
        status: 'success',
        values: {
          input: created.input,
          output: created.output,
          caller: created.caller as never,
          metadata: created.metadata,
        },
        captures: ['input', 'output', 'caller', 'metadata'],
        output_is_input: false,
        started_at_ms: 1760350000150,
        duration_ms: 3.25,
        observe_depth: 0,
        traceparent: created.traceparent as string,
      }),
    ]
    const frame = JSON.parse(`{"type":"invocationsobserved","items":[${items.join(',')}]}`)
    expect(frame).toEqual(expected)
  })

  it('invocationsobserved-event-source.json (a private source called inside a routed handler returns its input)', async () => {
    fake.fakeClock()
    const expected = fixture('invocationsobserved-event-source.json')
    const [want] = expected.items as Json[]
    const { sdk, socket } = connect({ workerName: 'database' })
    sdk.registerFunction('database::on-row-changed', (change: unknown) => change, { private: true })
    sdk.registerFunction('database::execute', async () => {
      await sdk.call({ function_id: 'database::on-row-changed', payload: want.input })
      return { rows: 1 }
    })
    socket.deliver(
      fake.functionRegistered(
        'database::on-row-changed',
        (want.matched as string[]).map((id) => fake.observerInfo(id, 'billing::on-invoice-paid')),
      ),
    )
    socket.deliver(fake.functionRegistered('database::execute'))
    await fake.settle()
    socket.deliver(fixture('invokefunction-caller.json'))
    await fake.settle()
    await fake.elapse(10)
    const [frame] = socket.framesOf('invocationsobserved')
    const [item] = frame.items as Json[]
    // The id and the trace context are the local call's own.
    expect({ ...frame, items: [timed(item, want)] }).toEqual({
      ...expected,
      items: [{ ...want, invocation_id: item.invocation_id, traceparent: item.traceparent, baggage: item.baggage }],
    })
    expect(item).not.toHaveProperty('output')
  })

  it.each([
    'invocationsobserved-routed.json',
    'invocationsobserved-routed-items.json',
  ])('%s (the item shape: encodes from its values)', (name) => {
    // Decision 38 (a93643b29): a host reports a routed call only through
    // `invocationresult.observed`, so the SDK builds these items for no call;
    // their shape is the one every local report item has.
    const expected = fixture(name)
    const items = (expected.items as Json[]).map((item) =>
      encodeReportItem({
        function_id: item.function_id as string,
        namespace: item.namespace as string,
        invocation_id: item.invocation_id as string,
        matched: item.matched as string[],
        status: item.status as 'success',
        values: { input: item.input, output: item.output, caller: item.caller as never },
        captures: ['input', 'output', 'caller'],
        started_at_ms: item.started_at_ms as number,
        duration_ms: item.duration_ms as number,
        observe_depth: item.observe_depth as number,
        traceparent: item.traceparent as string,
      }),
    )
    expect(JSON.parse(`{"type":"invocationsobserved","items":[${items.join(',')}]}`)).toEqual(expected)
  })

  it('invocationresult-hook-result.json (a routed call answered by onObserverDeliver: its value, no observed)', async () => {
    const expected = fixture('invocationresult-hook-result.json')
    const { sdk, socket } = connect({ workerName: 'orders' })
    sdk
      .registerFunction('orders::submit', (order: Json) => ({ order_id: 'ord_1004', sku: order.sku, total: 1250 }))
      .onObserverDeliver((call) => ({ order_id: call.output?.order_id, status: 'accepted' }))
    socket.deliver(
      fake.functionRegistered('orders::submit', [fake.observerInfo('c6ccda45-4a35-461a-a9c4-a318eec7c36a', 'w::r')]),
    )
    await fake.settle()
    socket.clear()
    socket.deliver(
      fake.invokeFunction(
        'orders::submit',
        { sku: 'A-1', qty: 2 },
        { invocation_id: expected.invocation_id as string, traceparent: expected.traceparent as string },
      ),
    )
    await fake.settle()
    expect(socket.frames()).toEqual([expected])
  })

  it("invocationresult-observed.json (iii's delivery of a routed call: the ids ride the answer)", async () => {
    const expected = fixture('invocationresult-observed.json')
    const { sdk, socket } = connect({ workerName: 'orders' })
    sdk.registerFunction('orders::create', () => expected.result)
    socket.deliver(
      fake.functionRegistered(
        'orders::create',
        (expected.observed as string[]).map((id) => fake.observerInfo(id, 'alerts::big-order')),
      ),
    )
    await fake.settle()
    socket.clear()
    socket.deliver(
      fake.invokeFunction(
        'orders::create',
        { sku: 'A-1', qty: 2 },
        {
          invocation_id: expected.invocation_id as string,
          traceparent: expected.traceparent as string,
        },
      ),
    )
    await fake.settle()
    expect(socket.frames()).toEqual([expected])
  })

  it('invocationresult-observed-error.json (builds; observed kept on an error answer)', () => {
    const expected = fixture('invocationresult-observed-error.json')
    const decoded = wire.decodeInvocationResult(body(expected))
    expect(decoded.observed).toEqual(['c81e2f4a-5b6c-4d7e-9f0a-1b2c3d4e5f60'])
    expect(decoded).not.toHaveProperty('result')
    expect({ type: 'invocationresult', ...wire.buildInvocationResult(decoded as never) }).toEqual(expected)
  })

  it('localinvocationstats.json', () => {
    const stats = new Map([
      ['database::on-row-changed', { count: 48211, errors: 0, duration_ms_sum: 190.2 }],
      ['database::query', { count: 37, errors: 2, duration_ms_sum: 15.5 }],
    ])
    const drops = new Map([
      ['database::on-row-changed', new Map([['BUFFER_FULL', 12]])],
      ['database::execute', new Map([['LATE_DELIVERY', 1]])],
    ])
    expect({ type: 'localinvocationstats', ...buildLocalInvocationStats(10000, stats, drops) }).toEqual(
      fixture('localinvocationstats.json'),
    )
  })

  it.each([
    ['registerfunction.json', {}],
    ['registerfunction-private.json', { private: true, standby: true, localDispatch: false }],
    ['registerfunction-not-observable.json', { observable: false, private: false }],
  ] as const)('%s (registerFunction; hooks, standby and localDispatch never travel)', async (name, extra) => {
    const expected = fixture(name)
    const { sdk, socket } = connect()
    sdk
      .registerFunction(expected.id as string, async () => null, {
        description: expected.description as string,
        request_format: expected.request_format as never,
        response_format: expected.response_format as never,
        args_format: expected.args_format as never,
        ...extra,
      })
      .onObserverAdded(() => undefined)
      .onObserverRemoved(() => undefined)
    expect(socket.framesOf('registerfunction')).toEqual([expected])
  })

  it('no registerfunction carries the 0.25 observe object (registerfunction-v1-*.json are engine-only)', () => {
    const { sdk, socket } = connect()
    sdk.registerFunction('a::plain', async () => null)
    sdk.registerFunction('b::source', (e: unknown) => e, { private: true, args_format: { type: 'object' } })
    sdk.registerFunction('c::secret', async () => null, { observable: false })
    sdk.registerFunction('d::routed', async () => null).onObserverDeliver((call) => call.output)
    for (const name of HANDLED.engineOnly) expect(fixture(name)).toHaveProperty('observe')
    const frames = socket.framesOf('registerfunction')
    expect(frames.map((f) => f.id)).toEqual(['a::plain', 'b::source', 'c::secret', 'd::routed'])
    for (const frame of frames) expect(frame).not.toHaveProperty('observe')
  })

  it('invokefunction-observer-call.json (a call through an observer: target, activation, caller, no namespace)', () => {
    const expected = fixture('invokefunction-observer-call.json')
    const built = wire.buildInvokeFunction({
      invocation_id: expected.invocation_id as string,
      function_id: 'users::get',
      data: expected.data,
      traceparent: expected.traceparent as string,
      caller: wire.callerFor({
        function_id: 'http::on-request',
        invocation_id: '9b1d3f5a-7c9e-4b1d-8f3a-5c7e9b1d3f5a',
      }),
      observe_depth: 0,
      activation: { observer_id: 'b6c4e2a0-9f1d-4b3a-8e7c-6d5f4e3a2b1c' },
    })
    expect({ type: 'invokefunction', ...built }).toEqual(expected)
  })

  it('invokefunction-observer-call-namespace.json (decodes and re-encodes; this SDK sends no namespace on it)', () => {
    const expected = fixture('invokefunction-observer-call-namespace.json')
    expect({ type: 'invokefunction', ...wire.buildInvokeFunction(wire.decodeInvokeFunction(body(expected))) }).toEqual(
      expected,
    )
  })

  it('invokefunction-observer-call-void.json (iii.call({ observer, payload, action: CallAction.Void() }))', async () => {
    const expected = fixture('invokefunction-observer-call-void.json')
    const activation = expected.activation as Json
    const { sdk, socket } = connect()
    let observer: Observer | undefined
    sdk
      .registerFunction('inventory::on-stock-changed', (e: unknown) => e)
      .onObserverAdded((obs) => {
        observer = obs
      })
    socket.deliver(
      fake.functionRegistered('inventory::on-stock-changed', [
        fake.observerInfo(activation.observer_id as string, expected.function_id as string),
      ]),
    )
    await fake.settle()
    await observer?.accepted()
    socket.clear()
    await sdk.call({ observer: observer as Observer, payload: expected.data, action: CallAction.Void() })
    expect(socket.framesOf('invokefunction')).toEqual([expected])
  })

  it('invokefunction-legacy.json (a pre-0.25 call keeps decoding, and re-encodes unchanged)', () => {
    const expected = fixture('invokefunction-legacy.json')
    const decoded = wire.decodeInvokeFunction(body(expected))
    expect(decoded.caller).toBeUndefined()
    expect(decoded.observe_depth).toBeUndefined()
    expect(decoded.activation).toBeUndefined()
    expect({ type: 'invokefunction', ...wire.buildInvokeFunction(decoded) }).toEqual(expected)
  })

  it('invokefunction-workers-register.json (features in engine::workers::register)', () => {
    const expected = fixture('invokefunction-workers-register.json')
    sdk = registerWorker('ws://fixtures.test', {
      reconnectionConfig: { maxRetries: 0 },
      workerName: 'billing',
      namespace: 'default',
    })
    const socket = fake.lastSocket()
    socket.open()
    const [frame] = socket
      .framesOf('invokefunction')
      .filter((f) => f.function_id === 'engine::workers::register') as Json[]
    const data = frame.data as Json
    const want = expected.data as Json
    // The frame is the fixture's; its payload is a subset of the fixture's
    // fields (a browser has no version, pid or telemetry), and `os` is the
    // user agent.
    expect({ ...frame, data: null }).toEqual({ ...expected, data: null })
    expect(Object.keys(data).filter((k) => !(k in want))).toEqual([])
    expect(data).toMatchObject({ runtime: 'browser', name: 'billing', namespace: 'default' })
    // The fixture is a native SDK's. A browser receives one 16 MiB frame at
    // most, so it declares every feature but `large-messages.v1`.
    expect(want.features).toEqual(['observe.v1', 'observe.v2', 'local-dispatch.v1', 'large-messages.v1'])
    expect(data.features).toEqual(['observe.v1', 'observe.v2', 'local-dispatch.v1'])
    expect(typeof data.os).toBe('string')
  })
})

describe('engine → worker: the SDK decodes without loss', () => {
  const decoders: Record<string, (v: Json) => unknown> = {
    'workerregistered.json': wire.decodeWorkerRegistered,
    'workerregistered-v1.json': wire.decodeWorkerRegistered,
    'workerregistered-legacy.json': wire.decodeWorkerRegistered,
    'registrationrejected-unsupported.json': wire.decodeRegistrationRejected,
    'registrationrejected-invalid-args-format.json': wire.decodeRegistrationRejected,
    'invocationresult.json': (v) => wire.buildInvocationResult(wire.decodeInvocationResult(v) as never),
    'invocationresult-not-function-owner.json': (v) =>
      wire.buildInvocationResult(wire.decodeInvocationResult(v) as never),
    'observeresult.json': wire.decodeObserveResult,
    'observeresult-error.json': wire.decodeObserveResult,
    'observerstatechanged.json': wire.decodeObserverStateChanged,
    'observerstatechanged-reason.json': wire.decodeObserverStateChanged,
    'functionregistered.json': wire.decodeFunctionRegistered,
    'functionregistered-empty.json': wire.decodeFunctionRegistered,
    'observeradded.json': wire.decodeObserverAdded,
    'observerremoved.json': wire.decodeObserverRemoved,
    'invokefunction-activation.json': (v) => wire.buildInvokeFunction(wire.decodeInvokeFunction(v)),
    'invokefunction-activation-engine-source.json': (v) => wire.buildInvokeFunction(wire.decodeInvokeFunction(v)),
    'invokefunction-caller.json': (v) => wire.buildInvokeFunction(wire.decodeInvokeFunction(v)),
    'invokefunction-observer-call-forwarded.json': (v) => wire.buildInvokeFunction(wire.decodeInvokeFunction(v)),
  }

  it('decodes exactly the engine → worker fixtures listed above', () => {
    expect(Object.keys(decoders).sort()).toEqual([...HANDLED.engineToWorker].sort())
  })

  it.each(Object.keys(decoders))('%s', (name) => {
    const expected = fixture(name)
    expect({ type: expected.type, ...(asJson(decoders[name](body(expected))) as Json) }).toEqual(expected)
  })

  it('accepts null for optional fields and keeps unknown enum values', () => {
    expect(wire.decodeObserveResult({ id: 'a', state: 'hibernating', error: null, warnings: null })).toEqual({
      id: 'a',
      state: 'hibernating',
    })
    expect(wire.decodeObserverStateChanged({ id: 'a', state: 'active', reason: null, fires: 3 })).toEqual({
      id: 'a',
      state: 'active',
      fires: 3,
    })
    expect(wire.decodeWorkerRegistered({ worker_id: 'w', reattach_token: null, features: null })).toEqual({
      worker_id: 'w',
    })
    expect(
      wire.decodeRegistrationRejected({
        code: 'UNSUPPORTED',
        namespace: 'default',
        owner_worker_id: '',
        worker_name: null,
        function_id: null,
      }),
    ).toEqual({ code: 'UNSUPPORTED', namespace: 'default', owner_worker_id: '' })
    // `result: null` is a value; `observed: []` and a malformed `observed` read as absent
    expect(
      wire.decodeInvocationResult({ invocation_id: 'i', function_id: 'f', result: null, error: null, observed: [] }),
    ).toEqual({ invocation_id: 'i', function_id: 'f', result: null })
    expect(wire.decodeInvocationResult({ invocation_id: 'i', function_id: 'f', observed: 'o-1' })).toEqual({
      invocation_id: 'i',
      function_id: 'f',
    })
    const info = wire.decodeObserverInfo({
      id: 'o',
      args: null,
      filters: null,
      target: 't',
      capture: ['input', 'telemetry'],
      on: 'start',
    })
    // `filters: null` is kept as sent: compiling it fails (INVALID_FILTER), it is never read as match-all
    expect(info).toEqual({
      id: 'o',
      args: {},
      filters: null,
      target: 't',
      capture: ['input', 'telemetry'],
      on: 'start',
    })
    expect(wire.decodeObserverInfo({ id: 'o', target: 't' })).toMatchObject({
      filters: {},
      capture: ['input', 'output', 'caller'],
      on: 'success',
    })
    const invoke = wire.decodeInvokeFunction({
      function_id: 'f',
      data: 1,
      caller: { worker_id: 'w', parent: { worker_id: 'p', parent: { worker_id: 'gp' } } },
      observe_depth: null,
      activation: null,
    })
    expect(invoke).toEqual({ function_id: 'f', data: 1, caller: { worker_id: 'w', parent: { worker_id: 'p' } } })
  })
})

describe('fixture coverage', () => {
  it('every fixture file is exercised above', () => {
    const files = readdirSync(DIR).filter((f) => f.endsWith('.json'))
    const handled = new Set([...HANDLED.workerToEngine, ...HANDLED.engineToWorker, ...HANDLED.engineOnly])
    expect(files.filter((f) => !handled.has(f))).toEqual([])
    expect([...handled].filter((f) => !files.includes(f))).toEqual([])
  })
})

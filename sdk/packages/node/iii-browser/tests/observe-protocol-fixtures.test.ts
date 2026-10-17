// Derived from sdk/packages/node/iii/tests/observe-protocol-fixtures.test.ts by scripts/sync-browser-observe.py; edit those, not this file.
import { readdirSync, readFileSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { afterEach, describe, expect, it } from 'vitest'
import { ObserveRejection } from '../src/errors'
import { registerWorker } from '../src/index'
import * as wire from '../src/observe-wire'
import { buildLocalInvocationStats, encodeReportItem } from '../src/reporter'
import type { ISdk } from '../src/types'
import * as fake from './observe-fake-engine'

/**
 * The observe wire contract (`sdk/fixtures/protocol/observe`, CONTRACT D5):
 * every worker → engine message the SDK builds equals its fixture as a JSON
 * value given the same inputs, and every engine → worker message decodes
 * without loss.
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

/**
 * Every fixture this file tests; the coverage test below fails when a new
 * fixture file is not listed (and handled) here.
 */
const HANDLED = {
  workerToEngine: [
    'observe.json',
    'observe-minimal.json',
    'observe-auto.json',
    'unobserve.json',
    'observerack.json',
    'observerack-error.json',
    'invocationsobserved.json',
    'localinvocationstats.json',
    'registerfunction.json',
    'registerfunction-owner.json',
    'registerfunction-not-observable.json',
    'invokefunction-owner-call.json',
    'invokefunction-owner-call-namespace.json',
    'invokefunction-legacy.json',
    'invokefunction-workers-register.json',
    'unregistertriggertype.json',
  ],
  engineToWorker: [
    'workerregistered.json',
    'workerregistered-legacy.json',
    'observeresult.json',
    'observeresult-error.json',
    'observerstatechanged.json',
    'observerstatechanged-reason.json',
    'functionregistered.json',
    'functionregistered-empty.json',
    'observeradded.json',
    'observeradded-legacy.json',
    'observerremoved.json',
    'invokefunction-activation.json',
    'invokefunction-caller.json',
  ],
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
        source: { function_id: 'orders::archive' },
        target: 'search::reindex-order',
        options: { id: 'd94f3a5b-6c7d-4e8f-a01b-2c3d4e5f6a7b' },
      },
    ],
    [
      'observe-auto.json',
      {
        source: {
          function_id: 'payments::charge',
          filters: { 'input.amount': { $gte: 500 } },
          capture: ['input', 'error', 'caller'],
          on: 'error',
        },
        target: 'alerts::payment-failed',
        options: { id: 'c81e2f4a-5b6c-4d7e-9f0a-1b2c3d4e5f60', max_in_flight: 4 },
      },
    ],
  ] as const)('%s (iii.observe)', (name, input) => {
    const { sdk, socket } = connect()
    sdk.observe(input.source as never, input.target as never, input.options as never)
    expect(socket.framesOf('observe')).toEqual([fixture(name)])
  })

  it('unobserve.json (ObserverHandle.dispose)', async () => {
    const { sdk, socket } = connect()
    const handle = sdk.observe({ function_id: 'orders::archive' }, 'search::reindex-order', {
      id: '5b0c3e1a-8f2d-4c6b-9a7e-1d2f3a4b5c6d',
    })
    await handle.dispose()
    expect(socket.framesOf('unobserve')).toEqual([fixture('unobserve.json')])
  })

  it('observerack.json and observerack-error.json (the owner acks observeradded.json)', async () => {
    for (const [name, onObserverAdded] of [
      ['observerack.json', () => undefined],
      [
        'observerack-error.json',
        () => {
          throw new ObserveRejection('INVALID_CRON_EXPRESSION', 'invalid day-of-week field')
        },
      ],
    ] as const) {
      const { sdk, socket } = connect()
      sdk.registerFunction('cron::on-tick', async () => null, {
        observe: { delivery: 'owner', onObserverAdded },
      })
      socket.deliver(fake.functionRegistered('cron::on-tick', [], { revision: 12 }))
      socket.deliver(fixture('observeradded.json'))
      await fake.settle()
      expect(socket.framesOf('observerack')).toEqual([fixture(name)])
      await sdk.shutdown()
    }
  })

  it('invocationsobserved.json (report items)', () => {
    const expected = fixture('invocationsobserved.json')
    const [failed, nullOutput, emitted] = expected.items as Json[]
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
        values: { input: nullOutput.input, output: null, caller: nullOutput.caller as never, metadata: undefined },
        captures: ['input', 'output', 'caller', 'metadata'],
        started_at_ms: 1760350000180,
        duration_ms: 0.42,
        observe_depth: 1,
      }),
      encodeReportItem({
        function_id: 'database::on-row-changed',
        namespace: 'default',
        invocation_id: emitted.invocation_id as string,
        matched: emitted.matched as string[],
        status: 'success',
        values: { input: emitted.input, caller: emitted.caller as never },
        captures: ['input', 'caller'],
        started_at_ms: 1760350000201,
        duration_ms: 0.004,
        observe_depth: 0,
        traceparent: emitted.traceparent as string,
        baggage: emitted.baggage as string,
      }),
    ]
    const frame = JSON.parse(`{"type":"invocationsobserved","items":[${items.join(',')}]}`)
    expect(frame).toEqual(expected)
  })

  it('localinvocationstats.json', () => {
    const stats = new Map([
      ['database::on-row-changed', { count: 48211, errors: 0, duration_ms_sum: 190.2 }],
      ['orders::archive', { count: 37, errors: 2, duration_ms_sum: 15.5 }],
    ])
    const drops = new Map([['database::on-row-changed', new Map([['BUFFER_FULL', 12]])]])
    expect({ type: 'localinvocationstats', ...buildLocalInvocationStats(10000, stats, drops) }).toEqual(
      fixture('localinvocationstats.json'),
    )
  })

  it.each([
    ['registerfunction.json', undefined, undefined],
    [
      'registerfunction-owner.json',
      {
        delivery: 'owner',
        args_format: {
          type: 'object',
          additionalProperties: false,
          required: ['expression'],
          properties: { expression: { type: 'string' } },
        },
        legacy_trigger_type: 'cron',
        allow: true,
        onObserverAdded: () => undefined,
        onObserverRemoved: () => undefined,
      },
      false,
    ],
    ['registerfunction-not-observable.json', { delivery: 'auto', allow: false }, undefined],
  ] as const)('%s (registerFunction; hooks and localDispatch never travel)', (name, observe, localDispatch) => {
    const expected = fixture(name)
    const { sdk, socket } = connect()
    sdk.registerFunction(expected.id as string, async () => null, {
      description: expected.description as string,
      request_format: expected.request_format as never,
      response_format: expected.response_format as never,
      observe: observe as never,
      localDispatch,
    })
    expect(socket.framesOf('registerfunction')).toEqual([expected])
  })

  it('invokefunction-owner-call.json (obs.call: activation, caller, no namespace, depth 0 omitted)', () => {
    const expected = fixture('invokefunction-owner-call.json')
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
      activation: { observer_id: '4f2a9b1c-7d3e-4f6a-8b5c-2e1d0f9a8b7c' },
    })
    expect({ type: 'invokefunction', ...built }).toEqual(expected)
  })

  it('invokefunction-owner-call-namespace.json (decodes; this SDK sends no namespace on obs.call, CONTRACT D13)', () => {
    const expected = fixture('invokefunction-owner-call-namespace.json')
    expect({ type: 'invokefunction', ...wire.buildInvokeFunction(wire.decodeInvokeFunction(body(expected))) }).toEqual(
      expected,
    )
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
    expect(data).toMatchObject({ runtime: 'browser', name: 'billing', namespace: 'default', features: want.features })
    expect(typeof data.os).toBe('string')
  })

  it('unregistertriggertype.json', () => {
    const { sdk, socket } = connect()
    sdk.unregisterTriggerType({ id: 'webhook', description: 'Webhooks' })
    expect(socket.framesOf('unregistertriggertype')).toEqual([fixture('unregistertriggertype.json')])
  })
})

describe('engine → worker: the SDK decodes without loss', () => {
  const decoders: Record<string, (v: Json) => unknown> = {
    'workerregistered.json': wire.decodeWorkerRegistered,
    'workerregistered-legacy.json': wire.decodeWorkerRegistered,
    'observeresult.json': wire.decodeObserveResult,
    'observeresult-error.json': wire.decodeObserveResult,
    'observerstatechanged.json': wire.decodeObserverStateChanged,
    'observerstatechanged-reason.json': wire.decodeObserverStateChanged,
    'functionregistered.json': wire.decodeFunctionRegistered,
    'functionregistered-empty.json': wire.decodeFunctionRegistered,
    'observeradded.json': wire.decodeObserverAdded,
    'observeradded-legacy.json': wire.decodeObserverAdded,
    'observerremoved.json': wire.decodeObserverRemoved,
    'invokefunction-activation.json': (v) => wire.buildInvokeFunction(wire.decodeInvokeFunction(v)),
    'invokefunction-caller.json': (v) => wire.buildInvokeFunction(wire.decodeInvokeFunction(v)),
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
    const info = wire.decodeObserverInfo({
      id: 'o',
      args: null,
      filters: null,
      target: 't',
      capture: ['input', 'telemetry'],
      on: 'start',
      legacy: null,
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
    const handled = new Set([...HANDLED.workerToEngine, ...HANDLED.engineToWorker])
    expect(files.filter((f) => !handled.has(f))).toEqual([])
    expect([...handled].filter((f) => !files.includes(f))).toEqual([])
  })
})

/**
 * A fake engine for the observe unit tests: a stand-in for the `ws`
 * WebSocket that records every frame the SDK sends and lets a test push
 * engine frames. Install it in a test file with:
 *
 * ```ts
 * vi.mock('ws', async () => (await import('./observe-fake-engine')).wsModule())
 * vi.resetModules()
 * const { registerWorker } = await import('../src/index')
 * ```
 *
 * The sockets live on `globalThis`, so the module instance the mock factory
 * loads and the one the test imports share them.
 */
import { EventEmitter } from 'node:events'
import { vi } from 'vitest'

const REGISTRY = Symbol.for('iii.tests.fakeEngineSockets')
type Registry = { sockets: FakeSocket[] }
const registry: Registry = ((globalThis as Record<symbol, unknown>)[REGISTRY] as Registry | undefined) ?? {
  sockets: [],
}
;(globalThis as Record<symbol, unknown>)[REGISTRY] = registry

export const ENGINE_FEATURES = ['observe.v1', 'observe.v2', 'local-dispatch.v1']
/** What a 0.25 engine advertises: observe without `observe.v2`. */
export const ENGINE_FEATURES_V1 = ['observe.v1', 'local-dispatch.v1']

export type Frame = Record<string, unknown>

export class FakeSocket extends EventEmitter {
  static readonly CONNECTING = 0
  static readonly OPEN = 1
  static readonly CLOSING = 2
  static readonly CLOSED = 3

  readyState: number = FakeSocket.CONNECTING
  readonly sent: string[] = []
  /** What `ws` reports as handed over but not written; a test sets it to model a peer that stops reading. */
  bufferedAmount = 0
  terminated = false

  constructor(
    public readonly url?: string,
    public readonly opts?: Record<string, unknown>,
  ) {
    super()
    registry.sockets.push(this)
  }

  send(data: Buffer | string, callback?: (err?: Error | null) => void): void {
    this.sent.push(String(data))
    callback?.()
  }

  ping(): void {
    // the fake engine never answers pings
  }

  close(): void {
    this.readyState = FakeSocket.CLOSED
  }

  terminate(): void {
    if (this.terminated) return
    this.terminated = true
    this.readyState = FakeSocket.CLOSED
    this.emit('close')
  }

  /**
   * Opens the socket and answers with `workerregistered`: with `features`, or
   * without any (`'legacy'`, a pre-0.25 engine), or not at all (`null`).
   */
  open(features: string[] | 'legacy' | null = ENGINE_FEATURES, workerId = 'worker-1'): void {
    this.readyState = FakeSocket.OPEN
    this.emit('open')
    if (features !== null) {
      this.deliver({
        type: 'workerregistered',
        worker_id: workerId,
        reattach_token: 'token-1',
        ...(features !== 'legacy' ? { features } : {}),
      })
    }
  }

  /** Delivers one engine → worker frame. */
  deliver(frame: Frame): void {
    this.emit('message', JSON.stringify(frame))
  }

  /** Drops the connection the way the real socket does. */
  drop(): void {
    this.terminate()
  }

  frames(): Frame[] {
    return this.sent.map((s) => JSON.parse(s) as Frame)
  }

  framesOf(type: string): Frame[] {
    return this.frames().filter((f) => f.type === type)
  }

  /** Forgets what was sent so far. */
  clear(): void {
    this.sent.length = 0
  }
}

/** What `vi.mock('ws', ...)` should return. */
export function wsModule() {
  return { WebSocket: FakeSocket, default: { WebSocket: FakeSocket } }
}

export function sockets(): FakeSocket[] {
  return registry.sockets
}

export function lastSocket(): FakeSocket {
  const all = registry.sockets
  const last = all[all.length - 1]
  if (!last) throw new Error('no socket was opened')
  return last
}

/**
 * Fakes the timers the SDK schedules (linger, stats, timeouts, hook
 * deadlines) and `Date`, but not `setImmediate`, which {@link settle} uses.
 */
export function fakeClock(): void {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval', 'Date'] })
}

/** Advances the fake clock by `ms`, running what comes due and the promise chains it starts. */
export async function elapse(ms: number): Promise<void> {
  await vi.advanceTimersByTimeAsync(ms)
}

/** Lets queued microtasks and pending hook chains run. */
export async function settle(rounds = 10): Promise<void> {
  for (let i = 0; i < rounds; i++) await new Promise<void>((resolve) => setImmediate(resolve))
}

/** `functionregistered` for one function. */
export function functionRegistered(
  id: string,
  observers: Frame[] = [],
  opts: { revision?: number; local_dispatch?: boolean; namespace?: string } = {},
): Frame {
  return {
    type: 'functionregistered',
    id,
    namespace: opts.namespace ?? 'default',
    revision: opts.revision ?? 0,
    local_dispatch: opts.local_dispatch ?? true,
    observers,
  }
}

/** An observer as the engine pushes it to its function's host. */
export function observerInfo(
  id: string,
  target: string,
  opts: { args?: Frame; filters?: Frame; capture?: string[]; on?: string } = {},
): Frame {
  return {
    id,
    args: opts.args ?? {},
    filters: opts.filters ?? {},
    target,
    capture: opts.capture ?? ['input', 'output', 'caller'],
    on: opts.on ?? 'success',
  }
}

export function observerAdded(functionId: string, revision: number, observer: Frame, namespace = 'default'): Frame {
  return { type: 'observeradded', function_id: functionId, namespace, revision, observer }
}

export function observerRemoved(functionId: string, revision: number, id: string, namespace = 'default'): Frame {
  return { type: 'observerremoved', function_id: functionId, namespace, revision, id }
}

/** The items of every `invocationsobserved` frame sent so far. */
export function reportedItems(socket: FakeSocket): Frame[] {
  return socket.framesOf('invocationsobserved').flatMap((f) => f.items as Frame[])
}

/** A routed call of a function the worker hosts, as the engine forwards it. */
export function invokeFunction(
  functionId: string,
  data: unknown,
  opts: {
    invocation_id?: string
    traceparent?: string
    caller?: Frame
    metadata?: Frame
    observe_depth?: number
    activation?: Frame
  } = {},
): Frame {
  return { type: 'invokefunction', function_id: functionId, data, ...opts }
}

/** The `invocationresult` the worker answered `invocationId` with. */
export function resultOf(socket: FakeSocket, invocationId: string): Frame | undefined {
  return socket.framesOf('invocationresult').find((f) => f.invocation_id === invocationId)
}

/**
 * A fake engine for the observe unit tests: a stand-in for the browser
 * `WebSocket` that records every frame the SDK sends and lets a test push
 * engine frames. Importing this module installs it before every test of the
 * importing file (the test setup restores the real `WebSocket` after each
 * test); {@link installFakeEngine} installs it again by hand.
 */
import { beforeEach, vi } from 'vitest'
import type { IIIConnectionState } from '../src/iii-constants'
import type { ISdk } from '../src/types'
import { MockWebSocket } from './mock-websocket'

export const ENGINE_FEATURES = ['observe.v1', 'observe.v2', 'local-dispatch.v1']
/** What a 0.25 engine advertises: observe without `observe.v2`. */
export const ENGINE_FEATURES_V1 = ['observe.v1', 'local-dispatch.v1']

export type Frame = Record<string, unknown>

const opened: FakeSocket[] = []

export class FakeSocket extends MockWebSocket {
  /** What the SDK reads as `WebSocket.bufferedAmount`; tests raise it to simulate a peer that stops reading. */
  bufferedAmount = 0

  constructor(url: string | URL) {
    super(url)
    opened.push(this)
  }

  /** Every text frame the SDK sent on this socket. */
  get sent(): string[] {
    return this.sentMessages
  }

  /** Records the frame; the callback mirrors the Node fake's `send(data, cb)`. */
  override send(
    data: string | ArrayBufferLike | Blob | ArrayBufferView,
    callback?: (err?: Error | null) => void,
  ): void {
    super.send(data)
    callback?.()
  }

  /**
   * Opens the socket and answers with `workerregistered`: with `features`, or
   * without any (`'legacy'`, a pre-0.25 engine), or not at all (`null`).
   */
  open(features: string[] | 'legacy' | null = ENGINE_FEATURES, workerId = 'worker-1'): void {
    this.simulateOpen()
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
    this.simulateMessage(JSON.stringify(frame))
  }

  /** Drops the connection the way a network failure does; the SDK reconnects. */
  drop(): void {
    this.simulateClose(1006, 'dropped')
  }

  frames(): Frame[] {
    return this.sent.map((s) => JSON.parse(s) as Frame)
  }

  framesOf(type: string): Frame[] {
    return this.frames().filter((f) => f.type === type)
  }

  /** Forgets what was sent so far. */
  clear(): void {
    ;(this.sentRaw as unknown[]).length = 0
  }
}

/** Makes `new WebSocket(url)` create a {@link FakeSocket}. */
export function installFakeEngine(): void {
  opened.length = 0
  globalThis.WebSocket = FakeSocket as unknown as typeof WebSocket
}

beforeEach(() => installFakeEngine())

/** Every socket the SDK opened in this test. */
export function sockets(): FakeSocket[] {
  return opened
}

export function lastSocket(): FakeSocket {
  const last = opened[opened.length - 1]
  if (!last) throw new Error('no socket was opened')
  return last
}

/**
 * Fakes the timers the SDK uses (reconnect backoff, reporter linger and stats,
 * invocation timeouts) and `Date`, but not `setImmediate`, so {@link settle}
 * still lets pending promise chains run.
 */
export function fakeClock(): void {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval', 'Date'] })
}

/** Advances the fake clock, running what comes due. */
export async function elapse(ms: number): Promise<void> {
  await vi.advanceTimersByTimeAsync(ms)
}

/** Lets queued microtasks and pending hook chains run. */
export async function settle(rounds = 10): Promise<void> {
  for (let i = 0; i < rounds; i++) await new Promise<void>((resolve) => setImmediate(resolve))
}

export const sleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms))

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

/** Outbound `invokefunction` frames, without the `engine::workers::register` announce. */
export function invokes(socket: FakeSocket): Frame[] {
  return socket.framesOf('invokefunction').filter((f) => f.function_id !== 'engine::workers::register')
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

/** The worker's connection state (the browser client reports it to listeners only). */
export function connectionState(sdk: ISdk): IIIConnectionState {
  let state: IIIConnectionState = 'disconnected'
  sdk.addConnectionStateListener((s) => {
    state = s
  })()
  return state
}

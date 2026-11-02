/**
 * Helpers for the observe tests that run against a real engine (`III_URL`).
 */
import { registerWorker } from '../src/index'
import type { InvocationContext } from '../src/observe-types'
import type { IIIClient } from '../src/types'
import { engineWsUrl, uniqueWorkerName } from './utils'

const workers: IIIClient[] = []

/** A worker of its own, closed by {@link closeWorkers}. */
export function worker(role: string, opts: { namespace?: string } = {}): IIIClient {
  const sdk = registerWorker(engineWsUrl, {
    workerName: uniqueWorkerName(role),
    namespace: opts.namespace,
    reconnectionConfig: { maxRetries: 20, initialDelayMs: 100, maxDelayMs: 500, jitterFactor: 0 },
  })
  workers.push(sdk)
  return sdk
}

export async function closeWorkers(): Promise<void> {
  for (const sdk of workers.splice(0)) {
    try {
      await sdk.shutdown()
    } catch {
      // closing anyway
    }
  }
}

export async function waitFor<T>(get: () => T | undefined | false, what: string, timeoutMs = 5000): Promise<T> {
  const deadline = Date.now() + timeoutMs
  for (;;) {
    const value = get()
    if (value !== undefined && value !== false) return value
    if (Date.now() > deadline) throw new Error(`timed out after ${timeoutMs}ms waiting for ${what}`)
    await new Promise((r) => setTimeout(r, 10))
  }
}

export const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms))

export type Activation = { data: Record<string, unknown>; metadata: unknown; inv?: InvocationContext }

/** Registers a target that records every activation it receives. */
export function collector(sdk: IIIClient, functionId: string): Activation[] {
  const got: Activation[] = []
  sdk.registerFunction(functionId, async (data: Record<string, unknown>, metadata: unknown) => {
    got.push({ data, metadata, inv: sdk.currentInvocation() })
    return null
  })
  return got
}

/** Records every frame the worker sends (all of them go through `sendMessageRaw`). */
export function frameCounter(sdk: IIIClient): { count: number; types: string[]; frames: Record<string, unknown>[] } {
  const counter = { count: 0, types: [] as string[], frames: [] as Record<string, unknown>[] }
  const internal = sdk as unknown as { sendMessageRaw(data: string): void }
  const send = internal.sendMessageRaw.bind(internal)
  internal.sendMessageRaw = (data: string) => {
    const frame = JSON.parse(data) as Record<string, unknown>
    counter.count++
    counter.types.push(String(frame.type))
    counter.frames.push(frame)
    send(data)
  }
  return counter
}

type FunctionState = { confirmed: boolean; localDispatch: boolean }

/** The engine confirmed the function for local dispatch (`functionregistered`, M3). */
export async function localDispatchReady(sdk: IIIClient, functionId: string, timeoutMs = 5000): Promise<void> {
  const functions = (sdk as unknown as { functions: Map<string, { observe?: FunctionState }> }).functions
  await waitFor(
    () => functions.get(functionId)?.observe?.localDispatch === true,
    `functionregistered { local_dispatch: true } for ${functionId}`,
    timeoutMs,
  )
}

/** Drops the worker's socket the way a network failure does; the SDK reconnects. */
export function dropConnection(sdk: IIIClient): void {
  ;(sdk as unknown as { ws?: { terminate(): void } }).ws?.terminate()
}

/** The envelope fields that differ between two runs of the same invocation. */
export function stable(envelope: Record<string, unknown>): Record<string, unknown> {
  const { activation_id: _a, invocation_id: _i, started_at_ms: _s, duration_ms: _d, error, caller, ...rest } = envelope
  const out: Record<string, unknown> = { ...rest }
  if (error && typeof error === 'object') {
    const { stacktrace: _st, ...body } = error as Record<string, unknown>
    out.error = body
  }
  if (caller && typeof caller === 'object') {
    const { invocation_id: _ci, ...c } = caller as Record<string, unknown>
    out.caller = c
  }
  return out
}

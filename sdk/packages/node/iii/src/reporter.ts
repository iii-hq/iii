/**
 * The reporter (sdk.md §4.4, CONTRACT D12): one bounded buffer for matched
 * local invocations of auto functions and `obs.emit` events of owner
 * functions, flushed as `invocationsobserved` batches; plus the periodic
 * `localinvocationstats`.
 *
 * It never blocks a call: when the buffer is full the newest item is dropped
 * and counted (`BUFFER_FULL`), and an item too large on its own is dropped and
 * counted (`TOO_LARGE`). The 8 MiB bound covers both the buffer and the bytes
 * already handed to the socket but not yet written (a peer that stops reading
 * does not grow memory). Items stay buffered while disconnected and are
 * flushed once the engine confirmed their function on the new connection.
 */
import type { CallerWire, LocalInvocationStatsMessage } from './iii-types'
import { MessageType } from './iii-types'

export type ReportPart = 'input' | 'output' | 'error' | 'caller' | 'metadata'

const PART_ORDER: readonly ReportPart[] = ['input', 'output', 'error', 'caller', 'metadata']

/** The values an item may carry; a part is sent only when some matched observer captures it. */
export type ReportValues = {
  input?: unknown
  output?: unknown
  error?: unknown
  caller?: CallerWire
  metadata?: unknown
}

export type ReportInit = {
  function_id: string
  namespace?: string
  invocation_id: string
  matched: string[]
  status: 'success' | 'error'
  values: ReportValues
  captures: Iterable<string>
  started_at_ms: number
  duration_ms: number
  observe_depth: number
  traceparent?: string
  baggage?: string
}

export type ReporterLimits = {
  /** Flush at this many items (default 256). */
  batchItems: number
  /** Flush this long after the first buffered item (default 5 ms). */
  lingerMs: number
  /** Bounded buffer: items (default 10 000). */
  maxItems: number
  /** Bounded buffer: bytes (default 8 MiB); an item larger than this is `TOO_LARGE`. */
  maxBytes: number
  /** One frame's limit (default 16 MiB, the engine's). */
  maxFrameBytes: number
  /** `localinvocationstats` cadence (default 10 s). */
  statsIntervalMs: number
}

export const DEFAULT_REPORTER_LIMITS: ReporterLimits = {
  batchItems: 256,
  lingerMs: 5,
  maxItems: 10_000,
  maxBytes: 8 * 1024 * 1024,
  maxFrameBytes: 16 * 1024 * 1024,
  statsIntervalMs: 10_000,
}

export interface ReporterHost {
  /** Connected to an engine that advertised `observe.v1`. */
  online(): boolean
  /**
   * `ready`: the engine confirmed the function (`functionregistered`) on the
   * current connection; `wait`: not yet; `gone`: no longer registered here.
   */
  status(functionId: string): 'ready' | 'wait' | 'gone'
  /** Sends one JSON frame; false when the socket is not open. */
  sendFrame(data: string): boolean
  /** Bytes handed to the socket and not written yet (`ws.bufferedAmount`). */
  pendingBytes(): number
  logError(message: string, error?: unknown): void
}

type ItemState = { matched: number; captured: string[]; parts: [ReportPart, string][]; json: string; bytes: number }

class ReportItem {
  readonly parts = new Map<ReportPart, string>()
  readonly captured = new Set<string>()
  json = ''
  bytes = 0

  constructor(
    readonly init: Omit<ReportInit, 'captures'>,
    /** An `obs.emit` item: later emits of the same event value (`coalesceKey`) from the same `context` merge into it. */
    readonly emit: boolean,
    readonly coalesceKey: unknown,
    readonly context: unknown,
  ) {}

  /** Adds the parts `captures` asks for that are not in the item yet. */
  capture(captures: Iterable<string>): void {
    for (const c of captures) {
      if (this.captured.has(c)) continue
      this.captured.add(c)
      if (!PART_ORDER.includes(c as ReportPart)) continue
      const part = c as ReportPart
      if (!(part in this.init.values)) continue
      const encoded = encodeJson(this.init.values[part])
      if (encoded !== undefined) this.parts.set(part, encoded)
    }
  }

  /** What a merge can change, to undo it. */
  save(): ItemState {
    return {
      matched: this.init.matched.length,
      captured: [...this.captured],
      parts: [...this.parts],
      json: this.json,
      bytes: this.bytes,
    }
  }

  restore(state: ItemState): void {
    this.init.matched.length = state.matched
    this.captured.clear()
    for (const c of state.captured) this.captured.add(c)
    this.parts.clear()
    for (const [part, encoded] of state.parts) this.parts.set(part, encoded)
    this.json = state.json
    this.bytes = state.bytes
  }

  /** Assembles the wire JSON from the pre-encoded parts. */
  render(): void {
    const i = this.init
    let s = `{"function_id":${JSON.stringify(i.function_id)}`
    if (i.namespace !== undefined) s += `,"namespace":${JSON.stringify(i.namespace)}`
    s += `,"invocation_id":${JSON.stringify(i.invocation_id)},"matched":${JSON.stringify(i.matched)}`
    s += `,"status":${JSON.stringify(i.status)}`
    for (const part of PART_ORDER) {
      const encoded = this.parts.get(part)
      if (encoded !== undefined) s += `,"${part}":${encoded}`
    }
    s += `,"started_at_ms":${finite(i.started_at_ms)},"duration_ms":${finite(i.duration_ms)}`
    s += `,"observe_depth":${finite(i.observe_depth)}`
    if (i.traceparent !== undefined) s += `,"traceparent":${JSON.stringify(i.traceparent)}`
    if (i.baggage !== undefined) s += `,"baggage":${JSON.stringify(i.baggage)}`
    this.json = wellFormed(`${s}}`)
    this.bytes = byteLength(this.json)
  }
}

/**
 * Replaces lone surrogates with U+FFFD (CONTRACT parity 32): `JSON.stringify`
 * writes one as `\ud800`, which the engine's strict decoder refuses, and it
 * would take the whole batch down with it. `JSON.stringify` escapes nothing
 * else in that range (a valid pair is written as is), so every `\udXXX`
 * escape it produced is a lone surrogate. Fast path: no `\ud` at all.
 */
export function wellFormed(json: string): string {
  if (!json.includes('\\ud')) return json
  // An escape is a backslash preceded by an even run of backslashes (`\\ud800` is text).
  return json.replace(/(\\+)u(d[89a-f][0-9a-f]{2})/g, (match, slashes: string) =>
    slashes.length % 2 === 1 ? `${slashes.slice(0, -1)}\\ufffd` : match,
  )
}

function finite(n: number): number {
  return Number.isFinite(n) ? n : 0
}

function byteLength(s: string): number {
  return Buffer.byteLength(s, 'utf8')
}

/** JSON text of a value as the remote path would send it; `BigInt` is written as its number. */
function encodeJson(value: unknown): string | undefined {
  try {
    return JSON.stringify(value)
  } catch (error) {
    if (!(error instanceof TypeError)) throw error
    return JSON.stringify(value, (_k, v) => (typeof v === 'bigint' ? Number(v) : v))
  }
}

const FRAME_HEAD = `{"type":"${MessageType.InvocationsObserved}","items":[`
const FRAME_TAIL = ']}'
const FRAME_OVERHEAD = byteLength(FRAME_HEAD) + byteLength(FRAME_TAIL)

type FunctionStats = { count: number; errors: number; duration_ms_sum: number }

export class Reporter {
  private items: ReportItem[] = []
  private bytes = 0
  /** Buffered items per function: whether a flush has anything to send is one look per function. */
  private perFunction = new Map<string, number>()
  private lingerTimer?: ReturnType<typeof setTimeout>
  private statsTimer?: ReturnType<typeof setInterval>
  /** Emitted events still in the buffer, by event value: one event to N observers is one item. */
  private openEmits = new Map<unknown, ReportItem[]>()
  private stats = new Map<string, FunctionStats>()
  private drops = new Map<string, Map<string, number>>()
  private windowStart = Date.now()
  private unencodable = new Set<string>()
  readonly limits: ReporterLimits

  constructor(
    private readonly host: ReporterHost,
    limits?: Partial<ReporterLimits>,
  ) {
    this.limits = { ...DEFAULT_REPORTER_LIMITS, ...limits }
  }

  /** Starts the `localinvocationstats` timer. */
  start(): void {
    if (this.statsTimer) return
    this.windowStart = Date.now()
    this.statsTimer = setInterval(() => this.sendStats(), this.limits.statsIntervalMs)
    this.statsTimer.unref?.()
  }

  stop(): void {
    if (this.statsTimer) clearInterval(this.statsTimer)
    this.statsTimer = undefined
    if (this.lingerTimer) clearTimeout(this.lingerTimer)
    this.lingerTimer = undefined
  }

  /** Buffered items (for tests and diagnostics). */
  get size(): number {
    return this.items.length
  }

  /** A matched local invocation of an auto function. */
  push(init: ReportInit): boolean {
    const { captures, ...rest } = init
    const item = new ReportItem(rest, false, undefined, undefined)
    if (!this.build(item, captures)) return false
    return this.enqueue(item)
  }

  /**
   * An `obs.emit` event. The same event value emitted to several observers
   * from the same invocation context, before the buffer is flushed, becomes
   * one item whose `matched` lists them all.
   */
  pushEmit(init: Omit<ReportInit, 'matched'> & { observer_id: string; event: unknown; context: unknown }): boolean {
    const { observer_id, event, context, captures, ...rest } = init
    const open = this.openEmits.get(event)
    const merge = open?.find(
      (it) =>
        it.init.function_id === rest.function_id && it.context === context && !it.init.matched.includes(observer_id),
    )
    if (merge) return this.merge(merge, observer_id, captures)
    const item = new ReportItem({ ...rest, matched: [observer_id] }, true, event, context)
    if (!this.build(item, captures)) return false
    return this.enqueue(item)
  }

  /** Makes a buffered emit item a merge target. Only items in the buffer are: a flush re-indexes. */
  private index(item: ReportItem): void {
    if (!item.emit) return
    const list = this.openEmits.get(item.coalesceKey)
    if (list) list.push(item)
    else this.openEmits.set(item.coalesceKey, [item])
  }

  /** Adds an observer (and its captures) to a buffered emit item, within the same bounds as a new item. */
  private merge(item: ReportItem, observerId: string, captures: Iterable<string>): boolean {
    const fn = item.init.function_id
    const before = item.save()
    try {
      item.init.matched.push(observerId)
      item.capture(captures)
      item.render()
    } catch (error) {
      item.restore(before)
      this.logUnencodable(fn, error)
      return false
    }
    const growth = item.bytes - before.bytes
    const reason = this.tooLarge(item)
      ? 'TOO_LARGE'
      : growth > 0 && this.bytes + this.host.pendingBytes() + growth > this.limits.maxBytes
        ? 'BUFFER_FULL'
        : undefined
    if (reason) {
      item.restore(before)
      this.drop(fn, reason)
      return false
    }
    this.bytes += growth
    return true
  }

  private build(item: ReportItem, captures: Iterable<string>): boolean {
    try {
      item.capture(captures)
      item.render()
      return true
    } catch (error) {
      this.logUnencodable(item.init.function_id, error)
      return false
    }
  }

  private logUnencodable(functionId: string, error: unknown): void {
    if (this.unencodable.has(functionId)) return
    this.unencodable.add(functionId)
    this.host.logError(`Cannot encode an observed invocation of ${functionId} as JSON; it is not reported`, error)
  }

  /**
   * TOO_LARGE is measured against the buffer (8 MiB), the smaller of the two
   * limits: an item that can never be buffered. It also always fits a 16 MiB frame.
   */
  private tooLarge(item: ReportItem): boolean {
    return item.bytes > this.limits.maxBytes || item.bytes + FRAME_OVERHEAD > this.limits.maxFrameBytes
  }

  private enqueue(item: ReportItem): boolean {
    const fn = item.init.function_id
    if (this.tooLarge(item)) {
      this.drop(fn, 'TOO_LARGE')
      return false
    }
    if (
      this.items.length >= this.limits.maxItems ||
      this.bytes + this.host.pendingBytes() + item.bytes > this.limits.maxBytes
    ) {
      this.drop(fn, 'BUFFER_FULL')
      return false
    }
    this.items.push(item)
    this.bytes += item.bytes
    this.perFunction.set(fn, (this.perFunction.get(fn) ?? 0) + 1)
    // Indexed before a flush can send it: the flush then drops it from the index.
    this.index(item)
    if (this.items.length >= this.limits.batchItems) this.flush()
    else this.arm(this.limits.lingerMs)
    return true
  }

  private arm(ms: number): void {
    if (this.lingerTimer) return
    this.lingerTimer = setTimeout(() => {
      this.lingerTimer = undefined
      this.flush()
    }, ms)
    this.lingerTimer.unref?.()
  }

  /** Counts a report the SDK dropped (`BUFFER_FULL`, `TOO_LARGE`, `INACTIVE`). */
  drop(functionId: string, reason: string, count = 1): void {
    let byReason = this.drops.get(functionId)
    if (!byReason) {
      byReason = new Map()
      this.drops.set(functionId, byReason)
    }
    byReason.set(reason, (byReason.get(reason) ?? 0) + count)
  }

  /** Counts one local invocation (for `localinvocationstats`). */
  recordLocal(functionId: string, ok: boolean, durationMs: number): void {
    let s = this.stats.get(functionId)
    if (!s) {
      s = { count: 0, errors: 0, duration_ms_sum: 0 }
      this.stats.set(functionId, s)
    }
    s.count++
    if (!ok) s.errors++
    s.duration_ms_sum += durationMs
  }

  /**
   * Discards a function's buffered items, counted as `INACTIVE`: the
   * function is unregistered, or the engine refused it
   * (`FUNCTION_NAMESPACE_CONFLICT`), so they could never be confirmed.
   */
  discard(functionId: string): void {
    const count = this.perFunction.get(functionId)
    if (!count) return
    this.drop(functionId, 'INACTIVE', count)
    this.setItems(this.items.filter((it) => it.init.function_id !== functionId))
  }

  /**
   * Sends what can be sent: items whose function the engine confirmed on the
   * current connection, in frames under the frame limit, while the socket's
   * unwritten bytes stay within the buffer bound. The rest stays.
   */
  flush(): void {
    if (this.lingerTimer) {
      clearTimeout(this.lingerTimer)
      this.lingerTimer = undefined
    }
    if (this.items.length === 0 || !this.host.online()) return

    // One status per function, and nothing more to do while every one waits.
    const status = new Map<string, 'ready' | 'wait' | 'gone'>()
    for (const fn of this.perFunction.keys()) status.set(fn, this.host.status(fn))
    if (![...status.values()].some((s) => s !== 'wait')) return

    const ready: ReportItem[] = []
    const held: ReportItem[] = []
    for (const item of this.items) {
      const st = status.get(item.init.function_id)
      if (st === 'ready') ready.push(item)
      else if (st === 'wait') held.push(item)
      // The function was unregistered here: its observers are not ours to serve any more.
      else this.drop(item.init.function_id, 'INACTIVE')
    }
    this.setItems(held)

    let frame: ReportItem[] = []
    let frameBytes = FRAME_OVERHEAD
    let blocked = false
    const unsent: ReportItem[] = []
    const send = () => {
      if (frame.length === 0) return
      const data = `${FRAME_HEAD}${frame.map((it) => it.json).join(',')}${FRAME_TAIL}`
      // The socket is not draining (its unwritten bytes would pass the bound),
      // or it closed under us: the rest waits in the buffer, in order.
      const pending = blocked ? 0 : this.host.pendingBytes()
      if (blocked || (pending > 0 && pending + frameBytes > this.limits.maxBytes) || !this.host.sendFrame(data)) {
        blocked = true
        unsent.push(...frame)
      }
      frame = []
      frameBytes = FRAME_OVERHEAD
    }
    for (const item of ready) {
      // Cannot happen with the default limits (an item is at most the 8 MiB
      // buffer, `enqueue`), only with a frame limit set below the buffer's.
      if (item.bytes + FRAME_OVERHEAD > this.limits.maxFrameBytes) {
        this.drop(item.init.function_id, 'TOO_LARGE')
        continue
      }
      const separator = frame.length > 0 ? 1 : 0
      if (frameBytes + separator + item.bytes > this.limits.maxFrameBytes) send()
      frameBytes += (frame.length > 0 ? 1 : 0) + item.bytes
      frame.push(item)
    }
    send()
    if (unsent.length > 0) {
      this.requeue(unsent)
      // Try again once the socket had time to drain.
      if (this.host.online()) this.arm(this.limits.lingerMs * 20)
    }
  }

  /** Replaces the buffer, keeping the per-function counts and the emit coalescing index in step. */
  private setItems(items: ReportItem[]): void {
    this.items = items
    this.bytes = 0
    this.perFunction.clear()
    this.openEmits.clear()
    for (const item of items) {
      this.bytes += item.bytes
      const fn = item.init.function_id
      this.perFunction.set(fn, (this.perFunction.get(fn) ?? 0) + 1)
      this.index(item)
    }
  }

  /** Puts items back at the front of the buffer, within its bounds. */
  private requeue(items: ReportItem[]): void {
    const kept: ReportItem[] = []
    let bytes = this.bytes
    let count = this.items.length
    for (const item of items) {
      if (count >= this.limits.maxItems || bytes + item.bytes > this.limits.maxBytes) {
        this.drop(item.init.function_id, 'BUFFER_FULL')
        continue
      }
      kept.push(item)
      bytes += item.bytes
      count++
    }
    this.setItems([...kept, ...this.items])
  }

  /** Sends `localinvocationstats` when there is anything to report; keeps counting while offline. */
  sendStats(now = Date.now()): boolean {
    if (this.stats.size === 0 && this.drops.size === 0) {
      this.windowStart = now
      return false
    }
    if (!this.host.online()) return false
    const message = buildLocalInvocationStats(now - this.windowStart, this.stats, this.drops)
    if (!this.host.sendFrame(JSON.stringify({ type: MessageType.LocalInvocationStats, ...message }))) return false
    this.stats.clear()
    this.drops.clear()
    this.windowStart = now
    return true
  }

  /** Forgets everything buffered (the engine does not support observe). */
  clear(): void {
    this.setItems([])
  }
}

/** `localinvocationstats` from the counters, in insertion order. */
export function buildLocalInvocationStats(
  windowMs: number,
  stats: ReadonlyMap<string, FunctionStats>,
  drops: ReadonlyMap<string, ReadonlyMap<string, number>>,
): Omit<LocalInvocationStatsMessage, 'message_type'> {
  return {
    window_ms: Math.max(0, Math.round(windowMs)),
    functions: [...stats].map(([function_id, s]) => ({
      function_id,
      count: s.count,
      errors: s.errors,
      duration_ms_sum: s.duration_ms_sum,
    })),
    dropped_reports: [...drops].flatMap(([function_id, byReason]) =>
      [...byReason].map(([reason, count]) => ({ function_id, reason, count })),
    ),
  }
}

/** One item's wire JSON (exported for the fixture tests). */
export function encodeReportItem(init: ReportInit): string {
  const { captures, ...rest } = init
  const item = new ReportItem(rest, false, undefined, undefined)
  item.capture(captures)
  item.render()
  return item.json
}

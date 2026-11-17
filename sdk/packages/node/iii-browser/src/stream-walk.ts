/**
 * Walks over the values a call carries, for the streams in them: what the SDK
 * sends (stream values to replace with refs) and what it receives (refs to turn
 * into streams). Only arrays and plain objects (`Object.prototype` or `null`
 * prototype) are walked, so a `Date`, a `Map` or a class instance is left as it
 * is, with whatever it holds. A walk does not go back into a container it is
 * inside of (a cycle).
 */
import { InputStream, OutputStream, streamKind } from './call-streams'
import type { StreamChannelRef } from './iii-types'
import { isChannelRef } from './utils'

/** A value the walk stops at: a stream value, or a channel ref (`'ref'`). */
export type StreamSlotKind = 'input' | 'output' | 'source' | 'sink' | 'ref'

function isPlainObject(value: object): value is Record<string, unknown> {
  const proto = Object.getPrototypeOf(value)
  return proto === Object.prototype || proto === null
}

/** Sets `key` on `out` as an own data property: an own `__proto__` key too, which an assignment takes as the prototype. */
function setOwn(out: Record<string, unknown>, key: string, value: unknown): void {
  if (key === '__proto__')
    Object.defineProperty(out, key, { value, writable: true, enumerable: true, configurable: true })
  else out[key] = value
}

/**
 * `value` with every stream value and channel ref replaced by what `fn`
 * returns for it. Copy-on-write: a container without one is returned as it is.
 */
export function mapStreams(value: unknown, fn: (slot: unknown, kind: StreamSlotKind) => unknown): unknown {
  return mapWalk(value, fn, new Set())
}

/**
 * {@link mapStreams} under the containers in `path`. One of them reached again
 * (a cycle) is left as it is: its streams were found on the way in. A container
 * reached twice without a cycle is walked twice, so its streams count twice.
 */
function mapWalk(value: unknown, fn: (slot: unknown, kind: StreamSlotKind) => unknown, path: Set<object>): unknown {
  if (value === null || typeof value !== 'object') return value
  if (isChannelRef(value)) return fn(value, 'ref')
  const kind = streamKind(value)
  if (kind) return fn(value, kind)
  if (path.has(value)) return value
  if (Array.isArray(value)) {
    path.add(value)
    let out: unknown[] | undefined
    for (let i = 0; i < value.length; i++) {
      const mapped = mapWalk(value[i], fn, path)
      if (mapped !== value[i]) {
        out ??= value.slice()
        out[i] = mapped
      }
    }
    path.delete(value)
    return out ?? value
  }
  if (!isPlainObject(value)) return value
  path.add(value)
  let out: Record<string, unknown> | undefined
  for (const [k, v] of Object.entries(value)) {
    const mapped = mapWalk(v, fn, path)
    if (mapped !== v) {
      out ??= { ...value }
      setOwn(out, k, mapped)
    }
  }
  path.delete(value)
  return out ?? value
}

/** Whether `value` holds a stream value or a channel ref. */
export function containsStreams(value: unknown, seen: WeakSet<object> = new WeakSet()): boolean {
  if (value === null || typeof value !== 'object') return false
  if (isChannelRef(value) || streamKind(value)) return true
  // Seen already: that first visit gives the answer.
  if (seen.has(value)) return false
  seen.add(value)
  if (Array.isArray(value)) {
    for (const item of value) if (containsStreams(item, seen)) return true
    return false
  }
  if (!isPlainObject(value)) return false
  for (const key in value) {
    if (containsStreams(value[key], seen)) return true
  }
  return false
}

/** Whether `value` holds a stream value (a source, a sink, a stream), not counting channel refs. */
export function containsStreamValues(value: unknown): boolean {
  let found = false
  mapStreams(value, (slot, kind) => {
    if (kind !== 'ref') found = true
    return slot
  })
  return found
}

/**
 * Whether `value` hands whoever receives it a write side: an OutputStream, a
 * native sink, or a write ref. A call that returns one does not wait for its
 * sinks, since what the caller writes through it can be what they wait for.
 */
export function holdsWriteSide(value: unknown): boolean {
  let found = false
  mapStreams(value, (slot, kind) => {
    if (kind === 'output' || kind === 'sink' || (kind === 'ref' && (slot as StreamChannelRef).direction === 'write')) {
      found = true
    }
    return slot
  })
  return found
}

/**
 * A copy of `value` that keeps its streams: arrays and plain objects are
 * copied (once each, so a cycle is copied as a cycle), an {@link InputStream}
 * or {@link OutputStream} is the same object, and every other value is copied
 * with `cloneLeaf`.
 */
export function cloneKeepingStreams(
  value: unknown,
  cloneLeaf: (leaf: unknown) => unknown,
  copies: Map<object, unknown> = new Map(),
): unknown {
  if (value === null || typeof value !== 'object') return cloneLeaf(value)
  if (value instanceof InputStream || value instanceof OutputStream) return value
  if (copies.has(value)) return copies.get(value)
  if (Array.isArray(value)) {
    const out: unknown[] = new Array(value.length)
    copies.set(value, out)
    value.forEach((item, i) => {
      out[i] = cloneKeepingStreams(item, cloneLeaf, copies)
    })
    return out
  }
  if (!isPlainObject(value)) return cloneLeaf(value)
  const out: Record<string, unknown> = {}
  copies.set(value, out)
  for (const [k, v] of Object.entries(value)) setOwn(out, k, cloneKeepingStreams(v, cloneLeaf, copies))
  return out
}

/**
 * Whether `payload` is the `Observed` envelope the engine built for the
 * activation it arrived with: its `activation_id` and `observer_id` are the
 * activation's. A call through an observer gets its activation id from the
 * engine after the caller built the payload, so its payload never matches.
 */
export function isObservedEnvelope(
  payload: unknown,
  activation: { observer_id: string; activation_id?: string } | undefined,
): payload is Record<string, unknown> {
  if (!activation || typeof activation.activation_id !== 'string') return false
  if (payload === null || typeof payload !== 'object' || Array.isArray(payload)) return false
  const envelope = payload as Record<string, unknown>
  return envelope.activation_id === activation.activation_id && envelope.observer_id === activation.observer_id
}

/** The ref as it goes on the wire: its three fields, no more, so every SDK reads it. */
export function wireRef(ref: StreamChannelRef): StreamChannelRef {
  return { channel_id: ref.channel_id, access_key: ref.access_key, direction: ref.direction }
}

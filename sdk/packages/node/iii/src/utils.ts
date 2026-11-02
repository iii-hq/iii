import * as fs from 'node:fs'
import * as path from 'node:path'
import type { StreamChannelRef } from './iii-types'

/**
 * Handshake headers for an engine connection: the caller's, plus
 * `Authorization: Bearer $III_TOKEN` when the variable is set and not blank.
 * An `Authorization` header the caller passed wins (names match
 * case-insensitively). Browsers cannot set handshake headers, so this lives in
 * the Node package only.
 */
export function resolveHeaders(explicit?: Record<string, string>): Record<string, string> | undefined {
  const token = process.env.III_TOKEN?.trim()
  if (!token || Object.keys(explicit ?? {}).some((name) => name.toLowerCase() === 'authorization')) {
    return explicit
  }
  return { ...explicit, Authorization: `Bearer ${token}` }
}

/**
 * Returns a project identifier for telemetry, derived from the current working
 * directory. Reads `package.json` `name` if present at `cwd`; otherwise falls
 * back to the basename of `cwd`. Returns `undefined` only when both signals
 * are unavailable (e.g. cwd is the filesystem root).
 *
 * No directory walking, only inspects `cwd` itself, so the SDK never reads
 * files outside the user's explicit working directory.
 */
export function detectProjectName(cwd: string = process.cwd()): string | undefined {
  try {
    const manifest = path.join(cwd, 'package.json')
    if (fs.existsSync(manifest)) {
      const parsed = JSON.parse(fs.readFileSync(manifest, 'utf8')) as { name?: unknown }
      if (typeof parsed.name === 'string') {
        const trimmed = parsed.name.trim()
        if (trimmed) return trimmed
      }
    }
  } catch {
    // fall through to directory-name fallback
  }

  const base = path.basename(cwd).trim()
  return base || undefined
}

/**
 * Type guard that checks if a value is a {@link StreamChannelRef}.
 *
 * @param value - Value to check.
 * @returns `true` if the value is a valid `StreamChannelRef`.
 */
export const isChannelRef = (value: unknown): value is StreamChannelRef => {
  if (typeof value !== 'object' || value === null) return false
  const maybe = value as Partial<StreamChannelRef>
  return (
    typeof maybe.channel_id === 'string' &&
    typeof maybe.access_key === 'string' &&
    (maybe.direction === 'read' || maybe.direction === 'write')
  )
}

/**
 * Recursively extract all {@link StreamChannelRef} values from a JSON-like
 * input, returning each match paired with its dotted/bracketed path. Mirrors
 * the Rust SDK's `extract_channel_refs`.
 *
 * @param data - Arbitrary JSON-like value.
 * @returns Array of `[path, ref]` tuples. Empty when no refs are found.
 */
export const extractChannelRefs = (data: unknown): Array<[string, StreamChannelRef]> => {
  const refs: Array<[string, StreamChannelRef]> = []
  extractRefsRecursive(data, '', refs)
  return refs
}

const extractRefsRecursive = (
  data: unknown,
  prefix: string,
  refs: Array<[string, StreamChannelRef]>,
): void => {
  if (isChannelRef(data)) {
    refs.push([prefix, data])
    return
  }
  if (Array.isArray(data)) {
    for (let i = 0; i < data.length; i++) {
      const path = prefix === '' ? `[${i}]` : `${prefix}[${i}]`
      extractRefsRecursive(data[i], path, refs)
    }
    return
  }
  if (typeof data !== 'object' || data === null) return

  for (const [key, value] of Object.entries(data as Record<string, unknown>)) {
    const path = prefix === '' ? key : `${prefix}.${key}`
    extractRefsRecursive(value, path, refs)
  }
}

/**
 * Freezes the plain objects and arrays of `value`, deeply, so a write to
 * them throws (in strict code). Class instances (a `Date`, a `Map`, a
 * channel) are left as they are, and so is what they hold. Returns `value`.
 */
export function freezePlain<T>(value: T): T {
  const stack: unknown[] = [value]
  while (stack.length > 0) {
    const v = stack.pop()
    if (v === null || typeof v !== 'object' || Object.isFrozen(v)) continue
    const proto = Object.getPrototypeOf(v)
    if (!Array.isArray(v) && proto !== Object.prototype && proto !== null) continue
    Object.freeze(v)
    for (const member of Object.values(v as Record<string, unknown>)) stack.push(member)
  }
  return value
}

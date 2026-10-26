import { FilterError, MAX_PATH_SEGMENTS } from './errors'
import { arrayElement, type JsonView, objectMember } from './value'

/** A path's first segment. `@` is the current `$any` element. */
export type Root = 'input' | 'output' | 'error' | 'caller' | 'metadata' | '@'

const ROOTS: ReadonlySet<string> = new Set(['input', 'output', 'error', 'caller', 'metadata', '@'])

/**
 * One segment after the root. `index` is set when the segment is a canonical
 * decimal (`0` or `[1-9][0-9]*`): it selects an array element, while `key`
 * looks up an object member.
 */
export type Segment = { key: string; index: number | undefined }

export type Path = { root: Root; segments: Segment[] }

const CANONICAL_INDEX = /^(?:0|[1-9][0-9]*)$/

function canonicalIndex(segment: string): number | undefined {
  // Longer than any array can be: never selects an element.
  if (segment.length > 15 || !CANONICAL_INDEX.test(segment)) return undefined
  return Number(segment)
}

function emptySegment(raw: string): FilterError {
  return FilterError.invalid(raw === '' ? 'empty path' : `path \`${raw}\`: empty segment`)
}

/**
 * Parses `root ("." segment)*`. `\.` and `\\` escape inside keys; any other
 * backslash sequence, an empty segment or an unknown root is
 * `INVALID_FILTER`. `@` is a root only inside `$any`, where it is the only
 * root allowed.
 */
export function parsePath(raw: string, inAny: boolean): Path {
  const parts: string[] = []
  let current = ''
  for (let i = 0; i < raw.length; i++) {
    const c = raw[i]
    if (c === '\\') {
      const next = raw[i + 1]
      if (next === '.' || next === '\\') {
        current += next
        i++
      } else if (next === undefined) {
        throw FilterError.invalid(`path \`${raw}\`: dangling \`\\\` at the end`)
      } else {
        throw FilterError.invalid(
          `path \`${raw}\`: invalid escape \`\\${next}\` (only \`\\.\` and \`\\\\\` are allowed)`,
        )
      }
    } else if (c === '.') {
      if (current === '') throw emptySegment(raw)
      parts.push(current)
      current = ''
    } else {
      current += c
    }
  }
  if (current === '') throw emptySegment(raw)
  parts.push(current)

  const first = parts[0]
  if (!ROOTS.has(first)) {
    throw FilterError.invalid(
      inAny
        ? `path \`${raw}\`: inside \`$any\` paths start at \`@\`, the current element`
        : `path \`${raw}\`: unknown root \`${first}\` (expected input, output, error, caller or metadata)`,
    )
  }
  if (first === '@' && !inAny) {
    throw FilterError.invalid(`path \`${raw}\`: \`@\` paths are only allowed inside \`$any\``)
  }
  if (first !== '@' && inAny) {
    throw FilterError.invalid(`path \`${raw}\`: inside \`$any\` paths start at \`@\`, the current element`)
  }
  const segments = parts.slice(1).map((key) => ({ key, index: canonicalIndex(key) }))
  if (segments.length > MAX_PATH_SEGMENTS) {
    throw FilterError.invalid(
      `path \`${raw}\`: ${segments.length} segments after the root (limit ${MAX_PATH_SEGMENTS})`,
    )
  }
  return { root: first as Root, segments }
}

/** The document a filter reads. Roots that are `undefined` are absent. */
export type FilterDoc = {
  input?: unknown
  output?: unknown
  error?: unknown
  caller?: unknown
  metadata?: unknown
}

/** Compiles the path into a resolver; `undefined` means absent. */
export function compilePath(path: Path): (doc: FilterDoc, element: JsonView) => JsonView {
  const { root, segments } = path
  const step = (start: JsonView): JsonView => {
    let current = start
    for (let i = 0; i < segments.length; i++) {
      if (current === undefined || current === null || typeof current !== 'object') return undefined
      const segment = segments[i]
      if (Array.isArray(current)) {
        const index = segment.index
        if (index === undefined || index >= current.length) return undefined
        current = arrayElement(current, index)
      } else {
        current = objectMember(current as Record<string, unknown>, segment.key)
      }
    }
    return current
  }
  if (root === '@') return (_doc, element) => step(element)
  // A root is an own enumerable member of the document, like any other key.
  return (doc) => step(objectMember(doc as Record<string, unknown>, root))
}

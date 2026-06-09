import { describe, expect, it } from 'vitest'

import { parseIntegerEnv, parseNumberEnv, resolveFlushIntervalMs } from './utils'

describe('parseNumberEnv', () => {
  it('returns undefined when the variable is unset', () => {
    expect(parseNumberEnv(undefined, 0)).toBeUndefined()
  })

  it('parses a plain number', () => {
    expect(parseNumberEnv('250', 0)).toBe(250)
  })

  it('allows an explicit zero when the minimum permits it', () => {
    expect(parseNumberEnv('0', 0)).toBe(0)
  })

  it('treats an empty string as unset instead of coercing to 0', () => {
    expect(parseNumberEnv('', 0)).toBeUndefined()
  })

  it('treats a whitespace-only string as unset instead of coercing to 0', () => {
    expect(parseNumberEnv('   ', 0)).toBeUndefined()
  })

  it('rejects values below the minimum', () => {
    expect(parseNumberEnv('-5', 0)).toBeUndefined()
    expect(parseNumberEnv('0', 1)).toBeUndefined()
  })

  it('rejects non-numeric values', () => {
    expect(parseNumberEnv('abc', 0)).toBeUndefined()
    expect(parseNumberEnv('NaN', 0)).toBeUndefined()
    expect(parseNumberEnv('Infinity', 0)).toBeUndefined()
  })
})

describe('parseIntegerEnv', () => {
  it('parses integers and rejects fractions', () => {
    expect(parseIntegerEnv('3', 1)).toBe(3)
    expect(parseIntegerEnv('2.5', 1)).toBeUndefined()
  })

  it('treats an empty string as unset', () => {
    expect(parseIntegerEnv('', 0)).toBeUndefined()
  })
})

describe('resolveFlushIntervalMs', () => {
  it('prefers the explicit config value over everything', () => {
    expect(resolveFlushIntervalMs(50, '200', 100)).toBe(50)
  })

  it('respects an explicit config value of 0', () => {
    expect(resolveFlushIntervalMs(0, '200', 100)).toBe(0)
  })

  it('falls back to the III-specific env var next', () => {
    expect(resolveFlushIntervalMs(undefined, '200', 100)).toBe(200)
  })

  it('uses the default when config and env are unset', () => {
    expect(resolveFlushIntervalMs(undefined, undefined, 100)).toBe(100)
  })

  it('falls through past empty or invalid env values to the default', () => {
    expect(resolveFlushIntervalMs(undefined, '', 100)).toBe(100)
    expect(resolveFlushIntervalMs(undefined, 'abc', 100)).toBe(100)
  })
})

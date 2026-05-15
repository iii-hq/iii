import { describe, it, expect } from 'vitest'
import {
  redact,
  redactAndTruncate,
  REDACTED_PLACEHOLDER,
  resolveMaxBytesFromEnv,
} from '../src/telemetry-system/payload'

describe('redact', () => {
  it('redacts top-level sensitive keys', () => {
    const out = redact({ api_key: 'sk-abc123', model: 'claude-3-5' }) as Record<string, unknown>
    expect(out.api_key).toBe(REDACTED_PLACEHOLDER)
    expect(out.model).toBe('claude-3-5')
  })

  it('redacts nested sensitive keys', () => {
    const out = redact({
      headers: { Authorization: 'Bearer xyz', 'Content-Type': 'application/json' },
      config: { secret: 'hush' },
    }) as Record<string, Record<string, string>>
    expect(out.headers.Authorization).toBe(REDACTED_PLACEHOLDER)
    expect(out.headers['Content-Type']).toBe('application/json')
    expect(out.config.secret).toBe(REDACTED_PLACEHOLDER)
  })

  it('walks into arrays', () => {
    const out = redact({
      accounts: [
        { access_token: 'a', user: 'alice' },
        { access_token: 'b', user: 'bob' },
      ],
    }) as { accounts: Array<{ access_token: string; user: string }> }
    expect(out.accounts[0].access_token).toBe(REDACTED_PLACEHOLDER)
    expect(out.accounts[0].user).toBe('alice')
    expect(out.accounts[1].access_token).toBe(REDACTED_PLACEHOLDER)
  })

  it('redacts sensitive parent key wholesale', () => {
    const out = redact({
      credentials: [{ user: 'alice', token: 'a' }],
    }) as Record<string, unknown>
    // `credentials` itself is a sensitive key → whole subtree redacted.
    expect(out.credentials).toBe(REDACTED_PLACEHOLDER)
  })

  it('matches keys case-insensitively', () => {
    const out = redact({ API_KEY: 'x', PassWord: 'y', client_SECRET: 'z' }) as Record<string, unknown>
    expect(out.API_KEY).toBe(REDACTED_PLACEHOLDER)
    expect(out.PassWord).toBe(REDACTED_PLACEHOLDER)
    expect(out.client_SECRET).toBe(REDACTED_PLACEHOLDER)
  })

  it('redacts bare and suffix token keys, not substrings', () => {
    const out = redact({
      token: 'tok-1',
      id_token: 'tok-2',
      notification: 'ping',
      function_id: 'do_thing',
    }) as Record<string, unknown>
    expect(out.token).toBe(REDACTED_PLACEHOLDER)
    expect(out.id_token).toBe(REDACTED_PLACEHOLDER)
    expect(out.notification).toBe('ping')
    expect(out.function_id).toBe('do_thing')
  })

  it('handles null and primitives without crashing', () => {
    expect(redact(null)).toBe(null)
    expect(redact(undefined)).toBe(undefined)
    expect(redact(42)).toBe(42)
    expect(redact('hello')).toBe('hello')
    expect(redact(true)).toBe(true)
  })
})

describe('redactAndTruncate', () => {
  it('returns untruncated when under limit', () => {
    const { json, truncated } = redactAndTruncate({ model: 'claude-3-5' }, 4096)
    expect(truncated).toBe(false)
    expect(json).not.toContain('[TRUNCATED]')
  })

  it('truncates when over limit', () => {
    const big = 'x'.repeat(8192)
    const { json, truncated } = redactAndTruncate({ blob: big }, 4096)
    expect(truncated).toBe(true)
    expect(json).toContain('[TRUNCATED]')
    expect(Buffer.byteLength(json, 'utf8')).toBeLessThanOrEqual(4096)
  })

  it('respects maxBytes when smaller than the truncation marker', () => {
    // The marker itself is ~16 bytes; output must never exceed maxBytes
    // even when the cap is below marker length.
    const big = 'x'.repeat(100)
    for (const max of [1, 4, 8, 12]) {
      const { json, truncated } = redactAndTruncate({ blob: big }, max)
      expect(truncated).toBe(true)
      expect(Buffer.byteLength(json, 'utf8')).toBeLessThanOrEqual(max)
    }
  })

  it('never truncates when maxBytes is null (default)', () => {
    const big = 'x'.repeat(1_000_000)
    const { json, truncated } = redactAndTruncate({ blob: big })
    expect(truncated).toBe(false)
    expect(json).not.toContain('[TRUNCATED]')
    expect(Buffer.byteLength(json, 'utf8')).toBeGreaterThan(1_000_000)
  })

  it('never truncates when maxBytes is 0', () => {
    const big = 'x'.repeat(8192)
    const { json, truncated } = redactAndTruncate({ blob: big }, 0)
    expect(truncated).toBe(false)
    expect(json).not.toContain('[TRUNCATED]')
  })

  it('redaction runs before truncation', () => {
    const { json } = redactAndTruncate(
      { api_key: 'sk-must-not-leak', blob: 'x'.repeat(8192) },
      4096,
    )
    expect(json).not.toContain('sk-must-not-leak')
    expect(json).toContain('[REDACTED]')
  })

  it('preserves valid UTF-8 on truncation', () => {
    const s = 'aéaéaéaé'.repeat(2000)
    const { json, truncated } = redactAndTruncate({ v: s }, 100)
    expect(truncated).toBe(true)
    expect(json.endsWith('..."[TRUNCATED]"')).toBe(true)
  })
})

describe('resolveMaxBytesFromEnv', () => {
  const KEY = 'III_TRACE_PAYLOAD_MAX_BYTES'
  const restore = () => {
    delete process.env[KEY]
  }

  it('returns null when env var is unset', () => {
    restore()
    expect(resolveMaxBytesFromEnv()).toBeNull()
  })

  it('returns null for "0"', () => {
    process.env[KEY] = '0'
    expect(resolveMaxBytesFromEnv()).toBeNull()
    restore()
  })

  it('returns null for "unlimited"', () => {
    process.env[KEY] = 'unlimited'
    expect(resolveMaxBytesFromEnv()).toBeNull()
    restore()
  })

  it('returns the parsed integer for positive values', () => {
    process.env[KEY] = '8192'
    expect(resolveMaxBytesFromEnv()).toBe(8192)
    restore()
  })

  it('returns null for unparseable input', () => {
    process.env[KEY] = 'foo'
    expect(resolveMaxBytesFromEnv()).toBeNull()
    restore()
  })
})

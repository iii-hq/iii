import { describe, expect, it } from 'vitest'
import { extractErrorMessage } from './dlq-error'

describe('extractErrorMessage', () => {
  it('extracts a plain message from the Rust ErrorBody debug format', () => {
    const raw =
      'ErrorBody { code: "invocation_failed", message: "Simulated failure", stacktrace: None }'
    expect(extractErrorMessage(raw)).toBe('Simulated failure')
  })

  it('surfaces code and inner message from a worker-op JSON envelope with escaped quotes', () => {
    const raw =
      'ErrorBody { code: "W105", message: "{\\"type\\":\\"WorkerOpError\\",\\"code\\":\\"W105\\",\\"message\\":\\"invalid payload for \\\\\\"worker::add\\\\\\": missing field `source`\\",\\"details\\":{}}", stacktrace: None }'
    const result = extractErrorMessage(raw)
    expect(result).toContain('W105')
    expect(result).toContain('missing field `source`')
    expect(result).not.toContain('"type"')
  })

  it('humanizes a bare JSON envelope without the Debug wrapper', () => {
    const raw =
      '{"type":"WorkerOpError","code":"W110","message":"worker \\"x\\" not found","details":{"name":"x"}}'
    expect(extractErrorMessage(raw)).toBe('W110: worker "x" not found')
  })

  it('truncates long plain strings', () => {
    const raw = 'y'.repeat(150)
    expect(extractErrorMessage(raw)).toBe(`${'y'.repeat(100)}...`)
  })

  it('returns short plain strings unchanged', () => {
    expect(extractErrorMessage('boom')).toBe('boom')
  })

  it('caps an oversized envelope message extracted from the Debug wrapper', () => {
    const long = 'z'.repeat(300)
    const raw = `ErrorBody { code: "invocation_failed", message: "${long}", stacktrace: None }`
    expect(extractErrorMessage(raw)).toBe(`${'z'.repeat(100)}...`)
  })

  it('caps an oversized bare JSON envelope', () => {
    const raw = JSON.stringify({ type: 'WorkerOpError', code: 'W900', message: 'q'.repeat(300) })
    const result = extractErrorMessage(raw)
    expect(result.startsWith('W900: ')).toBe(true)
    expect(result.endsWith('...')).toBe(true)
    expect(result.length).toBe(103)
  })

  it('drops a non-string code and returns the message alone', () => {
    expect(extractErrorMessage('{"message":"hi","code":123}')).toBe('hi')
  })

  it('returns the raw input for a json object without a message field', () => {
    expect(extractErrorMessage('{"code":"W110"}')).toBe('{"code":"W110"}')
  })
})

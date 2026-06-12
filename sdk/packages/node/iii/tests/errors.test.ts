import { describe, expect, it } from 'vitest'

import { InvocationError } from '../src/index'
import { isErrorBody } from '../src/errors'

describe('InvocationError', () => {
  it('exposes code, function_id, stacktrace and a readable message', () => {
    const err = new InvocationError({
      code: 'FORBIDDEN',
      message: "function 'engine::functions::list' not allowed",
      function_id: 'engine::functions::list',
      stacktrace: 'trace here',
    })

    expect(err).toBeInstanceOf(Error)
    expect(err).toBeInstanceOf(InvocationError)
    expect(err.name).toBe('InvocationError')
    expect(err.code).toBe('FORBIDDEN')
    expect(err.function_id).toBe('engine::functions::list')
    expect(err.stacktrace).toBe('trace here')
    expect(err.message).toBe("FORBIDDEN: function 'engine::functions::list' not allowed")
  })

  it('does NOT serialize to "[object Object]" (the original bug)', () => {
    const err = new InvocationError({
      code: 'FORBIDDEN',
      message: "function 'engine::functions::list' not allowed",
      function_id: 'engine::functions::list',
    })

    // The bug reporter saw `[object Object]` when printing a plain ErrorBody.
    // Real Error subclasses stringify as `Name: message`, not `[object Object]`.
    expect(String(err)).not.toBe('[object Object]')
    expect(String(err)).toContain('InvocationError')
    expect(String(err)).toContain("engine::functions::list")
  })

  it('propagates stack traces as a real Error subclass', () => {
    const err = new InvocationError({ code: 'UNKNOWN', message: 'oops' })
    expect(err.stack).toBeTruthy()
    expect(typeof err.stack).toBe('string')
  })

  it('supports errors without function_id or stacktrace', () => {
    const err = new InvocationError({ code: 'TIMEOUT', message: 'gone' })
    expect(err.function_id).toBeUndefined()
    expect(err.stacktrace).toBeUndefined()
    expect(err.message).toBe('TIMEOUT: gone')
  })
})

describe('isErrorBody', () => {
  it('identifies wire-format ErrorBody objects', () => {
    expect(isErrorBody({ code: 'FORBIDDEN', message: 'nope' })).toBe(true)
    expect(isErrorBody({ code: 'X', message: 'y', stacktrace: 'z' })).toBe(true)
  })

  it('rejects non-ErrorBody values', () => {
    expect(isErrorBody(null)).toBe(false)
    expect(isErrorBody(undefined)).toBe(false)
    expect(isErrorBody('string')).toBe(false)
    expect(isErrorBody(42)).toBe(false)
    expect(isErrorBody({ code: 'X' })).toBe(false)
    expect(isErrorBody({ message: 'Y' })).toBe(false)
    expect(isErrorBody({ code: 1, message: 'Y' })).toBe(false)
    expect(isErrorBody(new Error('plain'))).toBe(false)
  })
})

describe('errors subpath and back-compat', () => {
  it('exports InvocationError from the iii-sdk/errors subpath', async () => {
    const errs = await import('../src/errors')
    expect(errs.InvocationError).toBeDefined()
    expect(typeof errs.InvocationError).toBe('function')
    const root = await import('../src/index')
    expect(root.InvocationError).toBe(errs.InvocationError)
  })

  it('keeps the deprecated IIIInvocationError alias pointing at InvocationError', async () => {
    const errs = await import('../src/errors')
    const root = await import('../src/index')
    expect(root.IIIInvocationError).toBe(errs.InvocationError)
    const err = new root.IIIInvocationError({ code: 'FORBIDDEN', message: 'nope' })
    expect(err).toBeInstanceOf(errs.InvocationError)
  })
})

import { describe, expect, it } from 'vitest'
import { Logger } from './logger'

describe('Logger', () => {
  it('exposes info, warn, error, and debug methods', () => {
    const log = new Logger()
    expect(typeof log.info).toBe('function')
    expect(typeof log.warn).toBe('function')
    expect(typeof log.error).toBe('function')
    expect(typeof log.debug).toBe('function')
  })
})

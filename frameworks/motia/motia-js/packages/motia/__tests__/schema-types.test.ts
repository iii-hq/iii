import { isAnyOf, JsonSchemaError } from '../src/types/schema.types'

describe('schema.types', () => {
  describe('JsonSchemaError', () => {
    it('extends Error with name JsonSchemaError', () => {
      const err = new JsonSchemaError('test message')
      expect(err).toBeInstanceOf(Error)
      expect(err).toBeInstanceOf(JsonSchemaError)
      expect(err.name).toBe('JsonSchemaError')
      expect(err.message).toBe('test message')
    })
  })

  describe('isAnyOf', () => {
    it('returns true for objects with anyOf array', () => {
      expect(isAnyOf({ anyOf: [{ type: 'string' }, { type: 'number' }] })).toBe(true)
    })

    it('returns false for objects without anyOf', () => {
      expect(isAnyOf({ type: 'string' })).toBe(false)
      expect(isAnyOf({ properties: {} })).toBe(false)
    })

    it('returns false for null', () => {
      expect(isAnyOf(null as any)).toBe(false)
    })
  })
})

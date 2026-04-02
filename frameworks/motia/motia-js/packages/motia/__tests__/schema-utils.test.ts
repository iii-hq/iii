import * as z from 'zod'
import { isJsonSchema, isStandardSchema, isZodSchema, schemaToJsonSchema } from '../src/schema-utils'

describe('schema-utils', () => {
  describe('isStandardSchema', () => {
    it('detects ~standard property', () => {
      const schema = { '~standard': { version: 1 } }
      expect(isStandardSchema(schema)).toBe(true)
    })

    it('returns false for null and undefined', () => {
      expect(isStandardSchema(null)).toBe(false)
      expect(isStandardSchema(undefined)).toBe(false)
    })

    it('returns false for primitives', () => {
      expect(isStandardSchema('string')).toBe(false)
      expect(isStandardSchema(123)).toBe(false)
      expect(isStandardSchema(true)).toBe(false)
    })

    it('returns false when ~standard is null', () => {
      expect(isStandardSchema({ '~standard': null })).toBe(false)
    })
  })

  describe('isZodSchema', () => {
    it('detects Zod schemas', () => {
      expect(isZodSchema(z.string())).toBe(true)
      expect(isZodSchema(z.object({ a: z.number() }))).toBe(true)
    })

    it('returns false for plain objects', () => {
      expect(isZodSchema({})).toBe(false)
      expect(isZodSchema({ type: 'string' })).toBe(false)
    })

    it('returns false for null and undefined', () => {
      expect(isZodSchema(null)).toBe(false)
      expect(isZodSchema(undefined)).toBe(false)
    })
  })

  describe('isJsonSchema', () => {
    it('detects JSON schema with type string', () => {
      expect(isJsonSchema({ type: 'string' })).toBe(true)
    })

    it('detects JSON schema with properties', () => {
      expect(isJsonSchema({ properties: { a: { type: 'string' } } })).toBe(true)
    })

    it('returns false for null and undefined', () => {
      expect(isJsonSchema(null)).toBe(false)
      expect(isJsonSchema(undefined)).toBe(false)
    })

    it('returns false for arrays', () => {
      expect(isJsonSchema([])).toBe(false)
    })
  })

  describe('schemaToJsonSchema', () => {
    it('converts Zod to JSON Schema', () => {
      const result = schemaToJsonSchema(z.string())
      expect(result).not.toBeNull()
      expect(result).toMatchObject({ type: 'string' })
    })

    it('passes through JSON Schema as-is', () => {
      const jsonSchema = { type: 'object', properties: { name: { type: 'string' } } }
      const result = schemaToJsonSchema(jsonSchema)
      expect(result).toEqual(jsonSchema)
    })

    it('returns null for null input', () => {
      expect(schemaToJsonSchema(null)).toBeNull()
    })

    it('returns null for undefined input', () => {
      expect(schemaToJsonSchema(undefined)).toBeNull()
    })

    it('returns null for StandardSchema without toJSON', () => {
      const standardSchema = { '~standard': { version: 1 } }
      expect(schemaToJsonSchema(standardSchema)).toBeNull()
    })

    it('converts StandardSchema with toJSON', () => {
      const standardSchema = {
        '~standard': { version: 1 },
        toJSON: () => ({ type: 'string' }),
      }
      const result = schemaToJsonSchema(standardSchema)
      expect(result).toEqual({ type: 'string' })
    })
  })
})

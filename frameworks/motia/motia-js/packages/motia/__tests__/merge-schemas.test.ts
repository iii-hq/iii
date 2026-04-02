import * as z from 'zod'
import { isCompatible, mergeSchemas } from '../src/types/merge-schemas'
import { JsonSchemaError } from '../src/types/schema.types'

describe('merge-schemas', () => {
  describe('isCompatible', () => {
    it('returns true for same-type primitives', () => {
      expect(isCompatible({ type: 'string' } as any, { type: 'string' } as any)).toBe(true)
      expect(isCompatible({ type: 'number' } as any, { type: 'number' } as any)).toBe(true)
    })

    it('returns false for different types', () => {
      expect(isCompatible({ type: 'string' } as any, { type: 'number' } as any)).toBe(false)
      expect(isCompatible({ type: 'object' } as any, { type: 'array' } as any)).toBe(false)
    })

    it('checks object properties recursively', () => {
      const a = { type: 'object' as const, properties: { x: { type: 'string' as const } } }
      const b = { type: 'object' as const, properties: { x: { type: 'string' as const } } }
      expect(isCompatible(a as any, b as any)).toBe(true)
    })

    it('returns false for incompatible object properties', () => {
      const a = { type: 'object' as const, properties: { x: { type: 'string' as const } } }
      const b = { type: 'object' as const, properties: { x: { type: 'number' as const } } }
      expect(isCompatible(a as any, b as any)).toBe(false)
    })

    it('checks array items recursively', () => {
      const a = { type: 'array' as const, items: { type: 'string' as const } }
      const b = { type: 'array' as const, items: { type: 'string' as const } }
      expect(isCompatible(a as any, b as any)).toBe(true)
    })

    it('handles anyOf schemas', () => {
      const a = { anyOf: [{ type: 'string' as const }] }
      const b = { type: 'string' as const }
      expect(isCompatible(a as any, b as any)).toBe(true)
    })
  })

  describe('mergeSchemas', () => {
    it('merges two Zod schemas via intersection', () => {
      const a = z.object({ a: z.string() })
      const b = z.object({ b: z.number() })
      const result = mergeSchemas(a, b)
      expect(result).not.toBeNull()
      expect(result.type === 'object' || 'allOf' in result).toBe(true)
    })

    it('merges two object JSON schemas', () => {
      const a = {
        type: 'object' as const,
        properties: { name: { type: 'string' as const }, age: { type: 'number' as const } },
        required: ['name'] as const,
      }
      const b = {
        type: 'object' as const,
        properties: { name: { type: 'string' as const }, age: { type: 'number' as const } },
        required: ['age'] as const,
      }
      const result = mergeSchemas(a, b)
      expect(result.type).toBe('object')
      expect(result.properties).toHaveProperty('name')
      expect(result.properties).toHaveProperty('age')
      expect(result.required).toContain('name')
      expect(result.required).toContain('age')
    })

    it('merges array schemas', () => {
      const a = { type: 'array' as const, items: { type: 'string' as const } }
      const b = { type: 'array' as const, items: { type: 'string' as const } }
      const result = mergeSchemas(a, b)
      expect(result.type).toBe('array')
      expect(result.items).toEqual({ type: 'string' })
    })

    it('throws JsonSchemaError on incompatible schemas', () => {
      const a = { type: 'string' as const }
      const b = { type: 'number' as const }
      expect(() => mergeSchemas(a, b)).toThrow(JsonSchemaError)
      expect(() => mergeSchemas(a, b)).toThrow('Cannot merge schemas of different types')
    })

    it('throws when schemas cannot be converted', () => {
      expect(() => mergeSchemas(null as any, { type: 'string' })).toThrow(JsonSchemaError)
      expect(() => mergeSchemas(null as any, { type: 'string' })).toThrow(
        'Cannot merge schemas: failed to convert to JSON Schema',
      )
    })
  })
})

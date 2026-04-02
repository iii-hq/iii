import { generateTypeFromSchema } from '../src/types/generate-type-from-schema'

describe('generateTypeFromSchema', () => {
  it('returns unknown for falsy input', () => {
    expect(generateTypeFromSchema(null as any)).toBe('unknown')
    expect(generateTypeFromSchema(undefined as any)).toBe('unknown')
  })

  it('handles anyOf by joining types with |', () => {
    const schema = { anyOf: [{ type: 'string' }, { type: 'number' }] }
    expect(generateTypeFromSchema(schema as any)).toBe('string | number')
  })

  it('handles type array with items', () => {
    const schema = { type: 'array', items: { type: 'string' } }
    expect(generateTypeFromSchema(schema as any)).toBe('Array<string>')
  })

  it('handles type array without items', () => {
    const schema = { type: 'array' }
    expect(generateTypeFromSchema(schema as any)).toBe('Array<unknown>')
  })

  it('handles type object with properties and required', () => {
    const schema = {
      type: 'object',
      properties: { name: { type: 'string' }, age: { type: 'number' } },
      required: ['name'],
    }
    const result = generateTypeFromSchema(schema as any)
    expect(result).toContain('name: string')
    expect(result).toContain('age?: number')
  })

  it('handles type object with additionalProperties', () => {
    const schema = { type: 'object', additionalProperties: { type: 'string' } }
    expect(generateTypeFromSchema(schema as any)).toBe('Record<string, string>')
  })

  it('handles type string with format binary', () => {
    const schema = { type: 'string', format: 'binary' }
    expect(generateTypeFromSchema(schema as any)).toBe('Buffer')
  })

  it('handles type string with enum', () => {
    const schema = { type: 'string', enum: ['a', 'b'] }
    expect(generateTypeFromSchema(schema as any)).toBe("'a' | 'b'")
  })

  it('handles type string without enum', () => {
    const schema = { type: 'string' }
    expect(generateTypeFromSchema(schema as any)).toBe('string')
  })

  it('handles schema with not', () => {
    const schema = { not: { type: 'null' } }
    expect(generateTypeFromSchema(schema as any)).toBe('undefined')
  })

  it('handles type number', () => {
    const schema = { type: 'number' }
    expect(generateTypeFromSchema(schema as any)).toBe('number')
  })

  it('handles type boolean', () => {
    const schema = { type: 'boolean' }
    expect(generateTypeFromSchema(schema as any)).toBe('boolean')
  })

  it('handles unknown type as unknown', () => {
    const schema = { type: 'integer' }
    expect(generateTypeFromSchema(schema as any)).toBe('unknown')
  })
})

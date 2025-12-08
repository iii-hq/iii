import type { StandardSchemaV1 } from '@standard-schema/spec'
import * as z from 'zod'
import type { JsonSchema } from './types/schema.types'

// export interface StandardSchema<T = unknown> {
//     parse: (data: unknown) => T
//     safeParse: (data: unknown) => { success: true; data: T } | { success: false; error: unknown }
//     validate?: (data: unknown) => boolean
//     toJSON?: () => JsonSchema
// }

export type SchemaInput = StandardSchemaV1 | z.ZodType | JsonSchema | null | undefined

export function isStandardSchema(value: unknown): value is StandardSchemaV1 {
  if (!value || typeof value !== 'object') {
    return false
  }

  const schema = value as Record<string, unknown>

  return '~standard' in schema && schema['~standard'] !== null && typeof schema['~standard'] === 'object'
}

export function isZodSchema(value: unknown): value is z.ZodType {
  return Boolean(
    value &&
      typeof value === 'object' &&
      typeof (value as z.ZodType).safeParse === 'function' &&
      '_zod' in (value as z.ZodType) &&
      typeof (value as z.ZodType & { _zod: { def: unknown } })._zod === 'object' &&
      'def' in (value as z.ZodType & { _zod: { def: unknown } })._zod,
  )
}

export function isJsonSchema(value: unknown): value is JsonSchema {
  if (!value || typeof value !== 'object') {
    return false
  }
  const schema = value as Record<string, unknown>
  return typeof schema.type === 'string' || Array.isArray(schema.type) || 'properties' in schema
}

export function schemaToJsonSchema(schema: SchemaInput): JsonSchema | null {
  if (!schema) {
    return null
  }

  if (isZodSchema(schema)) {
    return z.toJSONSchema(schema, { target: 'draft-7' }) as JsonSchema
  }

  if (isStandardSchema(schema)) {
    if ('toJSON' in schema && typeof schema.toJSON === 'function') {
      return schema.toJSON() as JsonSchema
    }
    return null
  }

  if (isJsonSchema(schema)) {
    return schema as JsonSchema
  }

  return null
}

import type { JSONSchema } from 'zod/v4/core'

export type JsonSchema = JSONSchema.BaseSchema

export class JsonSchemaError extends Error {
  constructor(message: string) {
    super(message)
    this.name = 'JsonSchemaError'
  }
}

export const isAnyOf = (schema: JsonSchema): schema is JSONSchema.BaseSchema & { anyOf: JSONSchema.BaseSchema[] } => {
  return typeof schema === 'object' && schema !== null && 'anyOf' in schema
}

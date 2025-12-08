import * as z from 'zod'
import type { JSONSchema } from 'zod/v4/core'
import { isZodSchema, type SchemaInput, schemaToJsonSchema } from '../schema-utils'
import { isAnyOf, type JsonSchema, JsonSchemaError } from './schema.types'

const isJsonSchema = (value: unknown): value is JsonSchema => {
  return typeof value === 'object' && value !== null && !Array.isArray(value) && typeof value !== 'boolean'
}

export const isCompatible = (schema: JsonSchema, otherSchema: JsonSchema): boolean => {
  if (isAnyOf(schema)) {
    return schema.anyOf.every((item) => isCompatible(item, otherSchema))
  } else if (isAnyOf(otherSchema)) {
    return otherSchema.anyOf.every((item) => isCompatible(schema, item))
  }

  if (schema.type !== otherSchema.type) {
    return false
  }

  if (schema.type === 'array' && otherSchema.type === 'array') {
    if (!schema.items || !otherSchema.items) {
      return schema.items === otherSchema.items
    }
    if (Array.isArray(schema.items) || Array.isArray(otherSchema.items)) {
      return false
    }
    if (isJsonSchema(schema.items) && isJsonSchema(otherSchema.items)) {
      return isCompatible(schema.items, otherSchema.items)
    }
    return schema.items === otherSchema.items
  }

  if (schema.type === 'object' && otherSchema.type === 'object') {
    const schemaProps = schema.properties
    const otherSchemaProps = otherSchema.properties
    if (!schemaProps || !otherSchemaProps) {
      return schemaProps === otherSchemaProps
    }
    const keysFromSchema = Object.keys(schemaProps)
    const keysFromOtherSchema = Object.keys(otherSchemaProps)
    const commonKeys = keysFromSchema.filter((key) => keysFromOtherSchema.includes(key))

    if (schema.required?.some((key) => !keysFromOtherSchema.includes(key))) {
      return false
    } else if (otherSchema.required?.some((key) => !keysFromSchema.includes(key))) {
      return false
    }

    if (commonKeys.length > 0) {
      return commonKeys.every((key) => {
        const prop1 = schemaProps[key]
        const prop2 = otherSchemaProps[key]
        if (isJsonSchema(prop1) && isJsonSchema(prop2)) {
          return isCompatible(prop1, prop2)
        }
        return prop1 === prop2
      })
    }
  }

  return true
}

const mergeZodSchemas = (schema: z.ZodType, otherSchema: z.ZodType): z.ZodType => {
  try {
    return z.intersection(schema, otherSchema)
  } catch (error) {
    throw new JsonSchemaError(`Cannot merge Zod schemas: ${error instanceof Error ? error.message : 'Unknown error'}`)
  }
}

export const mergeSchemas = (schema: SchemaInput, otherSchema: SchemaInput): JsonSchema => {
  if (isZodSchema(schema) && isZodSchema(otherSchema)) {
    const mergedZodSchema = mergeZodSchemas(schema, otherSchema)
    const jsonSchema = schemaToJsonSchema(mergedZodSchema)
    if (!jsonSchema) {
      throw new JsonSchemaError('Failed to convert merged Zod schema to JSON Schema')
    }
    return jsonSchema as JsonSchema
  }

  const schemaJsonResult = schemaToJsonSchema(schema)
  const otherSchemaJsonResult = schemaToJsonSchema(otherSchema)

  if (!schemaJsonResult || !otherSchemaJsonResult) {
    throw new JsonSchemaError('Cannot merge schemas: failed to convert to JSON Schema')
  }

  const schemaJson = schemaJsonResult as JsonSchema
  const otherSchemaJson = otherSchemaJsonResult as JsonSchema

  if (!isCompatible(schemaJson, otherSchemaJson)) {
    throw new JsonSchemaError('Cannot merge schemas of different types')
  }

  if (isAnyOf(schemaJson)) {
    return {
      anyOf: schemaJson.anyOf.map((item) => mergeSchemas(item, otherSchemaJson)),
    }
  } else if (isAnyOf(otherSchemaJson)) {
    return {
      anyOf: otherSchemaJson.anyOf.map((item) => mergeSchemas(schemaJson, item)),
    }
  }

  if (schemaJson.type === 'object' && otherSchemaJson.type === 'object') {
    const schemaProps = schemaJson.properties as Record<string, JSONSchema._JSONSchema> | undefined
    const otherSchemaProps = otherSchemaJson.properties as Record<string, JSONSchema._JSONSchema> | undefined
    if (!schemaProps || !otherSchemaProps) {
      throw new JsonSchemaError('Cannot merge object schemas without properties')
    }
    const mergedProperties: Record<string, JSONSchema._JSONSchema> = { ...schemaProps, ...otherSchemaProps }
    const otherSchemaKeys = Object.keys(otherSchemaProps).reduce((acc, key) => {
      acc[key] = true
      return acc
    }, {} as Record<string, boolean>)

    for (const key in schemaProps) {
      if (otherSchemaKeys[key]) {
        const prop1 = schemaProps[key]
        const prop2 = otherSchemaProps[key]
        if (isJsonSchema(prop1) && isJsonSchema(prop2)) {
          mergedProperties[key] = mergeSchemas(prop1, prop2)
        } else {
          mergedProperties[key] = prop2
        }
      }
    }

    const schemaRequired = schemaJson.required as string[] | undefined
    const otherSchemaRequired = otherSchemaJson.required as string[] | undefined
    const mergedRequired = new Set([...(schemaRequired ?? []), ...(otherSchemaRequired ?? [])])

    return {
      type: 'object',
      properties: mergedProperties,
      required: Array.from(mergedRequired),
    }
  }

  if (schemaJson.type === 'array' && otherSchemaJson.type === 'array') {
    if (!schemaJson.items || !otherSchemaJson.items) {
      throw new JsonSchemaError('Cannot merge array schemas without items')
    }
    if (Array.isArray(schemaJson.items) || Array.isArray(otherSchemaJson.items)) {
      throw new JsonSchemaError('Cannot merge array schemas with array items')
    }
    if (!isJsonSchema(schemaJson.items) || !isJsonSchema(otherSchemaJson.items)) {
      throw new JsonSchemaError('Cannot merge array schemas with non-object items')
    }
    return {
      type: 'array',
      items: mergeSchemas(schemaJson.items, otherSchemaJson.items),
    }
  }

  return {
    type: schemaJson.type as JSONSchema.BaseSchema['type'],
    description: (schemaJson.description ?? otherSchemaJson.description) as string | undefined,
  }
}

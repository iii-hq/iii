import type { JSONSchema } from 'zod/v4/core'
import { isAnyOf } from './schema.types'

export const generateTypeFromSchema = (schema: JSONSchema.BaseSchema): string => {
  if (!schema) {
    return 'unknown'
  }

  if (isAnyOf(schema)) {
    const types = schema.anyOf.map(generateTypeFromSchema)
    return types.join(' | ')
  }

  if (schema.type === 'array') {
    const itemType = schema.items ? generateTypeFromSchema(schema.items as JSONSchema.ArraySchema) : 'unknown'
    return `Array<${itemType}>`
  }

  if (schema.type === 'object' && schema.properties) {
    const props = Object.entries(schema.properties).map(([key, prop]) => {
      const isRequired = schema.required?.includes(key)
      const propType = generateTypeFromSchema(prop as JSONSchema.BaseSchema)
      return `${key}${isRequired ? '' : '?'}: ${propType}`
    })
    return props.length > 0 ? `{ ${props.join('; ')} }` : '{}'
  } else if (schema.type === 'object' && schema.additionalProperties) {
    const propType = generateTypeFromSchema(schema.additionalProperties as JSONSchema.BaseSchema)
    return `Record<string, ${propType}>`
  }

  if (schema.type === 'string') {
    if (schema.format === 'binary') {
      return 'Buffer'
    }
    return schema.enum && schema.enum.length > 0 // must have at least one enum value
      ? schema.enum.map((value) => `'${value}'`).join(' | ')
      : 'string'
  }

  if (typeof schema === 'object' && schema !== null && 'not' in schema) {
    return 'undefined'
  }

  switch (schema.type) {
    case 'number':
      return 'number'
    case 'boolean':
      return 'boolean'
    default:
      return 'unknown'
  }
}

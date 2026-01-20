const resolveRef = (ref: string, rootSchema?: Record<string, any>): Record<string, any> | undefined => {
  if (!rootSchema || !ref.startsWith('#/$defs/')) {
    return undefined
  }

  const defName = ref.replace('#/$defs/', '')
  return rootSchema.$defs?.[defName]
}

/**
 * Converts a schema to a JSON object with default values
 * Supports both TypeScript step schemas (inline definitions) and Python step schemas ($defs/$ref)
 * @param schema - The schema to convert
 * @param rootSchema - Optional root schema containing $defs for $ref resolution. Defaults to schema if not provided.
 */
export const convertSchemaToJson = (schema?: Record<string, any>, rootSchema?: Record<string, any>): any => {
  if (!schema) return {}

  const effectiveRootSchema = rootSchema ?? schema

  if (schema.$ref) {
    const resolvedSchema = resolveRef(schema.$ref, effectiveRootSchema)
    if (resolvedSchema) {
      return convertSchemaToJson(resolvedSchema, effectiveRootSchema)
    }
    return {}
  }

  if (schema.anyOf && Array.isArray(schema.anyOf)) {
    const nonNullSchema = schema.anyOf.find((item: any) => item && item.type !== 'null')
    if (nonNullSchema) {
      return convertSchemaToJson(nonNullSchema, effectiveRootSchema)
    }
    return null
  }

  if (schema.type === 'object') {
    const result: Record<string, any> = {}

    if (schema.properties) {
      Object.entries(schema.properties).forEach(([key, value]: [string, any]) => {
        result[key] = convertSchemaToJson(value, effectiveRootSchema)
      })
    }

    return result
  }

  switch (schema.type) {
    case 'array':
      return schema.items ? [convertSchemaToJson(schema.items, effectiveRootSchema)] : []
    case 'string':
      return schema.enum?.[0] ?? schema.description ?? 'string'
    case 'number':
      return schema.description ?? 0
    case 'integer':
      return 0
    case 'boolean':
      return schema.description ?? false
    case 'null':
      return null
    default:
      return undefined
  }
}

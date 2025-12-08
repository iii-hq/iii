import { generateTypeFromSchema } from './generate-type-from-schema'
import type { JsonSchema } from './schema.types'

export const generateTypesFromResponse = (record: Record<number, JsonSchema>): string => {
  return Object.entries(record)
    .map(([status, schema]) => {
      const schemaType = generateTypeFromSchema(schema as JsonSchema)
      return `ApiResponse<${status}, ${schemaType}>`
    })
    .join(' | ')
}

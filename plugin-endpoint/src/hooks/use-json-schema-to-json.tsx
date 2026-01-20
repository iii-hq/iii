import { useEffect, useState } from 'react'
import { convertSchemaToJson } from './utils'

export const useJsonSchemaToJson = (schema: Record<string, any> | undefined) => {
  const [body, setBody] = useState<string>('')

  useEffect(() => {
    if (schema) {
      setBody(JSON.stringify(convertSchemaToJson(schema), null, 2))
    }
  }, [schema])

  return { body, setBody }
}

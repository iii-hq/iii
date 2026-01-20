import { CircleX } from 'lucide-react'
import { type FC, memo, useCallback, useEffect, useRef } from 'react'
import { useShallow } from 'zustand/react/shallow'
import { JsonEditor } from '../components/json-editor'
import { getBodyIsValidSelector, getBodySelector, useEndpointConfiguration } from '../hooks/use-endpoint-configuration'
import { convertSchemaToJson } from '../hooks/utils'

type SidePanelBodyTabProps = {
  schema: Record<string, any> | undefined
}

export const SidePanelBodyTab: FC<SidePanelBodyTabProps> = memo(({ schema }) => {
  const { setBody, setBodyIsValid } = useEndpointConfiguration()
  const bodyIsValid = useEndpointConfiguration(useShallow(getBodyIsValidSelector))
  const body = useEndpointConfiguration(getBodySelector)
  const previousSchemaRef = useRef<Record<string, any> | undefined>(schema)
  const userClearedRef = useRef(false)

  useEffect(() => {
    const schemaChanged = previousSchemaRef.current !== schema
    previousSchemaRef.current = schema

    if (schemaChanged) {
      userClearedRef.current = false
    }

    if (schema && !body && !userClearedRef.current) {
      setBody(JSON.stringify(convertSchemaToJson(schema), null, 2))
      return
    }

    if (!schema) {
      setBody(body || '')
      setBodyIsValid(true)
    }
  }, [schema, body, setBody, setBodyIsValid])

  const handleBodyChange = useCallback(
    (value: string) => {
      setBody(value)
      const isEmptyWithSchema = schema && !value
      if (isEmptyWithSchema) {
        setBodyIsValid(false)
        userClearedRef.current = true
      } else {
        userClearedRef.current = false
      }
    },
    [setBody, schema, setBodyIsValid],
  )

  return (
    <div className="max-h-full h-full relative">
      <JsonEditor value={body} schema={schema} onChange={handleBodyChange} onValidate={setBodyIsValid} />
      {bodyIsValid === false && (
        <div
          className="absolute bottom-0 left-0 right-0 border-t border-border p-3 text-sm dark:text-yellow-500 text-yellow-700 flex items-center gap-1 font-medium"
          data-testid="endpoint-body-tab-invalid"
        >
          <CircleX className="w-4 h-4" />
          The body payload is invalid
        </div>
      )}
    </div>
  )
})

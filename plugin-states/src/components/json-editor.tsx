import Editor, { useMonaco } from '@monaco-editor/react'
import { useThemeStore } from '@motiadev/ui'
import { type FC, useEffect, useMemo } from 'react'

type JsonEditorProps = {
  value: string
  height?: number | string
  schema?: Record<string, unknown>
  onChange?: (value: string) => void
  onValidate?: (isValid: boolean) => void
  language?: string
  readOnly?: boolean
}

export const JsonEditor: FC<JsonEditorProps> = ({
  value,
  height = 300,
  schema,
  onChange,
  onValidate,
  language = 'json',
  readOnly = false,
}) => {
  const monaco = useMonaco()
  const theme = useThemeStore((state) => state.theme)
  const editorTheme = useMemo(() => (theme === 'dark' ? 'vs-dark' : 'light'), [theme])

  useEffect(() => {
    if (!monaco) return

    monaco.languages.typescript.javascriptDefaults.setCompilerOptions({ isolatedModules: true })
    monaco.languages.json.jsonDefaults.setDiagnosticsOptions({
      schemas: schema
        ? [
            {
              uri: window.location.href,
              fileMatch: ['*'],
              schema,
            },
          ]
        : [],
    })
  }, [monaco, schema, language])

  return (
    <Editor
      data-testid="json-editor"
      height={height}
      language={language}
      value={value}
      theme={editorTheme}
      onChange={(value) => {
        if (!value) {
          onValidate?.(false)
        }
        onChange?.(value ?? '')
      }}
      onValidate={(markers) => onValidate?.(markers.length === 0)}
      options={{
        readOnly,
        scrollBeyondLastLine: false,
        minimap: { enabled: false },
      }}
    />
  )
}

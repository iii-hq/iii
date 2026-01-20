import { Editor, useMonaco } from '@monaco-editor/react'
import { useThemeStore } from '@motiadev/ui'
import { type FC, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'

type JsonEditorProps = {
  value: string
  schema?: Record<string, unknown>
  onChange?: (value: string) => void
  onValidate?: (isValid: boolean) => void
  language?: 'json' | string
  readOnly?: boolean
}

export const JsonEditor: FC<JsonEditorProps> = ({
  value,
  schema,
  onChange,
  onValidate,
  language = 'json',
  readOnly = false,
}) => {
  const monaco = useMonaco()
  const theme = useThemeStore((state: { theme: string }) => state.theme)
  const editorTheme = useMemo(() => (theme === 'dark' ? 'transparent-dark' : 'transparent-light'), [theme])
  const [editor, setEditor] = useState<any>(null)
  const resizeAnimationFrameRef = useRef<number | null>(null)
  const isValidatingRef = useRef(false)

  useLayoutEffect(() => {
    if (!monaco) return

    monaco.editor.defineTheme('transparent-light', {
      base: 'vs',
      inherit: true,
      rules: [],
      colors: {
        'editor.background': '#00000000',
        'editor.lineHighlightBackground': '#00000000',
        'editorLineNumber.foreground': '#999999',
        'editorLineNumber.activeForeground': '#000000',
        focusBorder: '#00000000',
        'widget.border': '#00000000',
        'editor.border': '#00000000',
        'editorWidget.border': '#00000000',
      },
    })
    monaco.editor.defineTheme('transparent-dark', {
      base: 'vs-dark',
      inherit: true,
      rules: [],
      colors: {
        'editor.background': '#00000000',
        'editor.lineHighlightBackground': '#00000000',
        'editorLineNumber.foreground': '#666666',
        'editorLineNumber.activeForeground': '#ffffff',
        focusBorder: '#00000000',
        'widget.border': '#00000000',
        'editor.border': '#00000000',
        'editorWidget.border': '#00000000',
      },
    })
  }, [monaco])

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
  }, [monaco, schema])

  useLayoutEffect(() => {
    if (!monaco) return
    monaco.editor.setTheme(editorTheme)
  }, [monaco, editorTheme])

  useEffect(() => {
    if (!editor) return

    const container = editor.getContainerDomNode().parentElement?.parentElement
    if (!container) return

    const handleResize = () => {
      if (resizeAnimationFrameRef.current !== null) {
        cancelAnimationFrame(resizeAnimationFrameRef.current)
      }

      resizeAnimationFrameRef.current = requestAnimationFrame(() => {
        const { width, height } = container.getBoundingClientRect()
        editor.layout({ width, height })
        resizeAnimationFrameRef.current = null
      })
    }

    handleResize()

    const resizeObserver = new ResizeObserver(handleResize)
    resizeObserver.observe(container)

    return () => {
      resizeObserver.disconnect()
      if (resizeAnimationFrameRef.current !== null) {
        cancelAnimationFrame(resizeAnimationFrameRef.current)
      }
    }
  }, [editor])

  useEffect(() => {
    if (!editor || !monaco || !onValidate || isValidatingRef.current) return

    const model = editor.getModel()
    if (!model) return

    const isEmptyWithSchema = schema && !value
    if (isEmptyWithSchema) {
      isValidatingRef.current = true
      onValidate(false)
      isValidatingRef.current = false
      return
    }

    isValidatingRef.current = true
    const timeoutId = setTimeout(() => {
      const markers = monaco.editor.getModelMarkers({ resource: model.uri })
      const isValid = markers.length === 0
      onValidate(isValid)
      isValidatingRef.current = false
    }, 100)

    return () => {
      clearTimeout(timeoutId)
      isValidatingRef.current = false
    }
  }, [editor, monaco, onValidate, value, schema])

  const editorKey = useMemo(() => (schema ? JSON.stringify(schema) : 'no-schema'), [schema])

  return (
    <Editor
      key={editorKey}
      data-testid="json-editor"
      language={language}
      value={value}
      loading=""
      theme={editorTheme}
      onMount={setEditor}
      onChange={(value: string | undefined) => {
        onChange?.(value ?? '')
      }}
      onValidate={(markers: any[]) => {
        if (!onValidate || isValidatingRef.current) return
        isValidatingRef.current = true
        const isEmptyWithSchema = schema && !value
        if (isEmptyWithSchema) {
          onValidate(false)
        } else {
          onValidate(markers.length === 0)
        }
        isValidatingRef.current = false
      }}
      options={{
        automaticLayout: false,
        readOnly,
        scrollBeyondLastLine: false,
        minimap: { enabled: false },
        overviewRulerLanes: 0,
      }}
    />
  )
}

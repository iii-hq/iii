import { Button, useThemeStore } from '@motiadev/ui'
import { Download } from 'lucide-react'
import type React from 'react'
import { Prism as SyntaxHighlighter } from 'react-syntax-highlighter'
import { atomDark, oneLight } from 'react-syntax-highlighter/dist/esm/styles/prism'

type CodeDisplayProps = {
  code: string
  blob: Blob | undefined
  contentType: string
}

const codeTagProps = {
  style: {
    fontFamily: 'DM Mono, monospace',
    fontSize: '16px',
  },
}

const customStyle = {
  margin: 0,
  borderRadius: 0,
  padding: 0,
}

const useLanguage = (contentType: string) => {
  if (contentType?.includes('json')) {
    return 'json'
  }
  if (contentType?.includes('html')) {
    return 'html'
  }
  return 'text'
}

export const ResponseContent: React.FC<CodeDisplayProps> = ({ code, blob, contentType }) => {
  const theme = useThemeStore((state) => state.theme)
  const themeStyle = theme === 'dark' ? atomDark : oneLight
  const language = useLanguage(contentType)

  const onDownload = () => {
    if (blob) {
      const url = URL.createObjectURL(blob)
      window.open(url, '_blank')
    }
  }

  return (
    <div className="flex flex-col h-full overflow-y-auto">
      {blob && (
        <div className="flex p-8 justify-center">
          <Button variant="default" onClick={onDownload}>
            <Download className="h-4 w-4" /> Download
          </Button>
        </div>
      )}

      {!blob && code && (
        <SyntaxHighlighter
          showLineNumbers
          language={language}
          style={themeStyle}
          codeTagProps={codeTagProps}
          customStyle={customStyle}
          wrapLines
        >
          {code}
        </SyntaxHighlighter>
      )}
    </div>
  )
}

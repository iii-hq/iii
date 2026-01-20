import { Button, cn } from '@motiadev/ui'
import { Check, Copy } from 'lucide-react'
import { type FC, useState } from 'react'
import { usePathUrl } from '../hooks/use-path-url'

interface EndpointPathPreviewProps {
  path: string
  baseUrl?: string
}

export const EndpointPathPreview: FC<EndpointPathPreviewProps> = ({ path, baseUrl = window.location.origin }) => {
  const pathUrl = usePathUrl(path)
  const fullUrl = `${baseUrl}${pathUrl}`
  const [copied, setCopied] = useState(false)

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(fullUrl)
      setCopied(true)
      setTimeout(() => setCopied(false), 2000)
    } catch (err) {
      console.error('Failed to copy URL:', err)
    }
  }

  return (
    <div className="grid grid-cols-[auto_1fr] items-center gap-5 px-5 relative border-b border-border">
      <div className="text-sm font-medium py-2">URL Preview</div>

      <div className="bg-muted-foreground/10 box-border grid grid-cols-[1fr_auto] gap-1 h-6 items-center px-2 py-1 rounded border border-border">
        <div className="min-w-0">
          <div className="font-medium text-xs text-muted-foreground truncate" title={fullUrl}>
            {fullUrl}
          </div>
        </div>

        <Button
          onClick={handleCopy}
          className={cn(
            'w-3 h-3 grid place-items-center transition-colors cursor-pointer',
            copied ? 'text-green-400' : 'text-muted-foreground',
          )}
          variant="icon"
          size="sm"
          aria-label="Copy URL"
        >
          {copied ? <Check className="w-2.5 h-2.5" /> : <Copy className="w-2.5 h-2.5" />}
        </Button>
      </div>
    </div>
  )
}

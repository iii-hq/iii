import { Button, cn } from '@motiadev/ui'
import { Check, Copy } from 'lucide-react'
import { useCallback, useState } from 'react'

type CopyButtonProps = {
  textToCopy: string
  className?: string
  title?: string
}

export const CopyButton: React.FC<CopyButtonProps> = ({ textToCopy, className, title }) => {
  const [copied, setCopied] = useState(false)

  const handleCopy = useCallback(
    async (e: React.MouseEvent) => {
      e.stopPropagation()
      try {
        await navigator.clipboard.writeText(textToCopy)
        setCopied(true)
        setTimeout(() => setCopied(false), 2000)
      } catch (error) {
        console.error('Failed to copy:', error)
      }
    },
    [textToCopy],
  )

  return (
    <Button
      variant="icon"
      size="icon"
      onClick={handleCopy}
      className={cn('cursor-pointer p-3 rounded-full', className)}
      title={title ?? (copied ? 'Copied!' : 'Copy')}
    >
      {copied ? <Check className="w-4 h-4 text-green-500" /> : <Copy className="w-4 h-4 text-gray-400" />}
    </Button>
  )
}

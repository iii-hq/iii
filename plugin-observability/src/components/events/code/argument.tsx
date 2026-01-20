import { memo, useMemo } from 'react'
import ReactJson from 'react18-json-view'
import { CopyButton } from '../../../components/ui/copy-button'
import { Popover, PopoverContent, PopoverTrigger } from '../../../components/ui/popover'
import 'react18-json-view/src/dark.css'
import 'react18-json-view/src/style.css'
import { formatArgumentValue } from './utils'

type ArgumentProps = {
  arg: string | object | false
  popover?: boolean
}

export const Argument: React.FC<ArgumentProps> = memo(({ arg, popover = true }) => {
  const isObject = useMemo(() => typeof arg === 'object' && arg !== null, [arg])

  const value = useMemo(() => formatArgumentValue(arg), [arg])

  const textToCopy = useMemo(() => JSON.stringify(arg, null, 2), [arg])

  if (!popover) {
    return (
      <>
        {isObject ? (
          <ReactJson src={arg} theme="default" enableClipboard={false} style={{ padding: 0 }} />
        ) : (
          <span className="font-mono text-blue-500">{value}</span>
        )}
      </>
    )
  }

  return (
    <Popover>
      <PopoverTrigger asChild>
        <span className="truncate font-mono text-blue-500 cursor-pointer inline-block">{value}</span>
      </PopoverTrigger>
      <PopoverContent
        className="backdrop-blur-md w-auto max-w-2xl relative max-h-[50vh] overflow-y-auto"
        hideWhenDetached
      >
        {isObject ? (
          <div>
            <CopyButton textToCopy={textToCopy} className="absolute top-4 right-4" title="Copy JSON" />
            <ReactJson src={arg} theme="default" enableClipboard={false} style={{ padding: 0 }} />
          </div>
        ) : (
          <div className="flex items-center justify-between">
            <span>{value}</span>
            <CopyButton textToCopy={textToCopy} className="ml-2" title="Copy JSON" />
          </div>
        )}
      </PopoverContent>
    </Popover>
  )
})
Argument.displayName = 'Argument'

import { cn } from '@motiadev/ui'
import { FileJson } from 'lucide-react'
import { memo, useCallback, useRef, useState } from 'react'
import JsonView from 'react18-json-view'
import {
  ChevronDownIcon,
  type ChevronDownIconHandle,
  ChevronRightIcon,
  CopyIcon,
  FileTextIcon,
} from './lucide-animated'
import 'react18-json-view/src/style.css'
import type { StreamItem } from '../types/stream'

interface StreamItemsTableProps {
  items: StreamItem[]
  streamName: string
  groupId: string
}

interface ItemRowProps {
  item: StreamItem
  index: number
}

const formatDate = (timestamp?: number | string): string => {
  if (!timestamp) return '-'
  const date = new Date(timestamp)
  return date.toLocaleString('en-US', {
    month: 'short',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  })
}

const getStatusColor = (status?: string): string => {
  if (!status) return 'bg-muted text-muted-foreground'

  switch (status.toLowerCase()) {
    case 'active':
    case 'completed':
    case 'success':
    case 'resolved':
      return 'bg-green-500/20 text-green-500'
    case 'pending':
    case 'processing':
    case 'in_progress':
      return 'bg-yellow-500/20 text-yellow-500'
    case 'failed':
    case 'error':
    case 'rejected':
      return 'bg-red-500/20 text-red-500'
    case 'escalated':
    case 'urgent':
      return 'bg-orange-500/20 text-orange-500'
    default:
      return 'bg-muted text-muted-foreground'
  }
}

const ItemRow = memo<ItemRowProps>(({ item, index }) => {
  const [isExpanded, setIsExpanded] = useState(false)
  const chevronRef = useRef<ChevronDownIconHandle>(null)

  const toggleExpanded = useCallback(() => {
    chevronRef.current?.startAnimation()
    setIsExpanded((prev) => !prev)
  }, [])

  const copyToClipboard = useCallback((text: string) => {
    navigator.clipboard.writeText(text)
  }, [])

  const data = item as unknown as Record<string, unknown>
  const id = (data.id as string) || '-'

  const status = (data.status as string) || (data.priority as string) || undefined

  const message =
    (data.lastMessage as string) ||
    (data.message as string) ||
    (data.reason as string) ||
    (data.metric as string) ||
    (data.description as string) ||
    (data.name as string) ||
    '-'

  const updatedAt =
    (data.updatedAt as string | number) ||
    (data.timestamp as string | number) ||
    (data._createdAt as string | number) ||
    (data.createdAt as string | number) ||
    undefined

  const value = data.value as number | string | undefined

  return (
    <>
      <tr
        onClick={toggleExpanded}
        className={cn(
          'cursor-pointer transition-colors group',
          isExpanded ? 'bg-muted/50' : 'hover:bg-muted/30',
          index % 2 === 0 ? 'bg-background' : 'bg-card/30',
        )}
      >
        <td className="px-3 py-2 w-8">
          {isExpanded ? (
            <ChevronDownIcon ref={chevronRef} size={16} className="text-muted-foreground" />
          ) : (
            <ChevronRightIcon size={16} className="text-muted-foreground" />
          )}
        </td>
        <td className="px-3 py-2 text-xs text-muted-foreground whitespace-nowrap">{formatDate(updatedAt)}</td>
        <td className="px-3 py-2">
          <div className="flex items-center gap-1.5">
            <span className="text-sm font-mono text-foreground truncate max-w-[180px]" title={id}>
              {id}
            </span>
            <button
              type="button"
              onClick={(e) => {
                e.stopPropagation()
                copyToClipboard(id)
              }}
              className="p-1 text-muted-foreground hover:text-foreground opacity-0 group-hover:opacity-100 transition-opacity"
              title="Copy ID"
            >
              <CopyIcon size={12} />
            </button>
          </div>
        </td>
        <td className="px-3 py-2">
          {status ? (
            <span className={cn('px-2 py-1 text-xs font-medium rounded-full capitalize', getStatusColor(status))}>
              {status}
            </span>
          ) : value !== undefined ? (
            <span className="px-2 py-1 text-xs font-medium rounded-full bg-blue-500/20 text-blue-500">{value}</span>
          ) : null}
        </td>
        <td className="px-3 py-2 text-sm text-muted-foreground truncate max-w-[300px]" title={message}>
          {message}
        </td>
      </tr>

      {isExpanded && (
        <tr className="bg-muted/30">
          <td colSpan={5} className="p-4">
            <div className="bg-background rounded-md border border-border p-3">
              <div className="flex items-center gap-2 mb-2 text-xs text-muted-foreground">
                <FileTextIcon size={16} />
                <span>Full Data</span>
              </div>
              <JsonView src={data} theme="atom" dark collapsed={2} style={{ fontSize: '12px' }} />
            </div>
          </td>
        </tr>
      )}
    </>
  )
})
ItemRow.displayName = 'ItemRow'

export const StreamItemsTable = memo<StreamItemsTableProps>(({ items, groupId }) => {
  if (items.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center h-full p-8 text-center">
        <FileJson className="h-12 w-12 text-muted-foreground/50 mb-4" />
        <p className="text-sm text-muted-foreground">No items in this group</p>
        <p className="text-xs text-muted-foreground/70 mt-1">
          Items will appear here when data is written to "{groupId}"
        </p>
      </div>
    )
  }

  return (
    <div className="flex flex-col h-full overflow-hidden">
      <div className="flex items-center justify-between px-4 py-2 border-b border-border bg-card/50 shrink-0">
        <div className="flex items-center gap-2 text-sm text-muted-foreground">
          <span className="font-medium text-foreground">{items.length}</span>
          <span>items in</span>
          <span className="font-mono text-xs bg-muted px-2 py-0.5 rounded">{groupId}</span>
        </div>
      </div>

      <div className="shrink-0 border-b border-border" style={{ backgroundColor: 'hsl(var(--card))' }}>
        <table className="w-full text-sm">
          <thead>
            <tr>
              <th className="px-3 py-2 w-8" style={{ backgroundColor: 'hsl(var(--card))' }}></th>
              <th
                className="px-3 py-2 text-left text-xs font-medium text-muted-foreground uppercase tracking-wider"
                style={{ backgroundColor: 'hsl(var(--card))' }}
              >
                Date
              </th>
              <th
                className="px-3 py-2 text-left text-xs font-medium text-muted-foreground uppercase tracking-wider"
                style={{ backgroundColor: 'hsl(var(--card))' }}
              >
                ID
              </th>
              <th
                className="px-3 py-2 text-left text-xs font-medium text-muted-foreground uppercase tracking-wider"
                style={{ backgroundColor: 'hsl(var(--card))' }}
              >
                Status
              </th>
              <th
                className="px-3 py-2 text-left text-xs font-medium text-muted-foreground uppercase tracking-wider"
                style={{ backgroundColor: 'hsl(var(--card))' }}
              >
                Message
              </th>
            </tr>
          </thead>
        </table>
      </div>

      <div className="flex-1 overflow-auto">
        <table className="w-full text-sm">
          <tbody className="divide-y divide-border/50">
            {items.map((item, index) => (
              <ItemRow
                key={((item as unknown as Record<string, unknown>).id as string) || index}
                item={item}
                index={index}
              />
            ))}
          </tbody>
        </table>
      </div>
    </div>
  )
})
StreamItemsTable.displayName = 'StreamItemsTable'

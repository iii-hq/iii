import { cn } from '@motiadev/ui'
import { Database } from 'lucide-react'
import { memo, useCallback, useRef, useState } from 'react'
import { useStreams } from '../hooks/use-streams'
import { useStreamsStore } from '../stores/use-streams-store'
import type { StreamInfo } from '../types/stream'
import { ChevronDownIcon, type ChevronDownIconHandle, ChevronRightIcon, LoaderPinwheelIcon } from './lucide-animated'
import { StreamEmptyState } from './stream-empty-state'

interface StreamItemProps {
  stream: StreamInfo & { id: string }
  isSelected: boolean
  onSelect: (streamId: string) => void
}

const StreamItem = memo<StreamItemProps>(({ stream, isSelected, onSelect }) => {
  const handleClick = useCallback(() => {
    onSelect(stream.id)
  }, [stream.id, onSelect])

  return (
    <button
      type="button"
      onClick={handleClick}
      className={cn(
        'w-full flex items-center gap-2 px-3 py-2 text-left transition-colors rounded-md mx-2',
        'hover:bg-muted/50',
        isSelected ? 'bg-primary/10 text-primary border-l-2 border-primary' : 'text-foreground/80',
      )}
    >
      <Database className={cn('h-4 w-4 shrink-0', isSelected ? 'text-primary' : 'text-muted-foreground')} />
      <div className="flex-1 min-w-0">
        <div className={cn('text-sm font-medium truncate', isSelected && 'text-primary')}>{stream.name}</div>
        {stream.filePath && (
          <div className="text-xs text-muted-foreground truncate">{stream.filePath.split('/').pop()}</div>
        )}
      </div>
      {stream.hidden && (
        <span className="text-[10px] bg-muted text-muted-foreground px-1.5 py-0.5 rounded">hidden</span>
      )}
    </button>
  )
})
StreamItem.displayName = 'StreamItem'

interface StreamGroupProps {
  groupName: string
  streams: (StreamInfo & { id: string })[]
  selectedStreamId: string | null
  onSelect: (streamId: string) => void
  defaultExpanded?: boolean
}

const StreamGroup = memo<StreamGroupProps>(
  ({ groupName, streams, selectedStreamId, onSelect, defaultExpanded = true }) => {
    const [isExpanded, setIsExpanded] = useState(defaultExpanded)
    const chevronRef = useRef<ChevronDownIconHandle>(null)

    const toggleExpanded = useCallback(() => {
      chevronRef.current?.startAnimation()
      setIsExpanded((prev) => !prev)
    }, [])

    return (
      <div className="mb-1">
        <button
          type="button"
          onClick={toggleExpanded}
          className="w-full flex items-center gap-2 px-3 py-2 text-xs font-semibold text-muted-foreground uppercase tracking-wider hover:bg-muted/30 transition-colors"
        >
          {isExpanded ? <ChevronDownIcon ref={chevronRef} size={14} /> : <ChevronRightIcon size={14} />}
          <span>{groupName}</span>
          <span className="ml-auto text-[10px] bg-muted px-1.5 py-0.5 rounded-full">{streams.length}</span>
        </button>

        {isExpanded && (
          <div className="space-y-0.5 pb-2">
            {streams.map((stream) => (
              <StreamItem
                key={stream.id}
                stream={stream}
                isSelected={selectedStreamId === stream.id}
                onSelect={onSelect}
              />
            ))}
          </div>
        )}
      </div>
    )
  },
)
StreamGroup.displayName = 'StreamGroup'

export const StreamList = memo(() => {
  const selectedStreamId = useStreamsStore((state) => state.selectedStreamId)
  const selectStream = useStreamsStore((state) => state.selectStream)

  const { groupedStreams, visibleCount, loading, error } = useStreams()

  const handleSelect = useCallback(
    (streamId: string) => {
      selectStream(streamId === selectedStreamId ? null : streamId)
    },
    [selectStream, selectedStreamId],
  )

  if (loading) {
    return (
      <div className="flex flex-col items-center justify-center h-full p-8 border-r border-border bg-card/50">
        <LoaderPinwheelIcon size={32} className="text-muted-foreground" />
        <p className="mt-2 text-sm text-muted-foreground">Loading streams...</p>
      </div>
    )
  }

  if (error) {
    return (
      <div className="flex flex-col items-center justify-center h-full p-8 border-r border-border bg-card/50">
        <p className="text-sm text-destructive">{error}</p>
      </div>
    )
  }

  if (visibleCount === 0) {
    return (
      <div className="border-r border-border bg-card/50">
        <StreamEmptyState />
      </div>
    )
  }

  const groupNames = Object.keys(groupedStreams).sort((a, b) => {
    if (a === 'Internal') return 1
    if (b === 'Internal') return -1
    return a.localeCompare(b)
  })

  return (
    <div className="flex flex-col h-full overflow-hidden border-r border-border bg-card/50">
      <div className="flex-1 overflow-y-auto py-2">
        {groupNames.map((groupName) => (
          <StreamGroup
            key={groupName}
            groupName={groupName}
            streams={groupedStreams[groupName]}
            selectedStreamId={selectedStreamId}
            onSelect={handleSelect}
            defaultExpanded={groupName !== 'Internal'}
          />
        ))}
      </div>
    </div>
  )
})
StreamList.displayName = 'StreamList'

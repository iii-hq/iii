import { cn } from '@motiadev/ui'
import { memo } from 'react'
import { useStreamsStore } from '../stores/use-streams-store'
import { StreamDetail } from './stream-detail'
import { StreamList } from './stream-list'
import { StreamsHeader } from './streams-header'

export const StreamsPage = memo(() => {
  const selectedStreamId = useStreamsStore((state) => state.selectedStreamId)

  return (
    <div className="flex flex-col h-full max-h-full bg-background">
      <StreamsHeader />
      <div className={cn('flex-1 grid overflow-hidden', selectedStreamId ? 'grid-cols-[280px_1fr]' : 'grid-cols-1')}>
        <StreamList />
        {selectedStreamId && <StreamDetail />}
      </div>
    </div>
  )
})
StreamsPage.displayName = 'StreamsPage'

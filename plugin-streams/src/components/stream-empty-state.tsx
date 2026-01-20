import { Database } from 'lucide-react'
import { memo } from 'react'

export const StreamEmptyState = memo(() => {
  return (
    <div className="flex flex-col items-center justify-center h-full p-8 text-center">
      <div className="p-4 rounded-full bg-muted/50 mb-4">
        <Database className="h-8 w-8 text-muted-foreground" />
      </div>
      <h3 className="text-lg font-semibold text-foreground mb-2">No Streams Found</h3>
      <p className="text-sm text-muted-foreground max-w-[280px]">
        Streams will appear here when you create them in your Motia application.
      </p>
      <div className="mt-4 text-xs text-muted-foreground/70">
        <p>Create a stream file with:</p>
        <code className="mt-1 block px-2 py-1 bg-muted rounded text-xs font-mono">*.stream.ts</code>
      </div>
    </div>
  )
})
StreamEmptyState.displayName = 'StreamEmptyState'

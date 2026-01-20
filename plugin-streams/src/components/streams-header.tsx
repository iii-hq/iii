import { cn } from '@motiadev/ui'
import { Database } from 'lucide-react'
import { memo, useCallback, useRef, useState } from 'react'
import { useStreams } from '../hooks/use-streams'
import { useStreamsStore } from '../stores/use-streams-store'
import {
  EyeIcon,
  EyeOffIcon,
  RefreshCwIcon,
  type RefreshCwIconHandle,
  SearchIcon,
  type SearchIconHandle,
  XIcon,
} from './lucide-animated'

export const StreamsHeader = memo(() => {
  const search = useStreamsStore((state) => state.search)
  const setSearch = useStreamsStore((state) => state.setSearch)
  const showHidden = useStreamsStore((state) => state.showHidden)
  const toggleShowHidden = useStreamsStore((state) => state.toggleShowHidden)

  const { totalCount, visibleCount, refetch, loading } = useStreams()
  const [, setSearchFocused] = useState(false)

  const searchIconRef = useRef<SearchIconHandle>(null)
  const refreshIconRef = useRef<RefreshCwIconHandle>(null)

  const handleClear = useCallback(() => {
    setSearch('')
  }, [setSearch])

  const handleChange = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      setSearch(e.target.value)
    },
    [setSearch],
  )

  const handleRefresh = useCallback(() => {
    refreshIconRef.current?.startAnimation()
    refetch()
  }, [refetch])

  return (
    <div className="flex items-center gap-3 px-4 py-3 border-b border-border bg-card">
      <div className="flex items-center gap-2 text-foreground">
        <Database className="h-5 w-5 text-primary" />
        <h1 className="text-lg font-semibold">Streams</h1>
        <span className="text-xs text-muted-foreground bg-muted px-2 py-0.5 rounded-full">
          {visibleCount} / {totalCount}
        </span>
      </div>

      <div className="flex-1 max-w-md">
        <div className="relative">
          <SearchIcon
            ref={searchIconRef}
            className="absolute left-3 top-1/2 -translate-y-1/2 text-muted-foreground"
            size={16}
          />
          <input
            type="text"
            placeholder="Search streams..."
            value={search}
            onChange={handleChange}
            onFocus={() => {
              setSearchFocused(true)
              searchIconRef.current?.startAnimation()
            }}
            onBlur={() => {
              setSearchFocused(false)
              searchIconRef.current?.stopAnimation()
            }}
            className="w-full pl-9 pr-9 py-2 text-sm bg-background border border-border rounded-md focus:outline-none focus:ring-2 focus:ring-ring focus:border-transparent placeholder:text-muted-foreground"
          />
          {search && (
            <button
              type="button"
              onClick={handleClear}
              className="absolute right-3 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground"
            >
              <XIcon size={16} />
            </button>
          )}
        </div>
      </div>

      <div className="flex items-center gap-2">
        <button
          type="button"
          onClick={toggleShowHidden}
          className={cn(
            'flex items-center gap-1.5 px-3 py-2 text-xs rounded-md transition-colors',
            showHidden
              ? 'bg-primary/10 text-primary border border-primary/20'
              : 'bg-muted text-muted-foreground hover:bg-muted/80',
          )}
          title={showHidden ? 'Hide internal streams' : 'Show internal streams'}
        >
          {showHidden ? <EyeIcon size={14} /> : <EyeOffIcon size={14} />}
          {showHidden ? 'Showing all' : 'Hidden: internal'}
        </button>

        <button
          type="button"
          onClick={handleRefresh}
          disabled={loading}
          className={cn(
            'p-2 rounded-md transition-colors',
            'bg-muted text-muted-foreground hover:bg-muted/80 hover:text-foreground',
            loading && 'opacity-50 cursor-not-allowed',
          )}
          title="Refresh streams"
        >
          <RefreshCwIcon ref={refreshIconRef} size={16} />
        </button>
      </div>
    </div>
  )
})
StreamsHeader.displayName = 'StreamsHeader'

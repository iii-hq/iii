import { Button, cn, Input } from '@motiadev/ui'
import { Search, Trash, X } from 'lucide-react'
import { memo } from 'react'
import { useObservabilityStore } from '../stores/use-observability-store'

export const SearchBar = memo(() => {
  const search = useObservabilityStore((state) => state.search)
  const setSearch = useObservabilityStore((state) => state.setSearch)
  const clearTraces = useObservabilityStore((state) => state.clearTraces)

  return (
    <div className="flex p-2 border-b gap-2" data-testid="logs-search-container">
      <div className="flex-1 relative">
        <Input
          variant="shade"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          className="px-9! font-medium"
          placeholder="Search by Trace ID or Step Name"
        />
        <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-muted-foreground/50" />
        <X
          className={cn(
            'cursor-pointer absolute right-3 top-1/2 -translate-y-1/2 w-4 h-4 text-muted-foreground/50 hover:text-muted-foreground',
            {
              visible: search !== '',
              invisible: search === '',
            },
          )}
          onClick={() => setSearch('')}
        />
      </div>
      <Button variant="default" onClick={clearTraces} className="h-[34px]">
        <Trash /> Clear
      </Button>
    </div>
  )
})

SearchBar.displayName = 'SearchBar'

import { cn, Input } from '@motiadev/ui'
import { Search, X } from 'lucide-react'

type EndpointsSearchProps = {
  value: string
  onChange: (value: string) => void
  onClear: () => void
}

export const EndpointsSearch = ({ value, onChange, onClear }: EndpointsSearchProps) => {
  return (
    <div className="p-2 border-b gap-4" data-testid="endpoints-search-container">
      <div className="relative">
        <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-muted-foreground/50" />
        <Input
          variant="shade"
          value={value}
          onChange={(e) => onChange(e.target.value)}
          className="px-9! font-medium"
          placeholder="Search by Method or Path"
        />
        <X
          className={cn(
            'cursor-pointer absolute right-3 top-1/2 -translate-y-1/2 w-4 h-4 text-muted-foreground/50 hover:text-muted-foreground',
            {
              visible: value !== '',
              invisible: value === '',
            },
          )}
          onClick={onClear}
        />
      </div>
    </div>
  )
}

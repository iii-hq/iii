import { Search, X } from 'lucide-react'
import { Input } from '@/components/ui/card'

interface SearchBarProps {
  value: string
  onChange: (value: string) => void
  placeholder: string
}

export function SearchBar({ value, onChange, placeholder }: SearchBarProps) {
  return (
    <div className="flex items-center gap-2 p-2 border-b border-border bg-dark-gray/20">
      <div className="flex-1 relative">
        <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-muted" />
        <Input
          value={value}
          onChange={(e) => onChange(e.target.value)}
          className="pl-9 pr-9 h-9"
          placeholder={placeholder}
        />
        {value && (
          <button
            type="button"
            onClick={() => onChange('')}
            className="absolute right-3 top-1/2 -translate-y-1/2 text-muted hover:text-foreground"
          >
            <X className="w-4 h-4" />
          </button>
        )}
      </div>
    </div>
  )
}

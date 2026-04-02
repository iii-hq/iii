import type * as React from 'react'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'

interface EmptyStateProps {
  icon?: React.ComponentType<{ className?: string }>
  title: string
  description: string
  variant?: 'default' | 'success'
  action?: {
    label: string
    onClick: () => void
  }
  className?: string
}

function EmptyState({
  icon: Icon,
  title,
  description,
  variant = 'default',
  action,
  className,
}: EmptyStateProps) {
  return (
    <div
      className={cn(
        'flex flex-col items-center justify-center text-center max-w-[320px] mx-auto py-12',
        className,
      )}
    >
      {Icon && (
        <div className="mb-4">
          <Icon
            className={cn('h-10 w-10', variant === 'success' ? 'text-success' : 'text-muted')}
          />
        </div>
      )}
      <h3 className="font-sans font-semibold text-base text-foreground">{title}</h3>
      <p className="mt-1 font-sans text-[13px] text-secondary line-clamp-2">{description}</p>
      {action && (
        <div className="mt-4">
          <Button variant="outline" size="sm" onClick={action.onClick} type="button">
            {action.label}
          </Button>
        </div>
      )}
    </div>
  )
}

export { EmptyState }
export type { EmptyStateProps }

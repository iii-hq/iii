import type { LucideIcon } from 'lucide-react'
import type { ReactNode } from 'react'

interface PageHeaderProps {
  icon: LucideIcon
  title: string
  children?: ReactNode
  actions?: ReactNode
}

export function PageHeader({ icon: Icon, title, children, actions }: PageHeaderProps) {
  return (
    <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-2 px-3 md:px-5 py-3 md:py-4 bg-dark-gray/30 border-b border-border">
      <div className="flex items-center gap-2 md:gap-4 flex-wrap">
        <h1 className="font-sans font-semibold text-lg tracking-tight flex items-center gap-2">
          <Icon className="w-5 h-5" />
          {title}
        </h1>
        {children}
      </div>

      {actions && <div className="flex items-center gap-1.5 md:gap-2">{actions}</div>}
    </div>
  )
}

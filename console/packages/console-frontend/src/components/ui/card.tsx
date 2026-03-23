import * as React from 'react'
import { cn } from '@/lib/utils'

export { cn }

export const Card = React.forwardRef<HTMLDivElement, React.HTMLAttributes<HTMLDivElement>>(
  ({ className, ...props }, ref) => (
    <div
      ref={ref}
      className={cn(
        'rounded-[var(--radius-lg)] border border-border-subtle bg-elevated text-foreground',
        className,
      )}
      {...props}
    />
  ),
)
Card.displayName = 'Card'

export const CardHeader = React.forwardRef<HTMLDivElement, React.HTMLAttributes<HTMLDivElement>>(
  ({ className, ...props }, ref) => (
    <div ref={ref} className={cn('flex flex-col space-y-1 p-4', className)} {...props} />
  ),
)
CardHeader.displayName = 'CardHeader'

export const CardTitle = React.forwardRef<
  HTMLParagraphElement,
  React.HTMLAttributes<HTMLHeadingElement>
>(({ className, children, ...props }, ref) => (
  <h3
    ref={ref}
    className={cn(
      'font-sans font-semibold text-xs uppercase tracking-[0.04em] text-muted',
      className,
    )}
    {...props}
  >
    {children}
  </h3>
))
CardTitle.displayName = 'CardTitle'

export const CardContent = React.forwardRef<HTMLDivElement, React.HTMLAttributes<HTMLDivElement>>(
  ({ className, ...props }, ref) => (
    <div ref={ref} className={cn('p-4 pt-0', className)} {...props} />
  ),
)
CardContent.displayName = 'CardContent'

export function StatCard({
  title,
  value,
  subtitle,
  icon: Icon,
  trend: _trend,
  className,
}: {
  title: string
  value: string | number
  subtitle?: string
  icon?: React.ComponentType<{ className?: string }>
  trend?: 'up' | 'down' | 'neutral'
  className?: string
}) {
  return (
    <Card className={cn('card-interactive', className)}>
      <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
        <CardTitle>{title}</CardTitle>
        {Icon && <Icon className="h-4 w-4 text-muted" />}
      </CardHeader>
      <CardContent>
        <div className="text-2xl font-semibold tracking-tight">{value}</div>
        {subtitle && (
          <p className="font-sans font-semibold text-xs text-muted uppercase tracking-[0.04em] mt-1">
            {subtitle}
          </p>
        )}
      </CardContent>
    </Card>
  )
}

export function Table({ children, className }: { children: React.ReactNode; className?: string }) {
  return (
    <div className={cn('overflow-x-auto', className)}>
      <table className="w-full text-xs">{children}</table>
    </div>
  )
}

export function TableHeader({ children }: { children: React.ReactNode }) {
  return <thead className="border-b border-border-subtle">{children}</thead>
}

export function TableBody({ children }: { children: React.ReactNode }) {
  return <tbody>{children}</tbody>
}

export function TableRow({
  children,
  className,
}: {
  children: React.ReactNode
  className?: string
}) {
  return (
    <tr
      className={cn(
        'border-b border-border-subtle transition-colors hover:bg-white/[0.02]',
        className,
      )}
    >
      {children}
    </tr>
  )
}

export function TableHead({
  children,
  className,
}: {
  children: React.ReactNode
  className?: string
}) {
  return (
    <th
      className={cn(
        'text-left py-3 px-4 font-sans font-semibold text-xs uppercase tracking-[0.04em] text-muted',
        className,
      )}
    >
      {children}
    </th>
  )
}

export function TableCell({
  children,
  className,
}: {
  children: React.ReactNode
  className?: string
}) {
  return <td className={cn('py-3 px-4 text-foreground', className)}>{children}</td>
}

export function Badge({
  children,
  variant = 'default',
  className,
}: {
  children: React.ReactNode
  variant?: 'default' | 'success' | 'warning' | 'error' | 'outline' | 'accent'
  className?: string
}) {
  const variants = {
    default: 'bg-border-subtle text-foreground',
    success: 'bg-success/20 text-success',
    warning: 'bg-warning/20 text-warning',
    error: 'bg-error/20 text-error',
    outline: 'border border-border-subtle text-muted',
    accent: 'bg-accent text-accent-text',
  }

  return (
    <span
      className={cn(
        'inline-flex items-center px-2 py-0.5 rounded-full text-[10px] font-medium tracking-wider uppercase',
        variants[variant],
        className,
      )}
    >
      {children}
    </span>
  )
}

export function Button({
  children,
  variant = 'primary',
  size = 'default',
  className,
  ...props
}: React.ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: 'primary' | 'accent' | 'outline' | 'ghost' | 'destructive'
  size?: 'sm' | 'default' | 'lg'
}) {
  const baseStyles =
    'inline-flex items-center justify-center gap-2 font-medium tracking-wider uppercase transition-all duration-150 whitespace-nowrap cursor-pointer focus:outline-none'

  const variants = {
    primary: 'bg-foreground text-background rounded-[var(--radius-md)] hover:bg-foreground/90',
    accent: 'bg-accent text-accent-text rounded-[var(--radius-md)] hover:bg-accent-hover',
    outline:
      'bg-transparent text-foreground border border-border-subtle rounded-[var(--radius-md)] hover:bg-hover hover:border-muted',
    ghost: 'text-muted hover:text-foreground hover:bg-hover rounded-[var(--radius-md)]',
    destructive: 'bg-error text-white rounded-[var(--radius-md)] hover:bg-error/80',
  }

  const sizes = {
    sm: 'text-[10px] px-3 py-1.5',
    default: 'text-xs px-4 py-2',
    lg: 'text-sm px-6 py-3',
  }

  return (
    <button className={cn(baseStyles, variants[variant], sizes[size], className)} {...props}>
      {children}
    </button>
  )
}

export function Input({ className, ...props }: React.InputHTMLAttributes<HTMLInputElement>) {
  return (
    <input
      className={cn(
        'w-full bg-border-subtle border border-border-subtle rounded-[var(--radius-md)] px-3 py-2 text-xs text-foreground placeholder:text-muted focus:outline-none focus:border-muted transition-colors',
        className,
      )}
      {...props}
    />
  )
}

export function Select({
  children,
  className,
  ...props
}: React.SelectHTMLAttributes<HTMLSelectElement>) {
  return (
    <select
      className={cn(
        'bg-border-subtle border border-border-subtle rounded-[var(--radius-md)] px-3 py-2 text-xs text-foreground focus:outline-none focus:border-muted transition-colors appearance-none cursor-pointer',
        className,
      )}
      {...props}
    >
      {children}
    </select>
  )
}

export function ActionBar({
  children,
  className,
}: {
  children: React.ReactNode
  className?: string
}) {
  return <div className={cn('flex items-center gap-2', className)}>{children}</div>
}

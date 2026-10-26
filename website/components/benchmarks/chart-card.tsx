import { cn } from '@/lib/utils'

/**
 * A chart with what it shows above it and how to read it below, in the bordered card the registry charts
 * use. The reading note is the part that turns a line into an answer: what a reader should look at first.
 */
export function ChartCard({
  title,
  subtitle,
  read,
  className,
  children,
}: {
  title: string
  subtitle?: React.ReactNode
  read?: React.ReactNode
  className?: string
  children: React.ReactNode
}) {
  return (
    <figure className={cn('min-w-0 overflow-hidden rounded-xl border bg-card shadow-xs', className)}>
      <figcaption className="px-5 pt-5 sm:px-6">
        <p className="font-medium text-[15px] text-foreground leading-snug">{title}</p>
        {subtitle ? (
          <p className="mt-1 text-pretty text-[13px] text-muted-foreground leading-snug">{subtitle}</p>
        ) : null}
      </figcaption>
      <div className="px-2 pt-4 pb-2 sm:px-3">{children}</div>
      {read ? (
        <div className="border-t px-5 py-4 text-pretty text-[13px] text-muted-foreground leading-relaxed sm:px-6">
          <span className="font-medium text-foreground">How to read it. </span>
          {read}
        </div>
      ) : null}
    </figure>
  )
}

/** A short statement of what a section found, with its number set apart. */
export function Finding({ figure, children }: { figure: React.ReactNode; children: React.ReactNode }) {
  return (
    <li className="flex gap-4 border-t py-4 first:border-t-0">
      <span className="w-[5.5rem] shrink-0 font-mono text-[13px] text-foreground tabular-nums">{figure}</span>
      <span className="text-pretty text-[14px] text-muted-foreground leading-relaxed">{children}</span>
    </li>
  )
}

import { cn } from '@/lib/utils'

/** The iii mark: three stems, three dots. Inherits `currentColor`. */
export function Logo({ className, width, height }: { className?: string; width?: number; height?: number }) {
  return (
    <svg
      width={width}
      height={height}
      viewBox="0 0 933.61 1050.31"
      aria-hidden="true"
      className={cn('h-4 w-auto fill-current', className)}
    >
      <rect width="233.4" height="233.4" />
      <rect y="350.1" width="233.4" height="700.21" />
      <rect x="350.1" width="233.4" height="233.4" />
      <rect x="350.1" y="350.1" width="233.4" height="700.21" />
      <rect x="700.21" width="233.4" height="233.4" />
      <rect x="700.21" y="350.1" width="233.4" height="700.21" />
    </svg>
  )
}

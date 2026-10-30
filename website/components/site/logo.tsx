import { cn } from '@/lib/utils'

/** The iii mark: three stems, three dots. Inherits `currentColor`. */
export function Logo({ className, width, height }: { className?: string; width?: number; height?: number }) {
  return (
    <svg
      width={width}
      height={height}
      viewBox="0 0 1075.74 1075.74"
      aria-hidden="true"
      className={cn('h-4 w-auto fill-current', className)}
    >
      <rect y="0.05" width="268.94" height="268.94" />
      <rect y="403.45" width="268.94" height="672.24" />
      <rect x="403.4" y="0.05" width="268.94" height="268.94" />
      <rect x="403.4" y="403.45" width="268.94" height="672.24" />
      <rect x="806.81" y="0.05" width="268.94" height="268.94" />
      <rect x="806.81" y="403.45" width="268.94" height="672.24" />
    </svg>
  )
}

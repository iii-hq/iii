import { cn } from '@/lib/utils'

/** A small "New" tag in the hero accent, set after a nav or footer label that points at something just launched. */
export function NewBadge({ className }: { className?: string }) {
  return (
    <span
      className={cn(
        'inline-flex h-[18px] shrink-0 items-center rounded-full bg-hero-accent/12 px-1.5 font-medium font-sans text-[10.5px] text-hero-accent leading-none tracking-[0.01em] ring-1 ring-hero-accent/25 ring-inset',
        className,
      )}
    >
      New
    </span>
  )
}

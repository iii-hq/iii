import { ArrowLeftIcon, ArrowRightIcon } from 'lucide-react'

import { TrackedLink } from '@/components/site/tracked-link'
import type { Spec } from '@/lib/specs'
import { cn } from '@/lib/utils'

const cell =
  'group/pager -mx-4 flex min-h-11 flex-col gap-1 rounded-xl px-4 py-3 outline-none transition-colors hover:bg-foreground/[0.04] focus-visible:outline-2 focus-visible:outline-foreground focus-visible:outline-offset-2'

function PagerLink({ spec, direction }: { spec: Spec; direction: 'newer' | 'older' }) {
  const older = direction === 'older'
  return (
    <TrackedLink
      href={`/roadmap/${spec.slug}/`}
      event="tech_spec_open"
      params={{ slug: spec.slug, cta_location: 'tech_spec_pager' }}
      className={cn(cell, older ? 'items-end text-right sm:col-start-2' : 'items-start')}
    >
      <span className="flex items-center gap-1.5 text-[13px] text-muted-foreground">
        {older ? null : (
          <ArrowLeftIcon
            aria-hidden
            strokeWidth={1.75}
            className="size-3.5 transition-transform group-hover/pager:-translate-x-0.5"
          />
        )}
        {older ? 'Older' : 'Newer'}
        {older ? (
          <ArrowRightIcon
            aria-hidden
            strokeWidth={1.75}
            className="size-3.5 transition-transform group-hover/pager:translate-x-0.5"
          />
        ) : null}
      </span>
      <span className="font-medium text-[15px] text-foreground leading-[1.4] tracking-[-0.01em]">{spec.title}</span>
    </TrackedLink>
  )
}

/** Newer / older spec, the list being newest first. */
export function SpecPager({ newer, older, className }: { newer?: Spec; older?: Spec; className?: string }) {
  if (!newer && !older) return null
  return (
    <nav aria-label="Other specs" className={cn('grid gap-3 sm:grid-cols-2', className)}>
      {newer ? <PagerLink spec={newer} direction="newer" /> : null}
      {older ? <PagerLink spec={older} direction="older" /> : null}
    </nav>
  )
}

'use client'

import { lazy, Suspense, useEffect, useState } from 'react'

import { currentSeason, type Season } from '@/lib/seasons'

// Loaded only while a season is showing, so the rest of the year no visitor downloads it.
const ThanksgivingLink = lazy(() => import('./thanksgiving/thanksgiving-link'))

/** The plain mark at the seasonal size, held in place while the seasonal code loads. */
function RestingMark({ size }: { size: number }) {
  return (
    <svg aria-hidden="true" viewBox="0 0 16 16" width={size} height={size} className="fill-foreground">
      {[0, 6, 12].map((x) => (
        <g key={x}>
          <rect x={x} y={0} width={4} height={4} />
          <rect x={x} y={6} width={4} height={10} />
        </g>
      ))}
    </svg>
  )
}

/**
 * The header and footer home link. Outside a season it renders `children` (the site's own logo) exactly as before;
 * during one it swaps in that season's mark at `size` px. The season is read after mount, from the reader's local
 * date or a `?season=` preview, so the static HTML is the same all year.
 */
export function SeasonalLink({
  href,
  label,
  size,
  dropIn,
  className,
  children,
}: {
  href: string
  label: string
  /** The seasonal mark's size in px. */
  size: number
  /** Play once on its own shortly after the first page load of a visit. */
  dropIn?: boolean
  className?: string
  children: React.ReactNode
}) {
  const [season, setSeason] = useState<Season | null>(null)
  useEffect(() => setSeason(currentSeason()), [])

  if (season === 'thanksgiving') {
    return (
      <Suspense
        fallback={
          <a href={href} aria-label={label} className={className}>
            <RestingMark size={size} />
          </a>
        }
      >
        <ThanksgivingLink href={href} label={label} size={size} dropIn={dropIn} className={className} />
      </Suspense>
    )
  }

  return (
    <a href={href} aria-label={label} className={className}>
      {children}
    </a>
  )
}

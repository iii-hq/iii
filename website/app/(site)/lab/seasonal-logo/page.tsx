import type { Metadata } from 'next'

import { SeasonalLab } from '@/components/seasonal/lab/seasonal-lab'

/** A private workbench for the Thanksgiving mark. Not linked from the site and kept out of search. */
export const metadata: Metadata = {
  title: { absolute: 'iii / seasonal logo lab' },
  robots: { index: false, follow: false },
}

export default function SeasonalLogoLabPage() {
  return (
    <main className="mx-auto max-w-[1240px] px-5 pt-14 pb-24 sm:px-6">
      <p className="font-mono text-[12px] text-muted-foreground uppercase tracking-wider">Lab · Thanksgiving 2026</p>
      <h1 className="mt-2 font-medium text-[28px] text-foreground tracking-tight">Seasonal logo ideas</h1>
      <p className="mt-2 max-w-[640px] text-pretty text-[15px] text-muted-foreground leading-relaxed">
        The six marks the site rotates through in the header and footer during Thanksgiving week (Nov 23 to 29). Each
        tab shows one large and at the real header and footer sizes. To preview the real header and footer before then,
        open any page with <code className="font-mono text-foreground/80">?season=thanksgiving</code> (and{' '}
        <code className="font-mono text-foreground/80">?season=off</code> to stop).
      </p>
      <div className="mt-8">
        <SeasonalLab />
      </div>
    </main>
  )
}

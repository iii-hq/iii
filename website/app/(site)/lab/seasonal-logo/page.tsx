import type { Metadata } from 'next'

import { SeasonalLab } from '@/components/seasonal/lab/seasonal-lab'

/** A private page showing the six Thanksgiving marks. Not linked from the site and kept out of search. */
export const metadata: Metadata = {
  title: { absolute: 'iii / seasonal logo lab' },
  robots: { index: false, follow: false },
}

export default function SeasonalLogoLabPage() {
  return (
    <main className="mx-auto max-w-[1240px] px-5 py-14 sm:px-6">
      <SeasonalLab />
    </main>
  )
}

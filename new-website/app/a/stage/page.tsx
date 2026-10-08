import type { Metadata } from 'next'

import { HeroStage } from '@/components/a/hero-stage'

/**
 * Recording stage for the hero's background film. Open at 1920×1080, record 24 seconds, encode to
 * `public/hero/system.mp4`. Not linked from anywhere and not indexed.
 */
export const metadata: Metadata = {
  title: 'iii hero stage',
  robots: { index: false, follow: false },
}

export default function Page() {
  return <HeroStage />
}

import type { Metadata } from 'next'

import { VersionA } from '@/components/a/version-a'

export const metadata: Metadata = {
  title: 'iii: More application. Less infrastructure.',
  description:
    'Connect any functions, agents, APIs, models, browsers, infrastructure, and machines into one live, composable system. The convenience of a platform, without the lock-in.',
}

export default function Page() {
  return <VersionA />
}

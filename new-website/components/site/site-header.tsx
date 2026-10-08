import { getCommunityStats } from '@/lib/community'
import { Header } from './header'

/** Server wrapper: loads live GitHub and Discord counts, then renders the client header. */
export async function SiteHeader() {
  const stats = await getCommunityStats()
  return <Header stats={stats} />
}

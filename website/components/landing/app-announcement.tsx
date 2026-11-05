import { ArrowRightIcon } from 'lucide-react'

import { TrackedLink } from '@/components/site/tracked-link'

import styles from './app-announcement.module.css'
import { iiiApp } from './content'

/**
 * The launch pill above the hero headline: a lit "New" badge and one line that opens the app's page in the registry.
 * Spans only, no paragraph, so scripts/generate-llms-agents.ts still reads the hero copy as the first <p>.
 */
export function AppAnnouncement() {
  const { announcement } = iiiApp
  return (
    <TrackedLink
      href={announcement.href}
      params={{ cta_id: 'iii_app_announcement', cta_location: 'hero' }}
      className={styles.pill}
    >
      <span className={styles.pillBadge}>
        <span aria-hidden className={styles.pillDot} />
        {announcement.badge}
      </span>
      <span className={`${styles.pillLabel} sm:hidden`}>{announcement.short}</span>
      <span className={`${styles.pillLabel} hidden sm:inline`}>{announcement.label}</span>
      <ArrowRightIcon aria-hidden strokeWidth={1.75} className={`${styles.pillArrow} size-3.5`} />
    </TrackedLink>
  )
}

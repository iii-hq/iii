/* failure — one crash fans out to an ordered stop (A7). data only. */

import type { FanHandler } from '@lib/components/diagrams/FanOut'

export const FAILURE_SOURCE = { label: 'database', sub: 'crashes post-ready' }

export const FAILURE_TRIGGER = 'cascade stop'

export const FAILURE_HANDLERS: FanHandler[] = [
  { id: 'api', label: 'api', desc: 'stopped first — depends_on database' },
  { id: 'cron', label: 'cron-reports', desc: 'stopped next — transitive dependent' },
  { id: 'log', label: 'operator log', desc: 'cause recorded · states set to failed' },
]

export const FAILURE_NOTES = [
  'the alternative is a queue worker pushing into a dead database — corruption you find later.',
  'the cascade is local: one daemon, its own children, reverse dependency order.',
  'each stopped worker’s post_run fires once its exit is confirmed — cleanup runs even on a crash.',
  'softer edges (systemd wants vs needs) can come later; v1 has one edge type and it is strict.',
] as const

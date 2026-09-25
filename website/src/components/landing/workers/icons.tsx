import type { ReactNode } from "react"
import type { WorkerName } from "./catalog"

// Hand-drawn 70×70 line icons, one per worker. Stroke styling lives on the
// wrapping <g> in <WorkerIcon>; `fill="currentColor"` marks the solid dots.
const ICONS: Record<WorkerName, ReactNode> = {
  // an object store: a bucket with objects settling into it
  storage: (
    <>
      <path d="M16 22 L 20 56 Q 35 60 50 56 L 54 22" />
      <ellipse cx="35" cy="22" rx="19" ry="4.5" />
      <path d="M18 27 Q 35 31.5 52 27" />
      <circle cx="29" cy="42" r="1.4" fill="currentColor" />
      <circle cx="37" cy="46" r="1.4" fill="currentColor" />
      <circle cx="42" cy="38" r="1.4" fill="currentColor" />
    </>
  ),
  // a database: the classic stacked cylinder
  database: (
    <>
      <ellipse cx="35" cy="17" rx="19" ry="5.5" />
      <path d="M16 17 V 53 Q 16 58.5 35 58.5 Q 54 58.5 54 53 V 17" />
      <path d="M16 29 Q 16 34.5 35 34.5 Q 54 34.5 54 29" />
      <path d="M16 41 Q 16 46.5 35 46.5 Q 54 46.5 54 41" />
    </>
  ),
  // a schedule: a clock whose hand is about to tick
  cron: (
    <>
      <circle cx="35" cy="36" r="20" />
      <path d="M35 22 V 36 L 44 42" />
      <path d="M35 10 V 14 M 28 10 H 42" />
      <circle cx="35" cy="36" r="1.4" fill="currentColor" />
    </>
  ),
  // a durable queue: messages lined up, one leaving
  queue: (
    <>
      <rect x="12" y="24" width="12" height="22" rx="2" />
      <rect x="29" y="24" width="12" height="22" rx="2" />
      <rect x="46" y="24" width="12" height="22" rx="2" strokeDasharray="3 3" />
      <path d="M52 14 L 60 14 M 57 11 L 60 14 L 57 17" />
    </>
  ),
}

export function WorkerIcon({ name, className }: { name: WorkerName; className?: string }) {
  return (
    <svg viewBox="0 0 70 70" aria-hidden="true" className={className}>
      <g fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round">
        {ICONS[name] ?? <circle cx="35" cy="35" r="18" />}
      </g>
    </svg>
  )
}

/** The small circled check in front of "Verified"; takes the text's colour. */
export function VerifiedIcon({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 16 16" aria-hidden="true" className={className}>
      <g fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round">
        <circle cx="8" cy="8" r="6.4" />
        <path d="M5 8.4 L7.2 10.6 L11.2 6" />
      </g>
    </svg>
  )
}

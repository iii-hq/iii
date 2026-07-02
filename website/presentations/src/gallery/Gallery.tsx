import { SPECS } from 'virtual:spec-manifest'
import type { SpecEntry } from 'virtual:spec-manifest'
import { PresentationCard } from './PresentationCard'

interface TimelineItem {
  spec: SpecEntry
  index: number
}

interface MonthGroup {
  month: string
  items: TimelineItem[]
}

/**
 * the roadmap: one column, strictly newest first, grouped under month
 * markers on a vertical rule. the manifest pre-sorts featured-first for the
 * landing feed (dist/index.json); a timeline is chronological by definition,
 * so it re-sorts by date here and ignores pinning.
 */
export function Gallery() {
  if (SPECS.length === 0) {
    return (
      <div className="border border-rule bg-bg px-6 py-16 text-center font-mono text-[13px] lowercase text-ink-faint">
        no specs yet. add <span className="text-ink">tech-specs/&lt;yyyy-mm-dd-slug&gt;/README.md</span> with a
        frontmatter block to list the first one.
      </div>
    )
  }

  const ordered = [...SPECS].sort((a, b) => b.date.localeCompare(a.date))
  const groups: MonthGroup[] = []
  ordered.forEach((spec, index) => {
    const last = groups[groups.length - 1]
    const item = { spec, index }
    if (last && last.month === spec.month) last.items.push(item)
    else groups.push({ month: spec.month, items: [item] })
  })

  return (
    <ol className="relative max-w-[860px] border-l border-rule">
      {groups.map((group, g) => (
        <li key={group.month} className={g > 0 ? 'pt-12' : ''}>
          <div className="relative pl-6 @3xl:pl-9">
            <span
              aria-hidden
              className="absolute left-0 top-[3px] size-[9px] -translate-x-1/2 border border-rule bg-panel"
            />
            <h3 className="font-mono text-[11px] uppercase tracking-[0.18em] leading-none text-ink-ghost">
              {group.month}
            </h3>
          </div>
          <ol>
            {group.items.map(({ spec, index }) => (
              <li key={spec.slug} className="relative mt-7 pl-6 @3xl:pl-9">
                <span
                  aria-hidden
                  className={`absolute left-0 top-[4px] size-[7px] -translate-x-1/2 rounded-full ${
                    index === 0 ? 'bg-accent pulse-dot' : 'bg-ink-ghost'
                  }`}
                />
                <div className="font-mono text-[11px] uppercase tracking-[0.14em] leading-none text-ink-faint">
                  {spec.dayLabel ?? spec.month}
                  {index === 0 ? <span className="text-accent"> · latest</span> : null}
                </div>
                <div className="mt-3">
                  <PresentationCard spec={spec} index={index} />
                </div>
              </li>
            ))}
          </ol>
        </li>
      ))}
    </ol>
  )
}

import type { SpecEntry } from 'virtual:spec-manifest'
import { StatusDot } from '@lib/components/schematic/StatusDot'

/**
 * one spec on the roadmap. a plain anchor to `<slug>/` (a real navigation
 * into the built deck — or the markdown viewer when no deck exists). the
 * timeline gutter owns the date, so the card carries only sequence number and
 * status; the single accent appears only on hover — border, title, arrow.
 */
export function PresentationCard({ spec, index }: { spec: SpecEntry; index: number }) {
  const { slug, title, tagline, tags, status, hasDeck } = spec
  const isDraft = status === 'draft'
  const num = String(index + 1).padStart(2, '0')

  return (
    <a
      href={`${slug}/`}
      aria-label={`${title}. ${tagline}${isDraft ? ' (draft)' : ''}`}
      style={{ animationDelay: `${Math.min(index, 8) * 60}ms` }}
      className="group flex flex-col border border-rule bg-bg p-6 @3xl:p-7 transition-colors hover:border-accent card-rise"
    >
      <div className="flex items-center justify-between gap-x-3">
        <span className="font-mono text-[11px] uppercase tracking-[0.14em] text-ink-ghost">{num}</span>
        <span className="flex items-center gap-x-3 font-mono text-[11px] uppercase tracking-[0.06em] text-ink-faint">
          {isDraft ? (
            <span className="flex items-center gap-x-1.5 text-warn">
              <StatusDot tone="warn" />
              draft
            </span>
          ) : null}
          <span>{hasDeck ? 'interactive deck' : 'spec'}</span>
        </span>
      </div>

      <h3 className="mt-4 font-mono text-[18px] @3xl:text-[20px] font-semibold lowercase tracking-[-0.01em] leading-[1.25] text-ink transition-colors group-hover:text-accent">
        {title}
      </h3>
      <p className="mt-2.5 font-mono text-[13px] leading-[1.6] text-ink-faint">{tagline}</p>

      <div className="mt-auto pt-6 flex items-end justify-between gap-x-3">
        <span className="flex flex-wrap gap-1.5">
          {tags.slice(0, 4).map((t) => (
            <span
              key={t}
              className="border border-rule px-1.5 py-0.5 font-mono text-[10px] uppercase tracking-[0.08em] text-ink-faint"
            >
              {t}
            </span>
          ))}
        </span>
        <span
          aria-hidden
          className="font-mono text-[12px] lowercase whitespace-nowrap text-ink-faint transition-colors group-hover:text-accent"
        >
          open &rarr;
        </span>
      </div>

      <div className="mt-4 border-t border-rule-2 pt-3 font-mono text-[10px] lowercase text-ink-ghost truncate">
        tech-specs/{slug}
      </div>
    </a>
  )
}

import { Wordmark } from '@lib/components/schematic/Wordmark'
import { SITE } from './site'

export function SiteFooter() {
  return (
    <footer className="border-t border-rule">
      <div className="flex flex-wrap items-center justify-between gap-y-3 px-4 py-4 @3xl:px-9">
        <span className="flex items-center gap-x-2.5">
          <Wordmark className="h-[14px]" />
          <span className="font-mono text-[11px] uppercase tracking-[0.06em] text-ink-faint">{SITE.attribution}</span>
        </span>
        <span className="font-mono text-[11px] uppercase tracking-[0.06em] text-ink-ghost">{SITE.source}</span>
      </div>
    </footer>
  )
}

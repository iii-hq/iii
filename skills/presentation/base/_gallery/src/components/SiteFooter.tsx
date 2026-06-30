import { Wordmark } from '@/components/schematic/Wordmark'
import { GALLERY_META } from '@/content/presentations'

export function SiteFooter() {
  return (
    <footer className="border-t border-rule">
      <div className="flex flex-wrap items-center justify-between gap-y-3 px-4 py-4 @3xl:px-9">
        <span className="flex items-center gap-x-2.5">
          <Wordmark className="h-[14px]" />
          <span className="font-mono text-[11px] uppercase tracking-[0.06em] text-ink-faint">
            {GALLERY_META.attribution}
          </span>
        </span>
        <span className="font-mono text-[11px] uppercase tracking-[0.06em] text-ink-ghost">
          {GALLERY_META.source}
        </span>
      </div>
    </footer>
  )
}

import { Prompt } from '@/components/schematic/Prompt'
import { Wordmark } from '@/components/schematic/Wordmark'
import { FOOTER } from '@/content/deck'

export function Footer() {
  return (
    <footer className="border-t border-rule">
      <div className="px-4 py-16 @3xl:px-9 @3xl:py-20">
        <div className="font-mono text-[11px] uppercase tracking-[0.06em] text-ink-faint mb-5">
          <Prompt symbol="$">{FOOTER.eyebrow}</Prompt>
        </div>
        <div className="font-mono text-[34px] @3xl:text-[48px] font-semibold lowercase leading-[1.05] tracking-[-0.03em] text-ink max-w-[24ch]">
          {FOOTER.headline}
        </div>
        <div className="mt-8 inline-flex items-center gap-x-2 border border-rule bg-bg px-4 py-3 font-mono text-[13px]">
          <Prompt symbol="$" />
          <span className="text-ink">{FOOTER.command}</span>
        </div>
      </div>
      <div className="flex flex-wrap items-center justify-between gap-y-3 border-t border-rule px-4 py-4 @3xl:px-9">
        <span className="flex items-center gap-x-2.5">
          <Wordmark className="h-[14px]" />
          <span className="font-mono text-[11px] uppercase tracking-[0.06em] text-ink-faint">
            {FOOTER.attribution}
          </span>
        </span>
        <span className="font-mono text-[11px] uppercase tracking-[0.06em] text-ink-ghost">
          {FOOTER.source}
        </span>
      </div>
    </footer>
  )
}

import { Prompt } from '@lib/components/schematic/Prompt'
import { Wordmark } from '@lib/components/schematic/Wordmark'
import type { FooterSpec } from '@lib/lib/deck-types'

export function Footer({ footer }: { footer: FooterSpec }) {
  return (
    <footer className="border-t border-rule">
      <div className="px-4 py-16 @3xl:px-9 @3xl:py-20">
        <div className="font-mono text-[11px] uppercase tracking-[0.06em] text-ink-faint mb-5">
          <Prompt symbol="$">{footer.eyebrow}</Prompt>
        </div>
        <div className="font-mono text-[34px] @3xl:text-[48px] font-semibold lowercase leading-[1.05] tracking-[-0.03em] text-ink max-w-[24ch]">
          {footer.headline}
        </div>
        <div className="mt-8 inline-flex items-center gap-x-2 border border-rule bg-bg px-4 py-3 font-mono text-[13px]">
          <Prompt symbol="$" />
          <span className="text-ink">{footer.command}</span>
        </div>
      </div>
      <div className="flex flex-wrap items-center justify-between gap-y-3 border-t border-rule px-4 py-4 @3xl:px-9">
        <span className="flex items-center gap-x-2.5">
          <Wordmark className="h-[14px]" />
          <span className="font-mono text-[11px] uppercase tracking-[0.06em] text-ink-faint">{footer.attribution}</span>
        </span>
        <span className="font-mono text-[11px] uppercase tracking-[0.06em] text-ink-ghost">{footer.source}</span>
      </div>
    </footer>
  )
}

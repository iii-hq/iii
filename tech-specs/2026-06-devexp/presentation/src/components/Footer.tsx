import { Prompt } from '@/components/schematic/Prompt'
import { Wordmark } from '@/components/schematic/Wordmark'

export function Footer() {
  return (
    <footer className="border-t border-rule">
      <div className="px-4 py-16 @3xl:px-9 @3xl:py-20">
        <div className="font-mono text-[11px] uppercase tracking-[0.06em] text-ink-faint mb-5">
          <Prompt symbol="$">get started</Prompt>
        </div>
        <div className="font-mono text-[34px] @3xl:text-[48px] font-semibold lowercase leading-[1.05] tracking-[-0.03em] text-ink max-w-[24ch]">
          one file. one command. one terminal.
        </div>
        <div className="mt-8 flex flex-wrap items-center gap-3">
          <span className="inline-flex items-center gap-x-2 border border-rule bg-bg px-4 py-3 font-mono text-[13px]">
            <Prompt symbol="$" />
            <span className="text-ink">iii init quickstart && cd quickstart && iii up</span>
          </span>
        </div>
      </div>
      <div className="flex flex-wrap items-center justify-between gap-y-3 border-t border-rule px-4 py-4 @3xl:px-9">
        <span className="flex items-center gap-x-2.5">
          <Wordmark className="h-[14px]" />
          <span className="font-mono text-[11px] uppercase tracking-[0.06em] text-ink-faint">
            developer experience overhaul
          </span>
        </span>
        <span className="font-mono text-[11px] uppercase tracking-[0.06em] text-ink-ghost">
          source of truth: tech-specs/2026-06-devexp
        </span>
      </div>
    </footer>
  )
}

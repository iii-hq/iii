"use client"

import { memo } from "react"
import { Token } from "@/components/code/token-lines"
import { keyed } from "@/lib/keys"
import type { CodeLine } from "@/lib/shiki"
import { cn } from "@/lib/utils"
import type { Worker } from "./catalog"
import { VerifiedIcon, WorkerIcon } from "./icons"
import { sliceTokenLines } from "./slice-tokens"

/** A card's two code lines (install command, what it registers), tokenized on the server. */
export type CardCode = { add: CodeLine; trigger: CodeLine[] }

type WorkerCardProps = {
  cardKey: number
  worker: Worker
  code: CardCode
  focused: boolean
  /** this card's lines are being typed; `add` / `trigger` hold the typed prefixes */
  typing: boolean
  add: string
  trigger: string
  register: (key: number, el: HTMLDivElement | null) => void
}

function Line({ tokens }: { tokens: CodeLine }) {
  return (
    <span className="code-tokens block">
      {keyed(tokens, (t) => t.text).map((t) => (
        <Token key={t.key} token={t.item} />
      ))}
    </span>
  )
}

/**
 * One registry entry. The outer div (what the ticker measures) carries the
 * gutter, so pruned cards hand their full pitch to the spacer and nothing
 * shifts. Cards not being typed into show their commands complete. Only the
 * focused card is at full strength; the rest sit back a little (opacity and a
 * hair of scale, both cheap) so the eye has one place to rest while the strip
 * moves.
 */
export const WorkerCard = memo(function WorkerCard({
  cardKey,
  worker,
  code,
  focused,
  typing,
  add,
  trigger,
  register,
}: WorkerCardProps) {
  const addLines = typing ? sliceTokenLines([code.add], add.length) : [code.add]
  const triggerLines = typing ? sliceTokenLines(code.trigger, trigger.length) : code.trigger

  return (
    <div ref={(el) => register(cardKey, el)} className="w-[412px] flex-none px-1.5 max-sm:w-[340px]">
      <div
        className={cn(
          "flex h-full flex-col rounded-[14px] bg-gray-2 p-5 max-sm:p-4",
          "transition-[opacity,transform,box-shadow] duration-500 ease-[cubic-bezier(0.23,1,0.32,1)] motion-reduce:transition-none",
          focused
            ? "shadow-[var(--shadow-panel),inset_0_0_0_1px_var(--gray-9)]"
            : "scale-[0.98] opacity-60 shadow-panel motion-reduce:scale-100",
        )}
      >
        <div className="flex items-center gap-3">
          <span className="flex size-10 shrink-0 items-center justify-center rounded-[8px] bg-gray-3 text-gray-12">
            <WorkerIcon name={worker.name} className="size-7" />
          </span>
          <div className="flex min-w-0 flex-1 items-baseline justify-between gap-3">
            <span className="truncate text-[15px] font-medium text-gray-12">{worker.name}</span>
            <span className="shrink-0 text-[12px] text-gray-10 tabular-nums">{worker.version}</span>
          </div>
        </div>

        <p className="mt-3 line-clamp-2 min-h-[41px] text-[13px] leading-[1.55] text-gray-11">{worker.desc}</p>

        <div className="mt-4 min-h-[78px] overflow-hidden rounded-[8px] bg-gray-1 px-3 py-2.5 font-mono text-[13px] leading-[1.7] tracking-[0.01em] whitespace-pre shadow-[inset_0_0_0_1px_var(--gray-5)] max-sm:text-[11px]">
          <div className="flex gap-2">
            <span className="shrink-0 text-gray-9 select-none">$</span>
            <span className="min-w-0 flex-1">{addLines[0] && <Line tokens={addLines[0]} />}</span>
          </div>
          <div className="flex gap-2">
            <span className="shrink-0 text-gray-9 select-none">›</span>
            <span className="min-w-0 flex-1">
              {keyed(triggerLines, (line) => line.map((t) => t.text).join("")).map((line) => (
                <Line key={line.key} tokens={line.item} />
              ))}
            </span>
          </div>
        </div>

        <div className="mt-4 flex items-center justify-between gap-3">
          {/* Every registry worker ships as a binary; the badge is the registry's own. */}
          <span className="rounded-[6px] bg-gray-3 px-2 py-0.5 text-[11px] font-medium text-gray-11">binary</span>
          {worker.verified && (
            <span className="inline-flex items-center gap-1 text-[11px] text-gray-10">
              <VerifiedIcon className="size-3" /> Verified
            </span>
          )}
        </div>
      </div>
    </div>
  )
})

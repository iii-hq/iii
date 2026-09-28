"use client"

import { m, useInView, useReducedMotion } from "motion/react"
import { useRef } from "react"
import { SearchIcon } from "@/components/site/iconly"
import { cn } from "@/lib/utils"
import type { WorkerName } from "./catalog"
import { useWorkerTicker } from "./use-worker-ticker"
import { type CardCode, WorkerCard } from "./worker-card"

type WorkersShowcaseProps = {
  /** server-tokenized `$` / `›` lines per worker */
  code: Record<WorkerName, CardCode>
  className?: string
}

/**
 * The registry search + the horizontally gliding strip of worker cards. The
 * whole thing is a picture for assistive tech: the query and the cards change
 * every few seconds and would only be noise read aloud.
 */
export function WorkersShowcase({ code, className }: WorkersShowcaseProps) {
  const rootRef = useRef<HTMLDivElement>(null)
  const seen = useInView(rootRef, { once: true, margin: "0px 0px -10% 0px", amount: 0.15 })
  const onScreen = useInView(rootRef)
  const reducedMotion = useReducedMotion() ?? false
  const { query, selected, cards, spacer, focusedKey, command, x, viewportRef, registerCard } = useWorkerTicker({
    started: seen,
    active: onScreen,
    reducedMotion,
  })

  return (
    <div
      ref={rootRef}
      role="img"
      aria-label="Searching the worker registry: a search term is typed, the matching worker card slides into view and its install command types itself in."
      className={className}
    >
      {/* Search field */}
      <div className="flex h-12 items-center gap-3 rounded-[12px] bg-gray-2 px-4 shadow-[inset_0_0_0_1px_var(--gray-6)]">
        <SearchIcon className="size-4 shrink-0 text-gray-9" />
        <div className="flex min-w-0 flex-1 items-center text-[15px] text-gray-12">
          {/* The query, shown as a selection (⌘A) just before it is typed over. */}
          <span
            className={cn(
              "truncate rounded-[2px] whitespace-pre transition-[background-color] duration-150 ease-out",
              selected && "bg-gray-7",
            )}
          >
            {query}
          </span>
          <m.span
            aria-hidden="true"
            className="mx-px inline-block h-[1.15em] w-px shrink-0 bg-gray-12"
            animate={reducedMotion || selected ? { opacity: selected ? 0 : 1 } : { opacity: [1, 1, 0, 0] }}
            transition={
              selected
                ? { duration: 0.1 }
                : { duration: 1.1, times: [0, 0.5, 0.5, 1], ease: "linear", repeat: Number.POSITIVE_INFINITY }
            }
          />
          {query === "" && <span className="truncate text-gray-9">Search the registry</span>}
        </div>
        {/* Where this search really lives. No result count: the registry grows, and a number here would be made up. */}
        <span className="shrink-0 font-mono text-[12px] tracking-[0.02em] text-gray-9 max-sm:hidden">
          workers.iii.dev
        </span>
      </div>

      {/* Card strip. Clipped on x only so the cards' shadows can breathe; the
          vertical padding is what gives them room. */}
      <div
        ref={viewportRef}
        className={cn(
          "relative -mt-4 -mb-8 min-h-[340px] overflow-x-clip overflow-y-visible py-8",
          "[mask-image:linear-gradient(to_right,transparent,black_5%,black_95%,transparent)]",
        )}
      >
        <m.div className="flex items-stretch" style={{ x }}>
          {spacer > 0 && <div aria-hidden="true" className="flex-none" style={{ width: spacer }} />}
          {cards.map((card) => {
            const typing = command?.key === card.key
            return (
              <WorkerCard
                key={card.key}
                cardKey={card.key}
                worker={card.worker}
                code={code[card.worker.name]}
                focused={card.key === focusedKey}
                typing={typing}
                add={typing ? command.add : ""}
                trigger={typing ? command.trigger : ""}
                register={registerCard}
              />
            )
          })}
        </m.div>
      </div>
    </div>
  )
}

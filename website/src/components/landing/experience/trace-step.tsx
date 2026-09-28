"use client"

import { m } from "motion/react"
import { type ReactNode, useEffect, useEffectEvent, useState } from "react"
import { Swap } from "@/components/motion/swap"
import { keyed } from "@/lib/keys"
import { cn } from "@/lib/utils"
import { EASE_OUT } from "./motion"
import { StepCard, StepCardHeader } from "./step-card"
import type { TraceStep as TraceStepData } from "./steps"
import { WaitAction } from "./wait-action"

type TraceStepProps = { step: TraceStepData; onAdvance: (label?: string) => void }

type Phase = "traces" | "waterfall" | "detail"
const PHASES: readonly { id: Phase; label: string }[] = [
  { id: "traces", label: "Traces" },
  { id: "waterfall", label: "Waterfall" },
  { id: "detail", label: "Detail" },
]
const ROW_HIDDEN = { opacity: 0, x: 6 }
const ROW_SHOWN = { opacity: 1, x: 0 }
const ROW = { duration: 0.25, ease: EASE_OUT }

/**
 * The iii console: the trace list, then the span waterfall, then the failing
 * span's detail. All three panes are laid out from the start and fade in as
 * their phase arrives, so the card never changes height.
 */
export function TraceStep({ step, onAdvance }: TraceStepProps) {
  const { traces, spans, autoAdvance, detail } = step
  const [phase, setPhase] = useState<Phase>("traces")
  const [rowsShown, setRowsShown] = useState(0)
  const [done, setDone] = useState(false)

  // Trace rows land one every 80ms after a 100ms lead (same arithmetic as the old
  // script): one interval reads the clock and shows however many rows are due.
  useEffect(() => {
    const count = traces.length
    const start = performance.now()
    const id = setInterval(() => {
      const due = Math.min(count, Math.floor((performance.now() - start - 100) / 80) + 1)
      if (due > 0) setRowsShown(due)
      if (due >= count) clearInterval(id)
    }, 40)
    return () => clearInterval(id)
  }, [traces.length])

  // Then the waterfall, then the failing span's detail.
  const traceDelay = traces.length * 80 + 400
  useEffect(() => {
    const id = setTimeout(() => setPhase("waterfall"), traceDelay)
    return () => clearTimeout(id)
  }, [traceDelay])
  const detailDelay = traceDelay + spans.length * 120 + 400 + 400
  useEffect(() => {
    const id = setTimeout(() => setPhase("detail"), detailDelay)
    return () => clearTimeout(id)
  }, [detailDelay])

  const inWaterfall = phase !== "traces"
  const inDetail = phase === "detail"

  // An Effect Event: the timer reads the latest `onAdvance` without restarting when it changes identity.
  const finish = useEffectEvent(() => {
    setDone(true)
    onAdvance()
  })
  useEffect(() => {
    if (!inDetail || !autoAdvance) return
    const t = setTimeout(finish, autoAdvance)
    return () => clearTimeout(t)
  }, [inDetail, autoAdvance])

  const hasErr = step.errorSpanIndex != null
  const action = done ? "done" : inDetail && !autoAdvance ? "continue" : "none"

  return (
    <StepCard>
      <StepCardHeader>
        <span className="font-medium text-gray-12">iii console</span>
        <span aria-hidden="true" className="mx-1 text-gray-7 max-sm:hidden">
          /
        </span>
        <ol aria-label="Console view" className="flex items-center gap-2.5 text-[12px] max-sm:hidden">
          {PHASES.map((p) => (
            <li
              key={p.id}
              aria-current={p.id === phase ? "step" : undefined}
              className={cn(
                "transition-colors duration-150 ease-out",
                p.id === phase ? "font-medium text-gray-12" : "text-gray-8",
              )}
            >
              {p.label}
            </li>
          ))}
        </ol>
        <span className="ml-auto flex items-center text-[12px] text-gray-11">
          <Swap id={action}>
            {action === "continue" && (
              <WaitAction
                label="Continue"
                onAction={(l) => {
                  setDone(true)
                  onAdvance(l)
                }}
              />
            )}
            {action === "done" && <span className="px-2">{hasErr ? "Error identified" : "Traces inspected"}</span>}
          </Swap>
        </span>
      </StepCardHeader>

      {/* Trace list */}
      <ul className="border-b border-gray-5 p-2 font-mono text-[14px] leading-[1.8] tracking-[0.01em]">
        {keyed(traces, (t) => t.operation).map(({ key, item: t }, i) => {
          const err = t.status === "error"
          return (
            <m.li
              key={key}
              className={cn(
                "flex items-center gap-3 rounded-[8px] px-2 py-1",
                i === step.activeTraceIndex && "bg-gray-3",
              )}
              initial={ROW_HIDDEN}
              animate={i < rowsShown ? ROW_SHOWN : ROW_HIDDEN}
              transition={ROW}
            >
              <span
                aria-hidden="true"
                className={cn("size-1.5 shrink-0 rounded-full", err ? "bg-gray-12" : "bg-gray-7")}
              />
              <span className={cn("min-w-0 flex-1 truncate", err ? "font-medium text-gray-12" : "text-gray-11")}>
                {t.operation}
              </span>
              <span className="text-gray-9 tabular-nums">{t.duration ?? "—"}</span>
              <span className={cn("w-7 text-right text-[11px] font-medium", err ? "text-gray-12" : "text-gray-9")}>
                {err ? "ERR" : "OK"}
              </span>
            </m.li>
          )
        })}
      </ul>

      {/* Span waterfall */}
      <ul
        aria-hidden={!inWaterfall}
        className="border-b border-gray-5 px-4 py-3 font-mono text-[14px] leading-[1.8] tracking-[0.01em]"
      >
        {keyed(spans, (sp) => `${sp.depth}:${sp.label}`).map(({ key, item: sp }, i) => {
          const err = sp.status === "error"
          return (
            <m.li
              key={key}
              className="flex items-center gap-3 py-1"
              style={{ paddingInlineStart: sp.depth * 16 }}
              initial={ROW_HIDDEN}
              animate={inWaterfall ? ROW_SHOWN : ROW_HIDDEN}
              transition={{ ...ROW, delay: inWaterfall ? i * 0.12 : 0 }}
            >
              <span
                className={cn(
                  "w-[120px] shrink-0 truncate sm:w-[168px]",
                  err ? "font-medium text-gray-12" : "text-gray-11",
                )}
              >
                {sp.label}
              </span>
              <span className="relative h-1.5 min-w-0 flex-1 overflow-hidden rounded-full bg-gray-4">
                <m.span
                  className={cn("absolute inset-y-0 left-0 origin-left rounded-full", err ? "bg-gray-8" : "bg-gray-6")}
                  style={{ width: `${sp.widthPercent}%` }}
                  initial={{ scaleX: 0 }}
                  animate={{ scaleX: inWaterfall ? 1 : 0 }}
                  transition={{ duration: 0.4, ease: EASE_OUT, delay: inWaterfall ? i * 0.12 + 0.1 : 0 }}
                />
              </span>
              <span className="w-14 shrink-0 text-right text-gray-9 tabular-nums">{sp.duration}</span>
            </m.li>
          )
        })}
      </ul>

      {/* Failing span detail */}
      <m.dl
        aria-hidden={!inDetail}
        className="px-4 py-3 font-mono text-[14px] leading-[1.8] tracking-[0.01em]"
        initial={{ opacity: 0 }}
        animate={{ opacity: inDetail ? 1 : 0 }}
        transition={{ duration: 0.25, ease: EASE_OUT }}
      >
        <DetailRow label="Status">
          <span className="font-medium text-gray-12">{detail.status}</span>
        </DetailRow>
        <DetailRow label="Service">
          <span className="text-gray-11">{detail.service}</span>
        </DetailRow>
        {detail.error && (
          <DetailRow label="Error">
            <span className="break-words text-gray-12">{detail.error}</span>
          </DetailRow>
        )}
      </m.dl>
    </StepCard>
  )
}

function DetailRow({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex gap-4">
      <dt className="w-14 shrink-0 text-gray-9">{label}</dt>
      <dd className="min-w-0 flex-1">{children}</dd>
    </div>
  )
}

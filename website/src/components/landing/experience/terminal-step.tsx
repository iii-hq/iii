"use client"

import { useEffect, useEffectEvent, useState } from "react"
import { cn } from "@/lib/utils"
import styles from "./experience.module.css"
import { StepCard, StepCardHeader } from "./step-card"
import type { TerminalStep as TerminalStepData } from "./steps"
import { useTicker } from "./use-sequencer"

type TerminalStepProps = { step: TerminalStepData; onAdvance: (label?: string) => void }

const OUTPUT_DELAY = 300
const DEFAULT_ADVANCE = 800

/** A terminal that types a CLI command, prints its output, and moves on by itself. */
export function TerminalStep({ step, onAdvance }: TerminalStepProps) {
  const { command, autoAdvance } = step
  const ticks = useTicker(true, step.typingSpeed ?? 40, command.length + 1)
  const typed = ticks > command.length
  const [showOutput, setShowOutput] = useState(false)

  useEffect(() => {
    if (!typed) return
    const t = setTimeout(() => setShowOutput(true), OUTPUT_DELAY)
    return () => clearTimeout(t)
  }, [typed])

  const advance = useEffectEvent(() => onAdvance())
  useEffect(() => {
    if (!showOutput) return
    const t = setTimeout(advance, autoAdvance ?? DEFAULT_ADVANCE)
    return () => clearTimeout(t)
  }, [showOutput, autoAdvance])

  return (
    <StepCard>
      <StepCardHeader>Terminal</StepCardHeader>
      <div className="px-4 py-3 font-mono text-[14px] leading-[1.8] tracking-[0.01em]">
        <div className="flex gap-2 break-words whitespace-pre-wrap">
          <span aria-hidden="true" className="text-gray-9 select-none">
            $
          </span>
          <span className="text-gray-12">
            {command.slice(0, Math.min(ticks, command.length))}
            {!typed && (
              <span
                aria-hidden="true"
                className={cn(styles.caret, "ml-px inline-block h-[15px] w-[7px] bg-gray-12 align-[-2px]")}
              />
            )}
          </span>
        </div>
        {/* The output line keeps its space from the start, so printing it never moves the transcript. */}
        <div
          className={cn(
            "text-gray-11 transition-opacity duration-[250ms] ease-out",
            showOutput ? "opacity-100" : "opacity-0",
          )}
        >
          {step.output}
        </div>
      </div>
    </StepCard>
  )
}

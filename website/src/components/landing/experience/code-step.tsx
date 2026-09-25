"use client"

import { type ReactNode, useCallback, useEffect, useEffectEvent, useLayoutEffect, useRef, useState } from "react"
import { TokenLines } from "@/components/code/token-lines"
import { Swap } from "@/components/motion/swap"
import { CheckIcon } from "@/components/site/iconly"
import type { CodeLine } from "@/lib/shiki"
import { cn } from "@/lib/utils"
import { LangIcon } from "./lang-icon"
import { StepCard, StepCardHeader } from "./step-card"
import type { CodeStep as CodeStepData } from "./steps"
import { useTicker } from "./use-sequencer"
import { WaitAction } from "./wait-action"

type CodeStepProps = {
  step: CodeStepData
  /** the whole file, tokenized by Shiki on the server */
  lines: CodeLine[]
  onAdvance: (label?: string) => void
}

const WRITE_START_DELAY = 400

/**
 * A code editor that writes its file line by line, then waits for Deploy.
 * The card is laid out at its final height from the start (hidden lines keep
 * their space), so writing is opacity and transform only and nothing below
 * it moves.
 */
export function CodeStep({ step, lines, onAdvance }: CodeStepProps) {
  const [writing, setWriting] = useState(false)
  const [deployed, setDeployed] = useState(false)
  const bodyRef = useRef<HTMLDivElement>(null)
  const { autoAdvance } = step
  const ticks = useTicker(writing, step.lineDelay ?? 220, lines.length + 1)
  const shown = Math.min(ticks, lines.length)
  const written = ticks > lines.length
  const label = step.doneLabel ?? "Deploy"

  useEffect(() => {
    const t = setTimeout(() => setWriting(true), WRITE_START_DELAY)
    return () => clearTimeout(t)
  }, [])

  // keep the newest line in view inside the editor's own scroll area
  useLayoutEffect(() => {
    const body = bodyRef.current
    if (!body || shown === 0) return
    const line = body.querySelectorAll<HTMLElement>("code > span")[shown - 1]
    if (!line) return
    const bottom = line.offsetTop + line.offsetHeight + 16
    if (bottom > body.scrollTop + body.clientHeight) body.scrollTop = bottom - body.clientHeight
  }, [shown])

  const deploy = useEffectEvent(() => {
    setDeployed(true)
    onAdvance()
  })
  useEffect(() => {
    if (!written || !autoAdvance) return
    const t = setTimeout(deploy, autoAdvance)
    return () => clearTimeout(t)
  }, [written, autoAdvance])

  const renderLine = useCallback(
    (line: ReactNode, i: number) => (
      <span
        className={cn(
          "block transition-[opacity,transform] duration-[250ms] ease-[cubic-bezier(0.23,1,0.32,1)] motion-reduce:transform-none",
          i < shown ? "opacity-100" : "translate-y-1 opacity-0",
        )}
      >
        {line}
      </span>
    ),
    [shown],
  )

  const state = deployed ? "deployed" : written && !autoAdvance ? "ready" : "writing"

  return (
    <StepCard>
      <StepCardHeader>
        <LangIcon language={step.language} filename={step.filename} className="size-3.5" />
        <span className="truncate font-medium text-gray-12">{step.filename}</span>
        <span className="ml-auto flex items-center text-[12px] text-gray-10">
          <Swap id={state}>
            {state === "writing" && <span className="px-2">{writing ? "Writing…" : "Opening…"}</span>}
            {state === "ready" && (
              <WaitAction
                label={label}
                onAction={(l) => {
                  setDeployed(true)
                  onAdvance(l)
                }}
              />
            )}
            {state === "deployed" && (
              <span className="flex items-center gap-1.5 px-2 text-gray-11">
                <CheckIcon className="size-3.5" />
                Deployed
              </span>
            )}
          </Swap>
        </span>
      </StepCardHeader>
      <div
        ref={bodyRef}
        className={cn(
          "relative max-h-[340px] overflow-y-auto transition-opacity duration-300 ease-out [scrollbar-color:var(--gray-6)_transparent] [scrollbar-width:thin]",
          deployed && "opacity-60",
        )}
      >
        <TokenLines lines={lines} renderLine={renderLine} showLineNumbers className="px-4 py-3" />
      </div>
    </StepCard>
  )
}

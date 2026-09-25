"use client"

import { MotionConfig, m, useInView } from "motion/react"
import { useCallback, useEffect, useMemo, useRef, useState } from "react"
import { SlackIcon } from "@/components/site/icons"
import { buttonVariants } from "@/components/ui/button-variants"
import { track } from "@/lib/analytics"
import type { CodeLine } from "@/lib/shiki"
import { cn } from "@/lib/utils"
import { ChatMessage } from "./chat-message"
import { CodeStep } from "./code-step"
import { Finish } from "./finish"
import { STEP_ENTER } from "./motion"
import { Composer, SentReply } from "./reply"
import { StatusPanel } from "./status-panel"
import { HOMEPAGE_FLOW, type Step } from "./steps"
import { TerminalStep } from "./terminal-step"
import { TraceStep } from "./trace-step"
import { TypingDots } from "./typing-dots"
import { useSequencer } from "./use-sequencer"

const steps = HOMEPAGE_FLOW

type SequencerProps = {
  /** each code step's file, tokenized by Shiki on the server, keyed by step id */
  code: Record<string, CodeLine[]>
  className?: string
}

/**
 * The Slack window that plays HOMEPAGE_FLOW: a title bar, the transcript, and
 * the message box where your replies get typed. Starts once the window is
 * 30% visible. Replies live in the composer until sent, then join the
 * transcript as your messages.
 */
export function Sequencer({ code, className }: SequencerProps) {
  const { session, revealed, typing, finished, start, restart, advance, endIntro } = useSequencer(steps)

  const windowRef = useRef<HTMLElement>(null)
  const scrollRef = useRef<HTMLDivElement>(null)
  const contentRef = useRef<HTMLDivElement>(null)
  const inView = useInView(windowRef, { once: true, amount: 0.3 })

  useEffect(() => {
    if (!inView || session !== 0) return
    track("experience_start", { trigger: "viewport" })
    start()
  }, [inView, session, start])

  // Pin the transcript to the newest step whenever it grows or the window resizes.
  useEffect(() => {
    const scroller = scrollRef.current
    const content = contentRef.current
    if (!scroller || !content) return
    const ro = new ResizeObserver(() => {
      scroller.scrollTop = scroller.scrollHeight
    })
    ro.observe(content)
    ro.observe(scroller)
    return () => ro.disconnect()
  }, [])

  // stable per-step callbacks, so step timers are not reset by re-renders
  const onAdvance = useMemo(() => steps.map((_, i) => (label?: string) => advance(i, label)), [advance])

  // Replies sent this session (keyed with the session so Restart clears them).
  const [sent, setSent] = useState<Record<string, true>>({})
  const shown = steps.slice(0, revealed)
  const lastIndex = revealed - 1
  const last = shown[lastIndex]
  const activeReply = last?.type === "reply" && !sent[`${session}-${last.id}`] ? last : undefined
  const onSend = useCallback(
    (label?: string) => {
      if (!activeReply) return
      setSent((s) => ({ ...s, [`${session}-${activeReply.id}`]: true }))
      onAdvance[lastIndex](label)
    },
    [activeReply, session, lastIndex, onAdvance],
  )

  return (
    <figure
      ref={windowRef}
      aria-label="A scripted work session with iii, in Slack"
      className={cn(
        "flex h-[600px] flex-col overflow-hidden rounded-[18px] bg-gray-2 text-left shadow-panel max-sm:h-[560px]",
        className,
      )}
    >
      {/* Window chrome */}
      <div className="flex h-14 shrink-0 items-center gap-4 border-b border-gray-5 px-4 sm:px-5">
        <div aria-hidden="true" className="flex gap-1.5 max-sm:hidden">
          <span className="size-2.5 rounded-full bg-gray-6" />
          <span className="size-2.5 rounded-full bg-gray-6" />
          <span className="size-2.5 rounded-full bg-gray-6" />
        </div>
        <p className="flex min-w-0 items-center gap-2 text-[13px] font-medium text-gray-12">
          <SlackIcon className="size-3.5 shrink-0" />
          <span className="truncate">
            Slack <span className="text-gray-8">·</span> <span className="font-normal text-gray-11">#product</span>
          </span>
        </p>
        <button
          type="button"
          onClick={restart}
          className={cn(buttonVariants({ variant: "ghost", size: "xs" }), "ml-auto -mr-1.5")}
        >
          Restart
        </button>
      </div>

      {/* Transcript */}
      <div
        ref={scrollRef}
        className="min-h-0 flex-1 overflow-x-hidden overflow-y-auto px-4 py-5 [scrollbar-color:var(--gray-6)_transparent] [scrollbar-width:thin] sm:px-5"
      >
        <MotionConfig reducedMotion="user">
          <div ref={contentRef} className="flex flex-col gap-5">
            {shown.map((step, i) => {
              if (step.type === "reply" && !sent[`${session}-${step.id}`]) return null
              return (
                <m.div key={`${session}-${step.id}`} {...STEP_ENTER}>
                  <StepView step={step} index={i} code={code[step.id]} onAdvance={onAdvance[i]} onStart={endIntro} />
                </m.div>
              )
            })}
            {typing && <TypingDots key={`typing-${session}-${revealed}`} />}
            {finished && (
              <m.div key={`finish-${session}`} {...STEP_ENTER}>
                <Finish />
              </m.div>
            )}
          </div>
        </MotionConfig>
      </div>

      <Composer key={activeReply ? `${session}-${activeReply.id}` : "idle"} step={activeReply} onSend={onSend} />
    </figure>
  )
}

type StepViewProps = {
  step: Step
  index: number
  code?: CodeLine[]
  onAdvance: (label?: string) => void
  onStart: () => void
}

function StepView({ step, index, code, onAdvance, onStart }: StepViewProps) {
  switch (step.type) {
    case "slack-message":
      return <ChatMessage step={step} index={index} onAdvance={onAdvance} onStart={onStart} />
    case "reply":
      return <SentReply step={step} />
    case "code-editor":
      return <CodeStep step={step} lines={code ?? []} onAdvance={onAdvance} />
    case "status":
      return <StatusPanel step={step} onAdvance={onAdvance} />
    case "terminal-command":
      return <TerminalStep step={step} onAdvance={onAdvance} />
    case "console-trace":
      return <TraceStep step={step} onAdvance={onAdvance} />
  }
}

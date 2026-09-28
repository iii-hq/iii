"use client"

import { useEffect, useEffectEvent } from "react"
import { buttonVariants } from "@/components/ui/button-variants"
import { cn } from "@/lib/utils"
import { MessageRow } from "./chat-message"
import styles from "./experience.module.css"
import type { ReplyStep } from "./steps"
import { useTicker } from "./use-sequencer"
import { WaitAction } from "./wait-action"

const YOU = "You"

type ComposerProps = {
  /** the reply being typed; without one the composer is the window's idle message box */
  step?: ReplyStep
  onSend?: (label?: string) => void
}

/**
 * The message box at the bottom of the Slack window. Idle it shows a
 * placeholder; during a reply step it types your answer word by word and
 * enables Send. Its height is constant so the transcript above never jumps.
 * Remount it (key) per reply so the typing restarts.
 */
export function Composer({ step, onSend }: ComposerProps) {
  const words = step ? step.content.split(" ") : []
  // one extra tick after the last word, like the old interval, before Send is ready
  const ticks = useTicker(!!step, step?.typingSpeed ?? 40, words.length + 1)
  const typed = !!step && ticks > words.length
  const autoAdvance = step?.autoAdvance
  const label = step?.sendLabel ?? "Send"

  const send = useEffectEvent(() => onSend?.())
  useEffect(() => {
    if (!typed || !autoAdvance) return
    const t = setTimeout(send, autoAdvance)
    return () => clearTimeout(t)
  }, [typed, autoAdvance])

  return (
    <div className="shrink-0 border-t border-gray-5 px-4 py-3 sm:px-5">
      <div className="flex items-end gap-3 rounded-[12px] bg-gray-1 py-2 pr-2 pl-3.5 shadow-[inset_0_0_0_1px_var(--gray-6)]">
        <p className="min-h-8 min-w-0 flex-1 py-1.5 text-[15px] leading-[1.35] text-gray-12">
          {step ? (
            <>
              {words.slice(0, Math.min(ticks, words.length)).join(" ")}
              {!typed && (
                <span
                  aria-hidden="true"
                  className={cn(styles.caret, "ml-px inline-block h-[15px] w-px bg-gray-12 align-[-2px]")}
                />
              )}
            </>
          ) : (
            <span className="text-gray-9 select-none">Message #product</span>
          )}
        </p>
        {typed && !autoAdvance ? (
          // Waiting for you: ring, Enter, and a countdown to sending itself.
          <WaitAction label={label} size="sm" enterKey onAction={(l) => onSend?.(l)} />
        ) : (
          <button type="button" disabled className={buttonVariants({ size: "sm" })}>
            {label}
          </button>
        )}
      </div>
    </div>
  )
}

/** A reply once it has been sent: your message in the transcript. */
export function SentReply({ step }: { step: ReplyStep }) {
  return (
    <MessageRow name={YOU} who="you">
      <p>{step.content}</p>
    </MessageRow>
  )
}

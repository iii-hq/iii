"use client"

import { type ReactNode, useEffect, useEffectEvent, useState } from "react"
import { trackCta } from "@/lib/analytics"
import { cn } from "@/lib/utils"
import type { ChatStep } from "./steps"
import { TicketCard } from "./ticket-card"
import { WaitAction } from "./wait-action"

function initials(name: string) {
  return name
    .split(" ")
    .map((w) => w[0] ?? "")
    .join("")
    .slice(0, 2)
    .toUpperCase()
}

type MessageRowProps = {
  name: string
  role?: string
  /** "you" inverts the avatar tile so your own messages read at a glance */
  who?: "them" | "you"
  children: ReactNode
}

/** One Slack-style message row: 28px avatar tile, name, role chip, body. */
export function MessageRow({ name, role, who = "them", children }: MessageRowProps) {
  return (
    <div className="flex gap-3">
      <span
        aria-hidden="true"
        className={cn(
          "flex size-7 shrink-0 items-center justify-center rounded-full text-[11px] font-medium select-none",
          who === "you" ? "bg-gray-12 text-gray-1" : "bg-gray-4 text-gray-12",
        )}
      >
        {initials(name)}
      </span>
      <div className="min-w-0 flex-1">
        <div className="flex min-h-5 items-center gap-2">
          <span className="text-[14px] font-medium text-gray-12">{name}</span>
          {role && (
            <span className="rounded-[6px] bg-gray-3 px-1.5 py-0.5 text-[11px] leading-[1.3] text-gray-10">{role}</span>
          )}
        </div>
        <div className="mt-0.5 max-w-[720px] text-[15px] leading-[1.55] text-pretty text-gray-12">{children}</div>
      </div>
    </div>
  )
}

type ChatMessageProps = {
  step: ChatStep
  index: number
  onAdvance: (label?: string) => void
  onStart: () => void
}

/** A message from Alex, with the ticket that starts the flow, a timer, or a Continue button. */
export function ChatMessage({ step, index, onAdvance, onStart }: ChatMessageProps) {
  const [advanced, setAdvanced] = useState(false)
  const { action, autoAdvance } = step

  const autoAdvanceNow = useEffectEvent(() => {
    setAdvanced(true)
    onAdvance()
  })
  useEffect(() => {
    if (action || !autoAdvance) return
    const t = setTimeout(autoAdvanceNow, autoAdvance)
    return () => clearTimeout(t)
  }, [action, autoAdvance])

  const advanceWith = (label: string) => {
    setAdvanced(true)
    onAdvance(label)
  }

  return (
    <MessageRow name={step.sender.name} role={step.sender.role}>
      <p>{step.content}</p>
      {!advanced && action && (
        <TicketCard
          ticketId={`TKT-${String(index + 1).padStart(3, "0")}`}
          title={action.title ?? step.sender.name.split(" ")[0]}
          label={action.label}
          count={action.count}
          onClick={() => {
            trackCta("experience_start", "experience", {
              cta_label: action.label.trim().toLowerCase(),
              step_id: step.id,
            })
            onStart()
            advanceWith(action.label)
          }}
        />
      )}
      {!advanced && !action && !autoAdvance && (
        <WaitAction label="Continue" variant="outline" className="mt-3" onAction={(l) => advanceWith(l ?? "")} />
      )}
    </MessageRow>
  )
}

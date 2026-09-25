"use client"

import { type HTMLMotionProps, m } from "motion/react"
import type { ReactNode } from "react"
import { Token, TokenLines } from "@/components/code/token-lines"
import { Swap } from "@/components/motion/swap"
import { keyed } from "@/lib/keys"
import type { CodeLine } from "@/lib/shiki"
import { cn } from "@/lib/utils"
import type { CommentTokens } from "../hello-world"
import { type CommentKey, type LineState, WORKERS, type WorkerId, type WorkerStatus } from "./flow-data"
import { LangMark } from "./lang-mark"

type Variant = "desktop" | "mobile"

const STATUS_LABEL: Record<WorkerStatus, string> = { running: "Running", awaiting: "Awaiting", idle: "Idle" }
const STATUS_DOT: Record<WorkerStatus, string> = { running: "bg-gray-12", awaiting: "bg-gray-8", idle: "bg-gray-6" }

type WorkerCardProps = Omit<HTMLMotionProps<"article">, "children"> & {
  worker: WorkerId
  lines: CodeLine[]
  variant?: Variant
  status: WorkerStatus
  /** line number → highlight state */
  states: Map<number, LineState>
  /** result comments to reveal (Node card only) */
  revealed?: CommentKey[]
  comments?: CommentTokens
}

/**
 * One worker as a code window: language chip and role in the title bar, a
 * status on the trailing edge, Shiki tokens below. The executing line gets a
 * band and a left rule; lines awaiting a result dim; result comments fade in
 * at the end of their line once the step fires.
 */
export function WorkerCard({
  worker,
  lines,
  variant = "desktop",
  status,
  states,
  revealed = [],
  comments,
  className,
  ...rest
}: WorkerCardProps) {
  const sample = WORKERS[worker]
  const mobile = variant === "mobile"
  const running = status === "running"

  const renderLine = (content: ReactNode, i: number) => {
    const n = i + 1
    const state = states.get(n)
    const comment = sample.comments?.find((c) => c.line === n)
    const commentTokens = comment ? comments?.[comment.key] : undefined
    return (
      <span
        className={cn(
          "block px-4 transition-[background-color,box-shadow,opacity] duration-200 ease-[ease]",
          state === "running" && "bg-gray-4 shadow-[inset_2px_0_0_var(--gray-12)]",
          state === "pending" && "opacity-50",
        )}
      >
        {content}
        {comment && commentTokens && (
          <m.span
            aria-hidden={!revealed.includes(comment.key)}
            className="italic"
            initial={false}
            animate={{ opacity: revealed.includes(comment.key) ? 1 : 0 }}
            transition={{ duration: 0.3, ease: "easeOut" }}
          >
            {" "}
            {keyed(commentTokens, (t) => t.text).map((t) => (
              <Token key={t.key} token={t.item} />
            ))}
          </m.span>
        )}
      </span>
    )
  }

  return (
    <m.article
      aria-label={`${sample.tag} worker, ${sample.role.toLowerCase()}`}
      className={cn("flex flex-col overflow-hidden rounded-[14px] bg-gray-2 shadow-panel", className)}
      {...rest}
    >
      <div className="flex h-11 shrink-0 items-center gap-3 border-b border-gray-5 px-4">
        <span
          className={cn(
            "inline-flex h-6 shrink-0 items-center gap-1.5 rounded-[8px] pr-2 pl-1.5 text-[12px] font-medium transition-[background-color,color] duration-150 ease-[ease]",
            running ? "bg-gray-12 text-gray-1" : "bg-gray-4 text-gray-12",
          )}
        >
          <LangMark lang={sample.lang} />
          {sample.tag}
        </span>
        <span className="truncate text-[13px] text-gray-10">{sample.role}</span>
        <span className="ml-auto flex shrink-0 items-center gap-2 text-[12px] text-gray-10">
          <span
            aria-hidden="true"
            className={cn(
              "size-1.5 rounded-full transition-[background-color] duration-150 ease-[ease]",
              STATUS_DOT[status],
            )}
          />
          <Swap id={status} className={cn(running && "text-gray-12")}>
            {STATUS_LABEL[status]}
          </Swap>
        </span>
      </div>
      <TokenLines
        lines={lines}
        renderLine={renderLine}
        className={cn(
          "py-4",
          // Size, leading and tracking come from TokenLines so every code surface on the page matches.
          mobile && "overflow-x-hidden whitespace-pre-wrap [word-break:break-word]",
        )}
      />
    </m.article>
  )
}

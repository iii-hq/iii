"use client"

import { type ReactNode, useEffect, useRef, useState } from "react"
import { Token } from "@/components/code/token-lines"
import { keyed } from "@/lib/keys"
import { cn } from "@/lib/utils"
import type { Line } from "./script"
import { sliceLine, sliceTokens, type TokenMap, type TypedRow } from "./timeline"

// Pieces both transcripts share: the caret, the prompt block, the way a line's
// text renders (Shiki tokens when the line is code, plain grey otherwise).

export function Caret() {
  return (
    <span
      aria-hidden="true"
      className="ml-px inline-block h-[1.05em] w-[0.55ch] translate-y-[0.18em] rounded-[1px] bg-gray-12"
    />
  )
}

/**
 * A line's text at `chars`. Code lines were tokenized on the server (`code`),
 * so they carry Shiki's colours; the rest is plain and takes the row's colour.
 */
export function LineText({ line, chars, code }: { line: Line; chars: number; code: TokenMap }) {
  const tokens = line.lang && typeof line.t === "string" ? code[line.t] : undefined
  if (!tokens) return <>{sliceLine(line, chars)}</>
  return (
    <span className="code-tokens">
      {keyed(sliceTokens(tokens, chars), (t) => t.text).map((t) => (
        <Token key={t.key} token={t.item} />
      ))}
    </span>
  )
}

/** The reader's prompt. Identical on both sides: it's the "same work". */
export function Prompt({ row, code }: { row: TypedRow; code: TokenMap }) {
  return (
    <div className="mb-1 rounded-[8px] bg-gray-3 px-3 py-2 text-gray-12" style={{ opacity: row.opacity }}>
      <LineText line={row.line} chars={row.chars} code={code} />
      {row.caret && <Caret />}
    </div>
  )
}

/** A transcript row: optional prefix glyph in the gutter colour, then the text. */
export function Row({
  row,
  code,
  prefix,
  className,
  prefixClassName,
  children,
}: {
  row: TypedRow
  code: TokenMap
  prefix?: string
  className?: string
  prefixClassName?: string
  children?: ReactNode
}) {
  return (
    <div className={cn("text-gray-11", className)} style={{ opacity: row.opacity }}>
      {prefix && <span className={cn("text-gray-9 select-none", prefixClassName)}>{prefix}</span>}
      <LineText line={row.line} chars={row.chars} code={code} />
      {row.caret && <Caret />}
      {children}
    </div>
  )
}

/**
 * The bottom-anchored transcript: what no longer fits leaves off the top, like
 * a terminal. The column is pinned by a transform rather than `justify-end`,
 * so when a line lands or wraps the text slides up over 300ms instead of
 * jumping a row. Shrinking (the loop restarting) snaps, so the first prompt
 * doesn't slide in from above.
 */
export function Transcript({ children }: { children: ReactNode }) {
  const box = useRef<HTMLDivElement>(null)
  const column = useRef<HTMLDivElement>(null)
  const [pin, setPin] = useState<{ y: number; snap: boolean }>({ y: 0, snap: true })
  const lastHeight = useRef(0)

  useEffect(() => {
    const b = box.current
    const c = column.current
    if (!b || !c) return
    const ro = new ResizeObserver(() => {
      const height = c.offsetHeight
      setPin({ y: b.clientHeight - height, snap: height < lastHeight.current })
      lastHeight.current = height
    })
    ro.observe(b)
    ro.observe(c)
    return () => ro.disconnect()
  }, [])

  return (
    <div
      ref={box}
      className="relative min-h-0 flex-1 overflow-hidden"
      style={{ maskImage: "linear-gradient(to bottom, transparent, black 32px)" }}
    >
      <div
        ref={column}
        className={cn(
          "absolute inset-x-0 top-0 flex flex-col gap-1.5 px-4 py-4 font-mono text-[14px] leading-[1.8] tracking-[0.01em] break-words whitespace-pre-wrap will-change-transform",
          !pin.snap &&
            "transition-transform duration-300 ease-[cubic-bezier(0.23,1,0.32,1)] motion-reduce:transition-none",
        )}
        style={{ transform: `translateY(${pin.y}px)` }}
      >
        {children}
      </div>
    </div>
  )
}

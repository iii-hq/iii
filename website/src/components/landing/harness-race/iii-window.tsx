import { LogoMark } from "@/components/site/logo"
import { identityKey } from "@/lib/keys"
import { cn } from "@/lib/utils"
import { RaceWindow } from "./race-window"
import { BASE_WORKERS, III, III_CPS, III_DELAY, III_DONE_AT, III_STATUS, iiiTokens } from "./script"
import { Thinking } from "./thinking"
import { clamp, easeOutCubic, isThinking, type TokenMap, type TypedRow, typeSteps } from "./timeline"
import { Caret, LineText, Prompt, Row, Transcript } from "./transcript"

// The iii console side: the same grey window, plus the worker bar the agent
// draws from and the artefacts it produces (functions, a worker, a deploy).

const chipClass =
  "inline-flex items-center rounded-[6px] px-2 py-0.5 font-mono text-[12px] leading-[1.6] transition-[background-color,color] duration-200 ease-out"

/** A worker chip. `on` latches it filled once the agent has used it; `glow` (0..1) is the flash as it's found. */
function Chip({ label, glow = 0, on = false }: { label: string; glow?: number; on?: boolean }) {
  return (
    <span
      className={cn(
        chipClass,
        on ? "bg-gray-12 text-gray-1" : "bg-gray-3 text-gray-11 shadow-[inset_0_0_0_1px_var(--gray-5)]",
      )}
      style={glow > 0 ? { boxShadow: `0 0 0 ${3 * glow}px var(--gray-7)` } : undefined}
    >
      {label}
    </span>
  )
}

/** The "pass" / "yes" badge that lands after a test or a confirmation, with a ring that settles. */
function Badge({ label, amount }: { label?: string; amount: number }) {
  return (
    <span
      className="ml-2 inline-flex items-center rounded-[5px] bg-gray-12 px-1.5 text-[11px] leading-[1.7] font-medium text-gray-1"
      style={amount > 0 ? { boxShadow: `0 0 0 ${3 * amount}px var(--gray-6)` } : undefined}
    >
      {label}
    </span>
  )
}

function IiiRow({ row, code }: { row: TypedRow; code: TokenMap }) {
  const { line, chars, caret, opacity, flash } = row
  const k = line.k

  if (k === "prompt") return <Prompt row={row} code={code} />
  // Signature, then the description under it.
  if (k === "fn")
    return (
      <div className="border-l border-gray-6 pl-3" style={{ opacity }}>
        <div className="text-gray-12">
          <LineText line={line} chars={chars} code={code} />
          {caret && <Caret />}
        </div>
        <div className="text-[12px] text-gray-10">{line.d}</div>
      </div>
    )
  if (k === "worker")
    return (
      <div
        className="flex items-center gap-3 rounded-[10px] bg-gray-3 px-3 py-2 shadow-[inset_0_0_0_1px_var(--gray-5)]"
        style={{ opacity }}
      >
        <span aria-hidden="true" className="size-1.5 rounded-full bg-gray-12" />
        <span className="text-gray-12">
          <LineText line={line} chars={chars} code={code} />
        </span>
        <span className="text-[12px] text-gray-10">node</span>
        <span className="ml-auto text-[11px] tracking-[0.08em] text-gray-9 uppercase">standalone</span>
      </div>
    )
  if (k === "test" || k === "confirm")
    return (
      <Row row={row} code={code} className={cn("flex items-center", k === "test" ? "text-gray-11" : "text-gray-12")}>
        {chars < 0 && <Badge label={k === "test" ? line.flash : line.ans} amount={flash} />}
      </Row>
    )
  if (k === "ok")
    return <Row row={row} code={code} prefix="✓ " prefixClassName="text-gray-12" className="text-gray-12" />
  if (k === "cmd") return <Row row={row} code={code} prefix="$ " className="text-gray-12" />
  return <Row row={row} code={code} className="text-gray-11" />
}

function WorkersBar({
  step,
  progress,
  glow,
  on,
}: {
  step: number
  progress: number
  glow: { label: string; amount: number } | null
  on: string[]
}) {
  const adding = step === 6
  const added = step > 6
  const p = adding ? easeOutCubic(clamp(progress / 0.5, 0, 1)) : 0
  return (
    <div className="flex shrink-0 flex-wrap items-center gap-1.5 border-b border-gray-5 px-4 py-2.5">
      <span className="mr-1.5 text-[12px] text-gray-9">Workers</span>
      {BASE_WORKERS.map((w) => (
        <Chip key={w} label={w} glow={glow?.label === w ? glow.amount : 0} on={on.includes(w)} />
      ))}
      {(adding || added) && (
        <span
          className="inline-flex origin-left"
          style={{ opacity: added ? 1 : p, transform: `scale(${added ? 1 : 0.96 + 0.04 * p})` }}
        >
          <Chip label="payments-ledger" on />
        </span>
      )}
    </div>
  )
}

type Props = {
  step: number
  progress: number
  dur: number
  code: TokenMap
  /** the traditional side's k tokens at this moment */
  versus: number
  /** wall-clock seconds into the race */
  elapsed: number
  aside?: React.ReactNode
}

export function IiiWindow({ step, progress, dur, code, versus, elapsed, aside }: Props) {
  const delay = III_DELAY[Math.min(step, III_DELAY.length - 1)]
  const rows = typeSteps(III, step, progress, dur, delay, III_CPS)
  const thinking = isThinking(step, progress, dur, delay)
  const glowRow = rows.find((r) => r.line.hl && r.flash > 0)
  const glow = glowRow?.line.hl ? { label: glowRow.line.hl, amount: glowRow.flash } : null
  // Latched workers, derived from the clock (a complete line that names the
  // worker), so any frame renders exactly from its time.
  const on = rows.flatMap((r) => (r.line.hl && r.chars === -1 ? [r.line.hl] : []))
  const last = step >= III.length - 1

  return (
    <RaceWindow
      label="iii"
      icon={<LogoMark className="size-4 shrink-0 text-gray-12" />}
      labelHidden
      tokens={iiiTokens(step, progress)}
      versus={versus}
      elapsed={last ? III_DONE_AT : elapsed}
      emphasis
      status={III_STATUS[Math.min(step, III_STATUS.length - 1)]}
      done={last}
      aside={aside}
    >
      <WorkersBar step={step} progress={progress} glow={glow} on={on} />
      <Transcript>
        {rows.map((row) => (
          <IiiRow key={identityKey(row.line)} row={row} code={code} />
        ))}
        {thinking && <Thinking t={progress * dur} />}
      </Transcript>
    </RaceWindow>
  )
}

import { cn } from "@/lib/utils"
import { USD_PER_K } from "./script"

type CostMeterProps = {
  /** k tokens used so far */
  tokens: number
  /** k tokens at the right-hand end of the bar; the same on both sides */
  max: number
  /** the iii side: the fill is ink rather than grey */
  emphasis?: boolean
  /** the other side's k tokens; shows how many times fewer this side has used */
  versus?: number
}

/**
 * The running spend along the bottom of a race window. Tokens are the big
 * number, since that's the argument; the bar puts both sides on one scale and
 * the dollar figure trails. On the iii side a pill says how many times fewer
 * tokens it has used, live. Bars are scaled transforms and every figure is
 * tabular in a fixed slot, so the row never shifts.
 */
export function CostMeter({ tokens, max, emphasis, versus }: CostMeterProps) {
  const frac = Math.min(tokens / max, 1)
  const ratio = versus && tokens > 0 ? versus / tokens : 0
  const showRatio = ratio >= 1.5
  const ratioText = ratio >= 3 ? `${Math.round(ratio)}×` : `${ratio.toFixed(1)}×`

  return (
    <div className="flex shrink-0 items-center gap-4 border-t border-gray-5 px-5 py-3 tabular-nums">
      <span className="flex shrink-0 items-baseline gap-2">
        <span className="text-[12px] text-gray-9">Tokens</span>
        <span className="min-w-[4ch] text-right text-[15px] leading-none font-medium text-gray-12">
          {Math.round(tokens)}k
        </span>
      </span>
      <span aria-hidden="true" className="relative h-1 min-w-0 flex-1 overflow-hidden rounded-full bg-gray-4">
        <span
          className={cn(
            "absolute inset-0 origin-left rounded-full transition-transform duration-500 ease-out motion-reduce:transition-none",
            emphasis ? "bg-gray-12" : "bg-gray-8",
          )}
          style={{ transform: `scaleX(${frac})` }}
        />
      </span>
      {versus !== undefined && (
        <span
          className={cn(
            "inline-flex h-6 shrink-0 items-center rounded-[6px] bg-gray-12 px-2 text-[12px] font-medium whitespace-nowrap text-gray-1",
            "transition-opacity duration-300 ease-out motion-reduce:transition-none",
            showRatio ? "opacity-100" : "opacity-0",
          )}
        >
          {ratioText} fewer
        </span>
      )}
      <span className="min-w-[5ch] shrink-0 text-right text-[13px] text-gray-10">
        ${(tokens * USD_PER_K).toFixed(2)}
      </span>
    </div>
  )
}

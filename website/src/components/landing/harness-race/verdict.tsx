import { Swap } from "@/components/motion/swap"
import { USD_PER_K } from "./script"

type VerdictProps = {
  /** k tokens each side has used */
  trad: number
  iii: number
  /** 0: reading the prompt, 1: racing, 2: iii has shipped */
  phase: 0 | 1 | 2
}

const usd = (k: number) => `$${(k * USD_PER_K).toFixed(2)}`

/**
 * One line under the pair that says what the two windows add up to. It runs
 * live while the race is on, then turns into the verdict once iii has
 * deployed. The figures are tabular so they can tick without the line moving.
 */
export function Verdict({ trad, iii, phase }: VerdictProps) {
  const ratio = iii > 0 ? trad / iii : 0
  const ratioText = ratio >= 3 ? `${Math.round(ratio)}×` : `${ratio.toFixed(1)}×`

  return (
    <p className="col-span-full flex min-h-10 items-center justify-center px-1 text-center text-[15px] leading-[1.5] text-pretty text-gray-11 sm:text-[16px]">
      <Swap id={phase}>
        {phase === 0 && <span>Same prompt, both sides.</span>}
        {phase === 1 && (
          <span className="tabular-nums">
            <span className="font-medium text-gray-12">{ratioText} fewer tokens</span> on iii so far, {usd(trad - iii)}{" "}
            less spent.
          </span>
        )}
        {phase === 2 && (
          <span className="tabular-nums">
            <span className="font-medium text-gray-12">Deployed to production for {usd(iii)}</span> instead of{" "}
            {usd(trad)}, and still counting on the left.
          </span>
        )}
      </Swap>
    </p>
  )
}

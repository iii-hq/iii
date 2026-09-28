import type { CSSProperties } from "react"
import { dayHeading } from "@/lib/spec-dates"
import type { Spec } from "@/lib/specs"
import { cn } from "@/lib/utils"

const W = 480
const H = 260
const MAIN_Y = 168
const BRANCH_Y = 96
const LEFT = 36
const RIGHT = 400

/**
 * The roadmap as a commit graph. Shipped specs are commits on `main`, placed
 * by date and drawn in left to right. Specs still in draft sit on an open
 * branch above, hollow, with the branch curving off after the newest commit
 * and running dashed toward what comes next. Labels are mono, as in a
 * terminal. Built from the real specs, so it is never out of date; draws in
 * once its box is on screen (`.motif`).
 */
export function RoadmapGraph({ specs, className }: { specs: Spec[]; className?: string }) {
  const dated = specs
    .filter((s) => s.date)
    .slice()
    .sort((a, b) => (a.date < b.date ? -1 : 1))
  if (!dated.length) return null
  const t = (iso: string) => new Date(`${iso}T00:00:00Z`).getTime()
  const min = t(dated[0].date)
  const max = t(dated[dated.length - 1].date)
  const x = (iso: string) => (max === min ? (LEFT + RIGHT) / 2 : LEFT + ((t(iso) - min) / (max - min)) * (RIGHT - LEFT))
  const live = dated.filter((s) => s.status === "live")
  const drafts = dated.filter((s) => s.status !== "live")
  const lastLive = live[live.length - 1]
  const forkX = lastLive ? x(lastLive.date) : LEFT
  const delay = (i: number) => ({ "--d": `${i * 120}ms` }) as CSSProperties
  let step = 0

  // Labels for commits closer than a label's width step onto a second tier.
  const tierFor = (list: Spec[]) => {
    const out: number[] = []
    list.forEach((s, i) => {
      const prev = i > 0 ? x(list[i - 1].date) : -Infinity
      out.push(x(s.date) - prev < 52 && out[i - 1] === 0 ? 1 : 0)
    })
    return out
  }
  const liveTier = tierFor(live)
  const draftTier = tierFor(drafts)

  return (
    <svg viewBox={`0 0 ${W} ${H}`} aria-hidden="true" className={cn("motif overflow-visible", className)}>
      <g fill="none" stroke="currentColor" strokeWidth="1.25" strokeLinecap="round" strokeLinejoin="round">
        {/* main */}
        <path d={`M ${LEFT - 16} ${MAIN_Y} H ${forkX + 40}`} pathLength={1} data-draw="" style={delay(step++)} />
        {/* the branch: leaves main after the newest commit, rises, runs to the right */}
        {drafts.length > 0 && (
          <path
            d={`M ${forkX} ${MAIN_Y} C ${forkX + 28} ${MAIN_Y} ${forkX + 20} ${BRANCH_Y} ${forkX + 48} ${BRANCH_Y} H ${RIGHT}`}
            pathLength={1}
            data-draw=""
            style={delay(step++)}
          />
        )}
        {/* what comes next, dashed */}
        <path
          d={`M ${drafts.length ? RIGHT : forkX + 40} ${drafts.length ? BRANCH_Y : MAIN_Y} H ${W - 12}`}
          pathLength={1}
          data-draw=""
          style={delay(step++)}
          className="opacity-40"
          strokeDasharray="1"
        />
      </g>

      {/* commits on main */}
      {live.map((s, i) => {
        const cx = x(s.date)
        const lift = liveTier[i] * 20
        return (
          <g key={s.slug} data-fill="" style={delay(step + i)}>
            <circle cx={cx} cy={MAIN_Y} r="5" className="fill-gray-12" />
            <path d={`M ${cx} ${MAIN_Y + 8} V ${MAIN_Y + 22 + lift}`} className="stroke-gray-7" strokeWidth="1" />
            <text
              x={cx}
              y={MAIN_Y + 36 + lift}
              textAnchor="middle"
              className="fill-gray-10 font-mono text-[11px] tracking-[0.04em]"
            >
              {dayHeading(s.date).toLowerCase()}
            </text>
          </g>
        )
      })}

      {/* commits on the branch */}
      {drafts.map((s, i) => {
        const cx = Math.max(x(s.date), forkX + 64)
        const lift = draftTier[i] * 20
        return (
          <g key={s.slug} data-fill="" style={delay(step + live.length + i)}>
            <circle cx={cx} cy={BRANCH_Y} r="5" className="fill-gray-1 stroke-gray-12" strokeWidth="1.25" />
            <path d={`M ${cx} ${BRANCH_Y - 8} V ${BRANCH_Y - 22 - lift}`} className="stroke-gray-7" strokeWidth="1" />
            <text
              x={cx}
              y={BRANCH_Y - 28 - lift}
              textAnchor="middle"
              className="fill-gray-10 font-mono text-[11px] tracking-[0.04em]"
            >
              {dayHeading(s.date).toLowerCase()}
            </text>
          </g>
        )
      })}

      {/* refs, as a terminal would print them */}
      <g className="font-mono text-[11px] tracking-[0.04em]" data-fill="" style={delay(step + dated.length)}>
        <text x={LEFT - 16} y={MAIN_Y - 14} className="fill-gray-9">
          main
        </text>
        {drafts.length > 0 && (
          <text x={forkX + 48} y={BRANCH_Y - 14 - (draftTier[0] ? 20 : 0) - 20} className="fill-gray-9">
            draft
          </text>
        )}
        {lastLive && (
          <>
            <rect
              x={forkX - 24}
              y={MAIN_Y + 46 + (liveTier[live.length - 1] ? 20 : 0)}
              width="48"
              height="18"
              rx="4"
              className="fill-gray-12"
            />
            <text
              x={forkX}
              y={MAIN_Y + 59 + (liveTier[live.length - 1] ? 20 : 0)}
              textAnchor="middle"
              className="fill-gray-1"
            >
              latest
            </text>
          </>
        )}
        <text x={W - 12} y={(drafts.length ? BRANCH_Y : MAIN_Y) - 14} textAnchor="end" className="fill-gray-9">
          next
        </text>
      </g>
    </svg>
  )
}

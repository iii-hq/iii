import { identityKey } from "@/lib/keys"
import { RaceWindow } from "./race-window"
import { TRAD, TRAD_CPS, TRAD_DELAY, TRAD_STATUS, tradTokens } from "./script"
import { Thinking } from "./thinking"
import { isThinking, type TokenMap, typeSteps } from "./timeline"
import { Prompt, Row, Transcript } from "./transcript"

// The traditional coding agent. Same grey window as the iii side: the
// difference is what happens inside it, not the paint.

type Props = {
  step: number
  progress: number
  dur: number
  code: TokenMap
  /** wall-clock seconds into the race */
  elapsed: number
  aside?: React.ReactNode
}

export function AgentWindow({ step, progress, dur, code, elapsed, aside }: Props) {
  const delay = TRAD_DELAY[Math.min(step, TRAD_DELAY.length - 1)]
  const rows = typeSteps(TRAD, step, progress, dur, delay, TRAD_CPS)
  const thinking = isThinking(step, progress, dur, delay)
  const last = step >= TRAD.length - 1

  return (
    <RaceWindow
      label="Traditional agent"
      tokens={tradTokens(step, progress)}
      elapsed={elapsed}
      status={TRAD_STATUS[Math.min(step, TRAD_STATUS.length - 1)]}
      aside={aside}
    >
      <Transcript>
        {rows.map((row) => {
          const k = row.line.k
          const key = identityKey(row.line)
          if (k === "prompt") return <Prompt key={key} row={row} code={code} />
          if (k === "choice" && Array.isArray(row.line.t))
            return (
              <div key={key} className="flex flex-wrap gap-x-5 text-gray-10" style={{ opacity: row.opacity }}>
                {row.line.t.map((c) => (
                  <span key={c}>[ ] {c}</span>
                ))}
              </div>
            )
          const prefix =
            k === "tool"
              ? "* "
              : k === "read"
                ? "→ "
                : k === "cmd"
                  ? "$ "
                  : k === "ask"
                    ? "~ "
                    : k === "warn"
                      ? "! "
                      : ""
          return (
            <Row
              key={key}
              row={row}
              code={code}
              prefix={prefix}
              prefixClassName={k === "warn" ? "text-gray-12" : undefined}
              className={
                k === "warn"
                  ? "text-gray-12"
                  : k === "del"
                    ? "text-gray-9 line-through"
                    : k === "muted"
                      ? "text-gray-11"
                      : "text-gray-10"
              }
            />
          )
        })}
        {/* The loop's last scene: the agent is still at it. */}
        {(thinking || last) && <Thinking t={progress * dur} label={last ? "Resolving conflicts" : "Thinking"} />}
      </Transcript>
    </RaceWindow>
  )
}

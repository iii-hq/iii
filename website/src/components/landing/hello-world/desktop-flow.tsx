"use client"

import type { CommentTokens, WorkerCode } from "../hello-world"
import { Controls, type FlowControlsProps, Timeline } from "./flow-controls"
import { FLOW_STEPS, lineStates, workerStatus } from "./flow-data"
import { WorkerCard } from "./worker-card"

const EASE_OUT = [0.23, 1, 0.32, 1] as const
// The card whose turn it is sits in front; the other rests behind it, a little
// smaller and higher, so its top edge peeks out like the next card in a deck.
const FRONT = { y: 0, scale: 1, opacity: 1 }
const BACK = { y: -14, scale: 0.96, opacity: 0.6 }
const SWAP = { duration: 0.45, ease: EASE_OUT }

type DesktopFlowProps = FlowControlsProps & { code: WorkerCode; comments: CommentTokens }

/** ≥1024px: the Node window beside a Rust/Python deck, then the timeline and controls. */
export function DesktopFlow({ code, comments, index, playing, onPrev, onNext, onToggle, onSeek }: DesktopFlowProps) {
  const step = FLOW_STEPS[index]

  return (
    <div className="max-lg:hidden">
      <div className="grid grid-cols-2 items-start gap-6 pt-4">
        <WorkerCard
          worker="ts"
          lines={code.ts}
          status={workerStatus(step, "ts")}
          states={lineStates(step, "ts")}
          revealed={step.comments}
          comments={comments}
        />
        {/* Both workers share one grid cell; the front one is decided by the step. */}
        <div className="grid">
          {(["rs", "py"] as const).map((id) => {
            const front = step.top === id
            return (
              <WorkerCard
                key={id}
                worker={id}
                lines={code[id]}
                status={workerStatus(step, id)}
                states={lineStates(step, id)}
                aria-hidden={!front}
                className="col-start-1 row-start-1 h-full origin-top"
                style={{ zIndex: front ? 2 : 1 }}
                initial={false}
                animate={front ? FRONT : BACK}
                transition={SWAP}
              />
            )
          })}
        </div>
      </div>

      <Timeline index={index} onSeek={onSeek} className="mt-6" />
      <Controls index={index} playing={playing} onPrev={onPrev} onNext={onNext} onToggle={onToggle} className="mt-1" />
    </div>
  )
}

'use client'

import { useState } from 'react'

import { IconCheckCircle } from '@/components/icons/iconly'
import { cn } from '@/lib/utils'
import { useCases } from './content'
import styles from './use-cases.module.css'
import { secs, usePlayhead } from './use-cases-motion'

/**
 * The three card visuals. Each loops on its own playhead while the section is on screen. Steps share one grammar:
 * the braille spinner from the `iii compose` renderer while something runs, then a filled check. Rows arrive when
 * they start (no dimmed placeholders), so nothing on the card ever looks washed out.
 */

type StepState = 'waiting' | 'running' | 'done'
const stateAt = (ms: number, start: number, run: number): StepState =>
  ms < start ? 'waiting' : ms < start + run ? 'running' : 'done'

function Mark({ state, frame }: { state: StepState; frame: string }) {
  return (
    <span className={styles.mark} data-state={state} aria-hidden>
      <span className={styles.spin}>{frame}</span>
      <IconCheckCircle className={cn(styles.check, 'size-[15px]')} />
    </span>
  )
}

/** The last moments of a lap: the visual fades out softly before it starts over, instead of snapping back. */
const OUTRO_MS = 420
const leaving = (ms: number, total: number, still: boolean) => !still && ms >= total - OUTRO_MS

/** Loops a timeline of `total` ms: the key changes on every lap so the playhead starts over. */
function useLoop(running: boolean, total: number) {
  const [lap, setLap] = useState(0)
  const ms = usePlayhead(running, String(lap), total, () => setLap((n) => n + 1))
  return { ms, lap }
}

/* ---------- Custom harness: pick the pieces, then the session runs on them ---------- */

const H = useCases.harness.session
/* Real registry workers for each choice; every lap lands on a different combination, which is the point. */
const CHOICES = [
  { label: 'Model', options: ['provider-anthropic', 'provider-openai', 'provider-deepseek'] },
  { label: 'Tools', options: ['github', 'browser', 'iii-sandbox'] },
  { label: 'Session', options: ['session-manager', 'context-manager', 'judge'] },
] as const
/* A slot that decelerates: six swaps whose gaps widen (t = D·(k/N)^1.6, so ~50ms at first and ~220ms before it
   lands), the last swap landing exactly on the final value. Rows land one after another. */
const ROLL_TICKS = 6
const LAND_AT = (row: number) => 950 + row * 420
const ASK_AT = 2300
/** The value index a rolling row shows at `ms`, counting back from the one it lands on. */
const rollIndex = (ms: number, land: number, final: number, length: number) => {
  const passed = Math.min(ROLL_TICKS, Math.floor(ROLL_TICKS * (Math.max(0, ms) / land) ** (1 / 1.6)))
  return (((final - (ROLL_TICKS - passed)) % length) + length) % length
}
const STEP_AT = (i: number) => 3000 + i * 760
const STEP_RUN = 520
const RESULT_AT = STEP_AT(H.steps.length) + 100
const HARNESS_TOTAL = RESULT_AT + 2600

export function HarnessVisual({ running, frame, still }: { running: boolean; frame: string; still: boolean }) {
  const { ms: playhead, lap } = useLoop(running, HARNESS_TOTAL)
  const ms = still ? Number.POSITIVE_INFINITY : playhead
  return (
    <div className={styles.visualBody} data-leaving={leaving(playhead, HARNESS_TOTAL, still)}>
      <ul className={styles.choices}>
        {CHOICES.map((choice, row) => {
          const landed = ms >= LAND_AT(row)
          const finalIndex = (lap + row) % choice.options.length
          const value = landed
            ? choice.options[finalIndex]
            : choice.options[rollIndex(ms, LAND_AT(row), finalIndex, choice.options.length)]
          return (
            <li key={choice.label} className={styles.choice} data-landed={landed}>
              <span className={styles.choiceLabel}>{choice.label}</span>
              <span className={styles.choiceValue}>
                <code key={value} className={styles.roll}>
                  {value}
                </code>
              </span>
              <span className={styles.choiceMark} data-on={landed} aria-hidden>
                <IconCheckCircle className="size-[15px]" />
              </span>
            </li>
          )
        })}
      </ul>

      <div className={styles.miniSession} data-on={ms >= ASK_AT}>
        <p className={styles.ask}>{H.request}</p>
        <ol className={styles.steps}>
          {H.steps.map((step, i) => {
            const state = stateAt(ms, STEP_AT(i), STEP_RUN)
            return (
              <li key={step} className={styles.step} data-state={state}>
                <Mark state={state} frame={frame} />
                <span>{step}</span>
              </li>
            )
          })}
        </ol>
        <p className={styles.result} data-on={ms >= RESULT_AT}>
          {H.result}
        </p>
      </div>
    </div>
  )
}

/* ---------- App platform: one workload at a time, each run traced call by call ---------- */

const WORKLOADS = useCases.platform.workloads
type Workload = (typeof WORKLOADS)[number]
const CALL_AT = (i: number) => 450 + i * 620
const CALL_RUN = 400
const workloadTotal = (w: Workload) => CALL_AT(w.calls.length) + 1900

const duration = (value: number) =>
  value >= 60_000 ? `${(value / 60_000).toFixed(1)}m` : value >= 1000 ? `${(value / 1000).toFixed(2)}s` : `${value}ms`

export function PlatformVisual({ running, frame, still }: { running: boolean; frame: string; still: boolean }) {
  const [index, setIndex] = useState(0)
  const workload = WORKLOADS[index]
  const playhead = usePlayhead(running, `${index}`, workloadTotal(workload), () =>
    setIndex((n) => (n + 1) % WORKLOADS.length),
  )
  const ms = still ? Number.POSITIVE_INFINITY : playhead
  /* Log scale, so a 3ms write and a three-minute epoch both read. */
  const longest = Math.log10(Math.max(...workload.calls.map((call) => call.ms)) + 1)
  const done = ms >= CALL_AT(workload.calls.length)
  return (
    <div className={styles.visualBody} data-leaving={leaving(playhead, workloadTotal(workload), still)}>
      <div key={workload.id} className={styles.workloadHead}>
        <span className={styles.trigger}>
          <code>{workload.trigger.type}</code>
          <code className={styles.triggerDetail}>{workload.trigger.detail}</code>
        </span>
        <span className={styles.workloadName}>{workload.label}</span>
      </div>

      <ol key={`${workload.id}-calls`} className={styles.calls}>
        {workload.calls.map((call, i) => {
          const state = stateAt(ms, CALL_AT(i), CALL_RUN)
          return (
            <li key={call.fn} className={styles.call} data-state={state}>
              <Mark state={state} frame={frame} />
              <span className={styles.callText}>
                <code>{call.fn}</code>
                <span>{call.worker}</span>
              </span>
              <span className={styles.callMs}>{state === 'done' ? duration(call.ms) : ''}</span>
              <span className={styles.callTrack} aria-hidden>
                <span
                  style={{
                    transform: `scaleX(${state === 'done' ? (Math.log10(call.ms + 1) / longest).toFixed(3) : 0})`,
                  }}
                />
              </span>
            </li>
          )
        })}
      </ol>
      <p className={styles.result} data-on={done}>
        {workload.result}
      </p>

      <fieldset className={styles.pager}>
        <legend className="sr-only">Workloads</legend>
        {WORKLOADS.map((item, i) => (
          <button
            key={item.id}
            type="button"
            aria-label={item.label}
            aria-pressed={i === index}
            onClick={() => setIndex(i)}
            className={styles.pagerDot}
          />
        ))}
      </fieldset>
    </div>
  )
}

/* ---------- Infrastructure: the `iii compose` renderer bringing the whole stack up ---------- */

const T = useCases.infra.terminal
const T_TYPE_MS = 45
const T_START = T.command.length * T_TYPE_MS + 250
const T_ENGINE_RUN = 450
const T_WORKERS_AT = T_START + T_ENGINE_RUN + 120
const T_STRETCH = 8
const isDep = (worker: (typeof T.workers)[number]) => 'dep' in worker && worker.dep
/** Workers start a beat apart; harness waits until every dependency under it is ready. */
const SCHEDULE = (() => {
  let top = 0
  let dep = 0
  const rows = T.workers.map((worker) => {
    const start = isDep(worker) ? T_WORKERS_AT + 240 + dep++ * 80 : T_WORKERS_AT + top++ * 100
    return { name: worker.name, start, finish: start + worker.ms * T_STRETCH }
  })
  const lastDep = Math.max(...rows.filter((_, i) => isDep(T.workers[i])).map((row) => row.finish))
  return rows.map((row) => (row.name === 'harness' ? { ...row, finish: lastDep + 320 } : row))
})()
const T_DONE_AT = Math.max(...SCHEDULE.map((row) => row.finish)) + 200
const INFRA_TOTAL = T_DONE_AT + 3200
const LAST_DEP = [...T.workers].reverse().find(isDep)?.name

export function InfraVisual({ running, frame, still }: { running: boolean; frame: string; still: boolean }) {
  const { ms: playhead } = useLoop(running, INFRA_TOTAL)
  const ms = still ? Number.POSITIVE_INFINITY : playhead
  const started = ms >= T_START
  const engine = stateAt(ms, T_START, T_ENGINE_RUN)
  const states = SCHEDULE.map((row): StepState => (ms < row.start ? 'waiting' : ms < row.finish ? 'running' : 'done'))
  const ready = states.filter((state) => state === 'done').length
  const done = ms >= T_DONE_AT
  const containers: StepState = done ? 'done' : ms >= T_WORKERS_AT ? 'running' : 'waiting'
  const progress = started ? (ready + (engine === 'done' ? 1 : 0)) / (T.workers.length + 1) : 0
  return (
    <div className={cn(styles.visualBody, styles.terminal)} data-leaving={leaving(playhead, INFRA_TOTAL, still)}>
      <p className={styles.tLine}>
        <span className={styles.tPrompt}>$</span>
        <span className={styles.tBright}>{T.command.slice(0, Math.floor(ms / T_TYPE_MS))}</span>
        {!started ? <span aria-hidden className={styles.tCursor} /> : null}
      </p>
      <div className={styles.tProgress} aria-hidden>
        <span style={{ transform: `scaleX(${progress.toFixed(3)})` }} />
      </div>
      <div className={styles.tBlock} data-on={started}>
        <p className={styles.tLine} data-state={engine}>
          <TMark state={engine} frame={frame} />
          <span className={styles.tBright}>Engine</span>
          <span className={styles.tStatus}>{engine === 'done' ? `Ready · ${T.engine}` : 'Starting'}</span>
        </p>
        <p className={styles.tLine} data-state={containers}>
          <TMark state={containers} frame={frame} />
          <span className={styles.tBright}>Containers</span>
          <span className={styles.tStatus}>
            {done ? 'Running' : 'Starting'} {ready}/{T.workers.length}
            {started && !done ? ` · ${secs(ms - T_START)}` : ''}
          </span>
        </p>
        <ul className={styles.tWorkers}>
          {T.workers.map((worker, i) => (
            <li key={worker.name} className={styles.tLine} data-state={states[i]} data-dep={isDep(worker) || undefined}>
              {isDep(worker) ? <span className={styles.tTree}>{worker.name === LAST_DEP ? '└' : '├'}</span> : null}
              <TMark state={states[i]} frame={frame} />
              <span className={styles.tBright}>{worker.name}</span>
              <span className={styles.tStatus}>{states[i] === 'done' ? `${worker.ms}ms` : ''}</span>
            </li>
          ))}
        </ul>
        <p className={styles.tLine} data-on={done}>
          <span className={styles.tPrompt}>›</span>
          <span className={styles.tBright}>{T.done}</span>
        </p>
      </div>
    </div>
  )
}

function TMark({ state, frame }: { state: StepState; frame: string }) {
  return (
    <span className={styles.tMark} data-state={state} aria-hidden>
      {state === 'done' ? '✓' : state === 'running' ? frame : '·'}
    </span>
  )
}

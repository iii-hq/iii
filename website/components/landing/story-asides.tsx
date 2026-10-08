'use client'

import type { ReactNode } from 'react'

import { IconCheckCircle, IconTerminal } from '@/components/icons/iconly'
import { cn } from '@/lib/utils'
import styles from './story.module.css'
import { CommandLine, commandShown, commandTypeMs } from './story-command'
import type { StoryStage } from './story-model'
import { useBeatClock } from './use-beat-clock'

/** Only Composability has a card beside its copy; every other chapter tells its story in the one figure on the right. */
export function StoryActivity({ stage, beat, running }: { stage: StoryStage; beat: number; running: boolean }) {
  return stage === 'compose' ? <ComposeActivity beat={beat} running={running} /> : null
}

function ActivityFrame({
  label,
  icon,
  status,
  children,
  footer,
}: {
  label: string
  icon: ReactNode
  /** Right side of the header; defaults to the "Example" note. */
  status?: ReactNode
  children: ReactNode
  footer: ReactNode
}) {
  return (
    <figure className={styles.activity}>
      <figcaption className={styles.activityHeader}>
        <span className={styles.activityIcon}>{icon}</span>
        <span className={styles.activityLabel}>{label}</span>
        <span className={styles.activityStatus}>{status ?? 'Example'}</span>
      </figcaption>
      <div className={styles.activityBody}>{children}</div>
      <div className={styles.activityFooter}>
        <span key={String(footer)} className="swap-in">
          {footer}
        </span>
      </div>
    </figure>
  )
}

/** A worker that Compose starts: the check pops in once it registers, the row lifts from dimmed to full. */
function WorkerRow({ ready, name, detail, meta }: { ready: boolean; name: string; detail: string; meta?: string }) {
  return (
    <li className={styles.workerRow} data-ready={ready}>
      <span className={styles.workerState} aria-hidden>
        <span className={styles.workerPending} />
        <IconCheckCircle className={cn(styles.workerCheck, 'size-4')} />
      </span>
      <code className={styles.workerName}>{name}</code>
      <span className={styles.workerDetail}>{detail}</span>
      {meta ? <span className={styles.workerMeta}>{meta}</span> : null}
      <span className="sr-only">{ready ? 'ready' : 'waiting'}</span>
    </li>
  )
}

type StepState = 'idle' | 'active' | 'done'

/** One numbered step on the stepper rail: number, then a filled check once all of its workers are ready. */
function Step({
  index,
  state,
  title,
  aside,
  children,
}: {
  index: number
  state: StepState
  title: string
  aside?: string
  children: ReactNode
}) {
  return (
    <li className={styles.step} data-state={state}>
      <span className={styles.stepMarker} aria-hidden>
        <span className={styles.stepNumber}>{index}</span>
        <IconCheckCircle className={cn(styles.stepCheck, 'size-[22px]')} />
      </span>
      <div className={styles.stepBody}>
        <p className={styles.stepTitle}>
          <span>{title}</span>
          {aside ? <span className={styles.stepAside}>{aside}</span> : null}
        </p>
        {children}
      </div>
    </li>
  )
}

const COMPOSE_STEPS = [
  {
    title: 'Add from the registry',
    aside: 'Registry workers',
    command: 'iii trigger compose::add worker=http worker=database',
    from: 1,
    to: 2,
    workers: [
      { name: 'http', detail: 'HTTP endpoints', meta: 'Rust', at: 1 },
      { name: 'database', detail: 'SQL queries', meta: 'Rust', at: 2 },
    ],
  },
  {
    title: 'Connect your code',
    aside: 'Your worker',
    command: 'iii trigger compose::add worker=./workers/api',
    from: 3,
    to: 3,
    workers: [{ name: 'api', detail: 'orders::create', meta: 'Python', at: 3 }],
  },
] as const

function ComposeActivity({ beat, running }: { beat: number; running: boolean }) {
  const ms = useBeatClock(beat, running)
  return (
    <ActivityFrame
      label="Compose"
      icon={<IconTerminal className="size-4" />}
      status={
        <span className={styles.engineStatus}>
          <span aria-hidden className={styles.engineDot} />
          Engine running
        </span>
      }
      footer={
        beat >= 6
          ? 'Three workers, two languages. Every call goes through iii.'
          : 'Add registry workers, then connect your own code.'
      }
    >
      <ol className={styles.steps}>
        {COMPOSE_STEPS.map((step, i) => {
          const state: StepState = beat > step.to ? 'done' : beat >= step.from ? 'active' : 'idle'
          /* The command types on its step's first beat; the first worker registers once the line is complete. */
          const typing = beat === step.from
          const typedOut = !typing || ms >= commandTypeMs(step.command)
          return (
            <Step key={step.title} index={i + 1} state={state} title={step.title} aside={step.aside}>
              <CommandLine
                command={step.command}
                shown={beat < step.from ? 0 : typing ? commandShown(ms) : undefined}
              />
              <ul className={styles.workerList}>
                {step.workers.map((worker) => (
                  <WorkerRow
                    key={worker.name}
                    ready={beat > worker.at || (beat === worker.at && typedOut)}
                    name={worker.name}
                    detail={worker.detail}
                    meta={worker.meta}
                  />
                ))}
              </ul>
            </Step>
          )
        })}
      </ol>
    </ActivityFrame>
  )
}

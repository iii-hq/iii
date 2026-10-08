'use client'

import { type CSSProperties, Fragment, useId, useState } from 'react'
import {
  IconArrowLeftCircle,
  IconArrowRightCircle,
  IconBot,
  IconCheckCircle,
  IconCode,
  IconGitHub,
  IconGraph,
  IconSwap,
} from '@/components/icons/iconly'
import { cn } from '@/lib/utils'
import styles from './live-demo.module.css'
import { DEMO_BEATS, DEMO_DONE, DEMO_TRAVEL_MS, DEMO_WORKERS, type Endpoint } from './live-demo-code'

const icons = { orchestrator: IconCode, github: IconGitHub, router: IconSwap, provider: IconBot }

// Every route enters the engine at (300, 210), including nested provider calls and replies.
const arms: Record<Endpoint, { in: string; out: string }> = {
  request: { in: 'M 300 12 L 300 210', out: 'L 300 12' },
  orchestrator: { in: 'M 150 84 C 150 185 300 145 300 210', out: 'C 300 145 150 185 150 84' },
  github: { in: 'M 450 84 C 450 185 300 145 300 210', out: 'C 300 145 450 185 450 84' },
  router: { in: 'M 150 336 C 150 235 300 275 300 210', out: 'C 300 275 150 235 150 336' },
  provider: { in: 'M 450 336 C 450 235 300 275 300 210', out: 'C 300 275 450 235 450 336' },
}

type Props = {
  step: number
  running: boolean
}

export function ExecutionGraph({ step, running }: Props) {
  const id = useId()
  const beat = DEMO_BEATS[step]
  const done = step === DEMO_DONE
  const route = beat ? `${arms[beat.from].in} ${arms[beat.to].out}` : null
  /* The receiver lights up when the packet lands on it, not when it leaves the sender. */
  const [arrivedAt, setArrivedAt] = useState(-1)
  const arrived = arrivedAt === step
  const playState = running ? 'running' : 'paused'
  const travel = { '--travel': `${DEMO_TRAVEL_MS}ms`, animationPlayState: playState } as CSSProperties
  const trail = `${id}-trail-${step}`
  return (
    <div className={styles.graphPanel}>
      <div className={styles.panelHeader}>
        <span className="flex items-center gap-2">
          <IconGraph className="size-3.5" />
          Worker graph
        </span>
        <span className="text-[11px]">4 workers · 2 languages</span>
      </div>
      <div className={styles.graph}>
        <svg className={styles.wires} viewBox="0 0 600 420" preserveAspectRatio="none" aria-hidden="true">
          {Object.entries(arms).map(([id, arm]) => (
            <path key={id} d={arm.in} className={styles.wire} strokeDasharray={id === 'request' ? '3 5' : undefined} />
          ))}
          {route ? (
            <g key={step}>
              {/* The lit route draws in behind the packet (same path, same timing) instead of appearing all at once. */}
              <mask id={trail} maskUnits="userSpaceOnUse" x={-20} y={-20} width={640} height={460}>
                <path d={route} pathLength={1} className={styles.trail} style={travel} />
              </mask>
              <path
                d={route}
                className={styles.activeWire}
                strokeDasharray={beat.reply ? '4 4' : undefined}
                mask={`url(#${trail})`}
              />
              <circle
                r="3.5"
                className={styles.packet}
                style={{ ...travel, offsetPath: `path('${route}')` }}
                onAnimationEnd={() => setArrivedAt(step)}
              />
            </g>
          ) : null}
        </svg>
        <span className="absolute top-1 left-1/2 -translate-x-1/2 bg-background px-2 text-[11px] text-muted-foreground">
          request
        </span>
        {DEMO_WORKERS.map((worker) => {
          const Icon = icons[worker.id]
          const sending = beat?.from === worker.id
          const receiving = beat?.to === worker.id && arrived
          const active = sending || receiving
          return (
            <div
              key={worker.id}
              className={styles.node}
              style={{ left: `${worker.x}%`, top: `${worker.y}%` }}
              data-active={active}
            >
              <span className={styles.nodeTop}>
                <Icon className="size-3.5 shrink-0" />
                <span className={styles.nodeName}>{worker.name}</span>
                {done ? (
                  <IconCheckCircle className="swap-in ml-auto hidden size-3 shrink-0 text-hero-accent sm:block" />
                ) : null}
              </span>
              <span className={styles.nodeFunction}>
                {worker.fn.split('::').map((part, index) => (
                  <Fragment key={part}>
                    {index > 0 ? (
                      <>
                        ::
                        <wbr />
                      </>
                    ) : null}
                    {part}
                  </Fragment>
                ))}
              </span>
              <span className={styles.nodeMeta}>
                <span>
                  {worker.language} · {worker.location}
                </span>
                <span className={cn('transition-colors duration-200', active && 'text-hero-accent')}>
                  {receiving ? 'receiving' : sending ? 'sending' : worker.id === 'orchestrator' ? 'custom' : 'registry'}
                </span>
              </span>
            </div>
          )
        })}
        <div className={styles.engine} data-active={Boolean(beat)}>
          <span className={styles.engineMark} aria-hidden>
            <span />
            <span />
            <span />
          </span>
          <div>
            <p className="text-[13px] font-medium">iii engine</p>
            <p className="mt-0.5 text-[11px] text-muted-foreground">{done ? 'Run complete' : 'Routes every call'}</p>
          </div>
        </div>
      </div>
      <div className={styles.graphCaption}>
        {/* Keyed on the step so each new caption rises in with the page's shared content-swap motion. */}
        <span
          key={`icon-${step}`}
          className="swap-in mt-px flex size-6 shrink-0 items-center justify-center text-hero-accent"
        >
          {done ? (
            <IconCheckCircle className="size-5" />
          ) : beat.reply ? (
            <IconArrowLeftCircle className="size-5" />
          ) : (
            <IconArrowRightCircle className="size-5" />
          )}
        </span>
        <div key={`text-${step}`} className="swap-in">
          <p className="text-[13px] font-medium">{done ? 'The digest is ready.' : beat.title}</p>
          <p className="mt-1 text-[12px] leading-relaxed text-muted-foreground">
            {done ? 'The finished digest returns to the caller through iii.' : beat.detail}
          </p>
        </div>
      </div>
    </div>
  )
}

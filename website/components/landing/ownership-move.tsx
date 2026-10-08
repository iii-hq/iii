'use client'

import { useEffect, useRef, useState } from 'react'

import { DemoPlayback } from '@/components/graphics/demo-playback'
import { IconCloud, IconMonitor, IconServer } from '@/components/icons/iconly'
import { Logo } from '@/components/site/logo'
import { useDemoPlayback } from '@/hooks/use-demo-playback'
import { easeInOut } from '@/lib/motion'

import { ownership } from './content'
import styles from './ownership-move.module.css'

const ICONS = [IconMonitor, IconServer, IconCloud]
/** One hop at a time, there and back: Local → Self-hosted → Cloud → Self-hosted → Local. Never a two-slot flight. */
const PATH = [0, 1, 2, 1]
const HOP_MS = 2800
/** Matches the carrier's CSS transition, so the lift peaks mid-flight and settles as it lands. */
const MOVE_MS = 700

/**
 * The engine picks itself up from one environment and drops into the next with its graph intact (the doc's
 * brief). The card lifts slightly while it travels, the slot it leaves dims at once, and the slot it lands
 * in lights as it arrives. Loops on its own with a Pause control; reduced motion parks it on Local.
 */
export function OwnershipMove() {
  const { ref, running, paused, setPaused, reduce } = useDemoPlayback<HTMLDivElement>()
  const [step, setStep] = useState(0)
  const at = PATH[step % PATH.length]

  useEffect(() => {
    if (!running) return
    const id = window.setInterval(() => setStep((s) => s + 1), HOP_MS)
    return () => window.clearInterval(id)
  }, [running])

  const card = useRef<HTMLDivElement>(null)
  const landed = useRef(at)
  useEffect(() => {
    if (landed.current === at) return
    landed.current = at
    card.current?.animate(
      [
        { transform: 'translateY(0) scale(1)' },
        { transform: 'translateY(-6px) scale(1.02)', offset: 0.4 },
        { transform: 'translateY(0) scale(1)' },
      ],
      { duration: MOVE_MS, easing: `cubic-bezier(${easeInOut.join(', ')})` },
    )
  }, [at])

  const { environments, app } = ownership
  return (
    <div ref={ref} className="graphic-stage">
      <p className="sr-only">
        The {app.engine} and its workers ({app.workers.join(', ')}) moving unchanged between{' '}
        {environments.map((e) => e.name).join(', ')}.
      </p>
      <div className={styles.track} style={{ '--at': at } as React.CSSProperties}>
        {environments.map((env, i) => {
          const Icon = ICONS[i]
          return (
            <div key={env.name} className={styles.slot} data-here={i === at}>
              <div className="flex items-center gap-2.5">
                <Icon className={`${styles.icon} size-4 shrink-0`} />
                <p className={`${styles.name} text-[14px] leading-none`}>{env.name}</p>
                <p className="text-[12.5px] text-muted-foreground leading-none">{env.where}</p>
              </div>
            </div>
          )
        })}

        <div aria-hidden className={styles.carrier}>
          <div ref={card} className={styles.card}>
            <div className={styles.graph}>
              <div className={styles.engine}>
                <Logo className="h-4" />
                {app.engine}
              </div>
              <ul className={styles.workers}>
                {app.workers.map((w) => (
                  <li key={w} className={styles.worker}>
                    <span className={`${styles.chip} font-mono`}>{w}</span>
                  </li>
                ))}
              </ul>
            </div>
          </div>
        </div>
      </div>
      <div className="mt-2 flex justify-end">
        <DemoPlayback paused={paused} reduce={reduce} onToggle={() => setPaused(!paused)} />
      </div>
    </div>
  )
}

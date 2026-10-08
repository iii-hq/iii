'use client'

import { useEffect, useState } from 'react'
import { IconArrowRight, IconPause, IconPlay } from '@/components/icons/iconly'
import { Button } from '@/components/ui/button'
import { useDemoPlayback } from '@/hooks/use-demo-playback'
import type { CodeLine } from '@/lib/highlight'
import { cn } from '@/lib/utils'
import { demo } from './content'
import styles from './live-demo.module.css'
import { DEMO_BEAT_MS, DEMO_DONE, DEMO_RESULT_HOLD_MS } from './live-demo-code'
import { ExecutionGraph } from './live-demo-graph'
import { ExecutionCode, ExecutionResult, ExecutionTrace } from './live-demo-panels'

export function LiveDemoClient({ code }: { code: CodeLine[] }) {
  const { ref, running, paused, setPaused, reduce } = useDemoPlayback<HTMLDivElement>()
  const [tick, setTick] = useState(0)
  const step = reduce ? DEMO_DONE : tick % (DEMO_DONE + 1)
  const run = Math.floor(tick / (DEMO_DONE + 1))
  const done = step === DEMO_DONE
  const PlaybackIcon = paused ? IconPlay : IconPause

  useEffect(() => {
    if (!running) return
    const timer = window.setInterval(() => setTick((current) => current + 1), done ? DEMO_RESULT_HOLD_MS : DEMO_BEAT_MS)
    return () => window.clearInterval(timer)
  }, [running, done])

  return (
    <div ref={ref} className="mt-9 sm:mt-12">
      <div className={styles.workspace} data-demo-step={step} data-demo-playing={running}>
        <div className={styles.toolbar}>
          <div className={styles.request}>
            <span className={styles.requestIcon}>
              <IconArrowRight className="size-4" />
            </span>
            <div>
              <p className={styles.label}>One request</p>
              <p className="mt-1.5 text-[13px] font-medium leading-relaxed sm:text-[14px]">{demo.prompt}</p>
            </div>
          </div>
          {reduce ? null : (
            <Button
              variant="ghost"
              size="icon"
              className={cn(styles.playback, 'size-9 shrink-0 rounded-full')}
              onClick={() => setPaused((current) => !current)}
              aria-label={paused ? 'Resume demo' : 'Pause demo'}
              title={paused ? 'Resume demo' : 'Pause demo'}
            >
              <PlaybackIcon key={paused ? 'play' : 'pause'} className="swap-in size-[15px]" />
            </Button>
          )}
        </div>
        <div className={styles.topGrid}>
          <ExecutionGraph step={step} running={running} />
          <ExecutionCode code={code} step={step} />
        </div>
        <ExecutionTrace key={run} step={step} running={running} />
        <ExecutionResult done={done} />
      </div>
      <div className="mt-4 flex flex-wrap items-center justify-between gap-2 text-[11px] text-muted-foreground">
        <p>Animated walkthrough with sample data. No setup required.</p>
        <p>
          {reduce
            ? 'Reduced motion · completed view'
            : paused
              ? 'Paused'
              : done
                ? 'Run complete · repeats automatically'
                : `Step ${step + 1} of ${DEMO_DONE}`}
        </p>
      </div>
    </div>
  )
}

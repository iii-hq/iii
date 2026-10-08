'use client'

import { useDemoPlayback } from '@/hooks/use-demo-playback'
import { cn } from '@/lib/utils'
import { DemoPlayback } from './demo-playback'
import styles from './problem-motion.module.css'

/** Controls playback without sending the static SVG artwork through the client bundle. */
export function ProblemMotion({ children }: { children: React.ReactNode }) {
  const { ref, reduce, paused, setPaused, running } = useDemoPlayback()

  return (
    <div ref={ref} data-running={running} className={cn(styles.stage, 'relative mt-12 md:mt-14')}>
      <div className="absolute -top-12 right-0">
        <DemoPlayback paused={paused} reduce={reduce} onToggle={() => setPaused(!paused)} />
      </div>
      {children}
    </div>
  )
}

import { IconActivity } from '@/components/icons/iconly'

import { cn } from '@/lib/utils'
import shared from './story.module.css'
import { OBSERVE_ELAPSED, STORY_TRACE, storyBeatDuration } from './story-model'
import styles from './story-observability.module.css'
import { OBSERVABILITY_TOTALS } from './story-observability-data'
import { TraceWaterfall } from './trace-waterfall'

const STATS = [
  { label: 'Function calls', value: OBSERVABILITY_TOTALS.calls.toLocaleString('en-US') },
  { label: 'Failed', value: OBSERVABILITY_TOTALS.failed.toLocaleString('en-US') },
  {
    label: 'Failure rate',
    value: `${((OBSERVABILITY_TOTALS.failed / OBSERVABILITY_TOTALS.calls) * 100).toFixed(1)}%`,
  },
] as const

/**
 * Observability: three plain numbers for the day (2026-10-07 sync, Anthony: "keep the stats as plain numbers, no
 * charts"), then one request traced across languages and machines. With the wider right column the trace gets the
 * full width, so the chapter fills without adding more things to look at.
 */
export function ObserveActivity({ beat, running }: { beat: number; running: boolean }) {
  const complete = beat >= 5
  return (
    <figure
      className={cn(shared.activity, styles.dashboard)}
      data-stage="observe"
      data-beat={beat}
      data-running={running}
    >
      <figcaption className={shared.activityHeader}>
        <span className={shared.activityIcon}>
          <IconActivity className="size-4" />
        </span>
        <span className={shared.activityLabel}>Observability</span>
        <span className={shared.activityStatus}>Sample iii workload · last 24h</span>
      </figcaption>
      <dl className={styles.stats} data-on={beat >= 1}>
        {STATS.map((stat, i) => (
          <div
            key={stat.label}
            className={styles.stat}
            style={{ transitionDelay: beat === 1 ? `${i * 60}ms` : undefined }}
          >
            <dt>{stat.label}</dt>
            <dd>{stat.value}</dd>
          </div>
        ))}
      </dl>
      <TraceWaterfall
        className={styles.trace}
        title="Request trace"
        meta={
          <span className={styles.requestStatus}>
            <code>POST /orders</code>
            <span aria-hidden>·</span>
            {complete ? '128 ms total' : beat >= 2 ? 'Tracing' : 'Waiting for a request'}
          </span>
        }
        ticks={['0', '32 ms', '64 ms', '96 ms', '128 ms']}
        total={128}
        spans={STORY_TRACE}
        elapsed={OBSERVE_ELAPSED[beat] ?? 128}
        active={complete ? undefined : STORY_TRACE.find((span) => span.at + 2 === beat)?.id}
        running={running}
        stepMs={storyBeatDuration('observe', beat)}
      >
        <p className={styles.traceSummary}>3 spans · 2 languages · local + cloud · one trace id</p>
      </TraceWaterfall>
      <div className={shared.activityFooter}>Exported over OTLP to your observability stack</div>
    </figure>
  )
}

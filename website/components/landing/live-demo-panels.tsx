'use client'

import { type CSSProperties, type RefObject, useLayoutEffect, useRef, useState } from 'react'
import { IconCheckCircle, IconChevronRight, IconGitHub, IconTypeScript } from '@/components/icons/iconly'
import type { CodeLine } from '@/lib/highlight'
import { cn } from '@/lib/utils'
import styles from './live-demo.module.css'
import {
  DEMO_BEAT_MS,
  DEMO_BEATS,
  DEMO_DONE,
  DEMO_DURATION,
  DEMO_RESULT,
  DEMO_WORKERS,
  demoCode,
  demoWorkerById,
} from './live-demo-code'
import { TraceWaterfall } from './trace-waterfall'

/**
 * One highlight block that slides from range to range, so the eye follows the execution down the file instead of
 * watching a highlight vanish in one place and reappear in another. Measured from the lines because long lines wrap
 * on narrow screens and no longer share one height.
 */
function useRangeBox(lines: RefObject<(HTMLSpanElement | null)[]>, from: number, to: number) {
  const [box, setBox] = useState<{ top: number; height: number } | null>(null)
  useLayoutEffect(() => {
    const first = lines.current[from]
    const last = lines.current[to]
    const pre = first?.parentElement?.parentElement
    if (!first || !last || !pre) return
    const measure = () => setBox({ top: first.offsetTop, height: last.offsetTop + last.offsetHeight - first.offsetTop })
    measure()
    const observer = new ResizeObserver(measure)
    observer.observe(pre)
    return () => observer.disconnect()
  }, [lines, from, to])
  return box
}

export function ExecutionCode({ code, step }: { code: CodeLine[]; step: number }) {
  const beat = DEMO_BEATS[step]
  const range = beat?.code ?? (beat ? demoWorkerById[beat.worker].code : [21, 21])
  const lines = useRef<(HTMLSpanElement | null)[]>([])
  const box = useRangeBox(lines, range[0], range[1])
  return (
    <div className={styles.codePanel}>
      <div className={styles.panelHeader}>
        <span className="flex items-center gap-2">
          <IconTypeScript className="size-4" />
          <span>{demoCode.file}</span>
        </span>
        <span className="text-[11px]">TypeScript</span>
      </div>
      <pre className={cn(styles.code, 'font-mono')}>
        {box ? (
          <span
            aria-hidden
            className={styles.codeHighlight}
            /* Laid out at a fixed 100px and sized with scaleY, so the slide between ranges is transform-only. */
            style={{ transform: `translateY(${box.top}px) scaleY(${(box.height / 100).toFixed(4)})` }}
          />
        ) : null}
        <code className="code-tokens">
          {code.map((tokens, index) => (
            <span
              // biome-ignore lint/suspicious/noArrayIndexKey: Server-tokenized lines never reorder.
              key={index}
              ref={(node) => {
                lines.current[index] = node
              }}
              className={styles.codeLine}
              data-active={index >= range[0] && index <= range[1]}
            >
              <span className={styles.lineNumber} aria-hidden>
                {index + 1}
              </span>
              <span className={styles.lineText}>
                {tokens.map((token, position) => (
                  // biome-ignore lint/suspicious/noArrayIndexKey: Tokens are static within each line.
                  <span key={position} style={token.style as CSSProperties}>
                    {token.text}
                  </span>
                ))}
                {'\n'}
              </span>
            </span>
          ))}
        </code>
      </pre>
      <div className={styles.codeFoot}>
        <span className="size-1.5 shrink-0 rounded-full bg-hero-accent" aria-hidden />
        {step === DEMO_DONE
          ? 'Assistant message returned to the caller.'
          : 'The highlighted code follows the execution.'}
      </div>
    </div>
  )
}

export function ExecutionTrace({ step, running }: { step: number; running: boolean }) {
  return (
    <TraceWaterfall
      className={styles.trace}
      title="Execution trace"
      meta={
        <>
          digest::run <span className="mx-1">/</span> illustrative timing
        </>
      }
      ticks={['0', '400ms', '800ms', '1.2s', '1.6s']}
      total={1600}
      spans={DEMO_WORKERS.map((worker) => ({ ...worker, name: worker.fn }))}
      elapsed={DEMO_BEATS[step]?.elapsed ?? DEMO_DURATION}
      active={DEMO_BEATS[step]?.worker}
      running={running}
      stepMs={DEMO_BEAT_MS}
    />
  )
}

export function ExecutionResult({ done }: { done: boolean }) {
  return (
    <div className={styles.result}>
      <div>
        <div key={done ? 'done' : 'pending'} className={cn(styles.resultHeading, 'swap-in')}>
          {done ? (
            <IconCheckCircle className="size-4 text-hero-accent" />
          ) : (
            <IconGitHub className="size-4 text-muted-foreground" />
          )}
          <h3 className="text-[13px] font-medium">{done ? 'Your PR digest' : 'Preparing the digest'}</h3>
        </div>
        <p className="mt-2 text-[11px] leading-relaxed text-muted-foreground">
          {done ? '3 open PRs, summarized.' : 'The result appears when the run finishes.'}
          <br />
          Sample data · iii-hq/workers
        </p>
      </div>
      <div className={styles.resultBody}>
        {/* Always laid out so the panel keeps its height; when the run finishes the three PRs rise in 50ms apart. */}
        <ol className={styles.resultList} style={{ visibility: done ? 'visible' : 'hidden' }}>
          {DEMO_RESULT.map((pr, index) => (
            <li
              key={pr.number}
              className={cn(styles.resultItem, done && 'swap-in')}
              style={done ? { animationDelay: `${index * 50}ms` } : undefined}
            >
              <span className="text-[11px] text-muted-foreground">#{pr.number}</span>
              <p className="mt-1 text-[12px] font-medium">{pr.title}</p>
              <p className="mt-1 text-[11px] leading-relaxed text-muted-foreground">{pr.description}</p>
            </li>
          ))}
        </ol>
        {done ? null : (
          <p className={cn(styles.resultPlaceholder, 'swap-in')}>
            <IconChevronRight className="size-3.5 shrink-0 text-hero-accent" />
            Fetch PRs, summarize, return the result.
          </p>
        )}
      </div>
    </div>
  )
}

import { cn } from '@lib/lib/utils'
import { useState } from 'react'
import {
  ALL_CLASSIFICATIONS,
  ORACLE_LABELS,
  SABOTAGE_MODES,
  type Classification,
} from '../content/sabotage'

/** break-the-run — pick a sabotage, watch the oracles catch it (deck-local). */

const TONE_TEXT = {
  accent: 'text-accent',
  alert: 'text-alert',
  warn: 'text-warn',
} as const

const LOG_GLYPH = {
  ok: { glyph: '✓', className: 'text-ink-faint' },
  bad: { glyph: '✗', className: 'text-alert' },
  warn: { glyph: '!', className: 'text-warn' },
  dim: { glyph: '·', className: 'text-ink-ghost' },
} as const

export function BreakTheRun() {
  const [modeId, setModeId] = useState('none')
  const [visited, setVisited] = useState<Classification[]>(['pass'])

  const mode = SABOTAGE_MODES.find((m) => m.id === modeId) ?? SABOTAGE_MODES[0]

  const select = (id: string) => {
    setModeId(id)
    const next = SABOTAGE_MODES.find((m) => m.id === id)
    if (next && !visited.includes(next.classification)) {
      setVisited([...visited, next.classification])
    }
  }

  return (
    <div
      role="group"
      aria-label="break the run: pick a sabotage and see which oracle catches it"
      className="border border-rule bg-bg"
    >
      {/* sabotage picker */}
      <div className="flex flex-wrap items-center gap-2 bg-panel px-3.5 py-2.5 border-b border-rule">
        <span className="font-mono text-[11px] uppercase tracking-[0.06em] text-ink-faint mr-1">
          sabotage:
        </span>
        {SABOTAGE_MODES.map((m) => (
          <button
            key={m.id}
            type="button"
            aria-pressed={m.id === mode.id}
            onClick={() => select(m.id)}
            className={cn(
              'border px-2.5 py-1 font-mono text-[12px] lowercase transition-colors',
              m.id === mode.id
                ? 'border-accent text-accent'
                : 'border-rule text-ink-faint hover:border-ink hover:text-ink',
            )}
          >
            {m.chip}
          </button>
        ))}
      </div>

      <div className="px-4 py-2.5 border-b border-rule-2 font-mono text-[12px] lowercase text-ink-faint">
        <span className="text-ink-ghost">you:</span> <span className="text-ink">{mode.deed}</span>
      </div>

      {/* run log + oracle verdicts */}
      <div className="grid grid-cols-1 @4xl:grid-cols-[minmax(0,1fr)_minmax(0,320px)] gap-px bg-rule">
        <div className="bg-bg px-4 py-3.5 min-w-0">
          <div className="font-mono text-[10px] uppercase tracking-[0.14em] text-ink-ghost mb-2.5">
            the run
          </div>
          <div key={mode.id} className="flex flex-col gap-y-1.5">
            {mode.log.map((line, i) => (
              <div
                key={`${mode.id}-${i}`}
                className="fade-rise flex items-baseline gap-x-2.5"
                style={{ animationDelay: `${i * 70}ms` }}
              >
                <span className={cn('font-mono text-[12px] shrink-0', LOG_GLYPH[line.tone].className)}>
                  {LOG_GLYPH[line.tone].glyph}
                </span>
                <span className="font-mono text-[12.5px] leading-[1.6] text-ink-faint lowercase">
                  {line.text}
                </span>
              </div>
            ))}
          </div>
        </div>

        <div className="bg-bg px-4 py-3.5 min-w-0">
          <div className="font-mono text-[10px] uppercase tracking-[0.14em] text-ink-ghost mb-2.5">
            oracle verdicts
          </div>
          <div className="flex flex-col">
            {ORACLE_LABELS.map((o) => {
              const call = mode.oracles[o.id]
              const glyph =
                call.v === 'fail'
                  ? { g: '✗', c: 'text-alert' }
                  : call.v === 'skip'
                    ? { g: '–', c: 'text-ink-ghost' }
                    : { g: '✓', c: 'text-ink-faint' }
              const note = call.note ?? (call.v === 'skip' ? 'not reached' : undefined)
              return (
                <div
                  key={o.id}
                  className="flex items-baseline gap-x-2.5 py-1 border-b border-rule-2 last:border-b-0"
                >
                  <span className={cn('font-mono text-[12px] shrink-0', glyph.c)}>{glyph.g}</span>
                  <span
                    className={cn(
                      'font-mono text-[12px] lowercase',
                      call.v === 'fail' ? 'text-ink font-semibold' : 'text-ink-faint',
                    )}
                  >
                    {o.label}
                  </span>
                  {note ? (
                    <span className="font-mono text-[10.5px] text-ink-ghost lowercase min-w-0">
                      {note}
                    </span>
                  ) : null}
                </div>
              )
            })}
          </div>
          <p className="mt-2.5 font-mono text-[10.5px] leading-[1.6] text-ink-ghost lowercase">
            traces and logs stay diagnostic; they never decide a verdict.
          </p>
        </div>
      </div>

      {/* verdict strip */}
      <div className="border-t border-rule px-4 py-3.5">
        <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1">
          <span className={cn('font-mono text-[14px] font-semibold', TONE_TEXT[mode.tone])}>
            {mode.classification}
          </span>
          <span className="font-mono text-[11.5px] text-ink-ghost tabular-nums">exit {mode.exit}</span>
          <span className="font-mono text-[12px] text-ink-faint lowercase">
            caught by: <span className="text-ink">{mode.caughtBy}</span>
          </span>
        </div>
        <p className="mt-1.5 font-mono text-[12px] leading-[1.65] text-ink-faint lowercase max-w-[86ch]">
          {mode.lesson}
        </p>
      </div>

      {/* collection tracker */}
      <div className="border-t border-rule bg-panel px-4 py-2.5 flex flex-wrap items-center gap-x-3 gap-y-1.5">
        <span className="font-mono text-[10px] uppercase tracking-[0.06em] text-ink-faint">
          classifications triggered · {visited.length}/{ALL_CLASSIFICATIONS.length}
        </span>
        <div className="flex flex-wrap gap-1.5">
          {ALL_CLASSIFICATIONS.map((c) => (
            <span
              key={c}
              className={cn(
                'font-mono text-[10.5px] lowercase border px-1.5 py-0.5',
                visited.includes(c)
                  ? 'border-ink text-ink'
                  : 'border-rule-2 text-ink-ghost',
              )}
            >
              {c}
            </span>
          ))}
        </div>
        {visited.length < ALL_CLASSIFICATIONS.length ? (
          <span className="font-mono text-[10.5px] text-ink-ghost lowercase">can you cause all six?</span>
        ) : (
          <span className="font-mono text-[10.5px] text-accent lowercase">all six. the taxonomy is complete.</span>
        )}
      </div>
    </div>
  )
}

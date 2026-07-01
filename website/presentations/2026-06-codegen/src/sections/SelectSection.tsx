import { Section } from '@lib/components/Section'
import { FnChip } from '@lib/components/schematic/FnChip'
import { ModeToggle } from '@lib/components/schematic/ModeToggle'
import { useState } from 'react'
import { CATALOG, inScope, SCOPES } from '../content/select'

/**
 * A9 - toggle explorer. The workers / functions / triggers selectors are globs
 * over the live catalog. Switch the scope and watch the selected set grow from
 * one worker to the whole registry.
 */
export function SelectSection() {
  const [scope, setScope] = useState<string>('harness')
  const active = SCOPES.find((s) => s.value === scope) ?? SCOPES[0]
  const selected = CATALOG.filter((fn) => inScope(fn.id, active.glob))

  return (
    <Section
      id="select"
      index="05"
      eyebrow="select"
      title="one glob, one worker, or the whole registry."
      lede="point an output at a single worker, a namespace, or * for everything. the same selectors that scope harness scope the entire catalog; only the glob changes."
    >
      <div className="flex flex-col gap-5 @3xl:flex-row @3xl:items-center @3xl:justify-between">
        <ModeToggle
          value={scope}
          onChange={setScope}
          options={SCOPES.map((s) => ({ value: s.value, label: s.label }))}
        />
        <div className="flex items-center gap-x-3 font-mono text-[12px] lowercase text-ink-faint">
          <span className="text-ink-ghost uppercase tracking-[0.1em] text-[10px]">functions:</span>
          <FnChip tone="accent">{active.glob}</FnChip>
          <span className="tabular-nums">
            <span className="text-ink">{selected.length}</span> of {CATALOG.length} selected
          </span>
        </div>
      </div>

      <div className="mt-6 grid grid-cols-1 @lg:grid-cols-2 @4xl:grid-cols-3 gap-px bg-rule border border-rule">
        {CATALOG.map((fn) => {
          const on = inScope(fn.id, active.glob)
          return (
            <div key={fn.id} className="bg-bg px-4 py-3 flex items-center justify-between gap-x-3 min-w-0">
              <FnChip tone={on ? 'ink' : 'ghost'} className={on ? '' : 'line-through opacity-50'}>
                {fn.id}
              </FnChip>
              <span
                className={`font-mono text-[10px] uppercase tracking-[0.06em] shrink-0 ${
                  on ? 'text-accent' : 'text-ink-ghost opacity-50'
                }`}
              >
                {on ? 'gen' : 'skip'}
              </span>
            </div>
          )
        })}
      </div>

      <p className="mt-5 font-mono text-[12px] leading-[1.6] text-ink-faint lowercase max-w-[72ch]">
        selection is the union of the workers, functions, and triggers lists. an empty list contributes nothing; only{' '}
        <span className="text-ink">*</span> means all. engine-internal handlers stay hidden unless a glob names them.
      </p>
    </Section>
  )
}

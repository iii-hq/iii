import { Section } from '@lib/components/Section'
import { CodeBlock } from '@lib/components/schematic/CodeBlock'
import { FnChip } from '@lib/components/schematic/FnChip'
import { ModeToggle } from '@lib/components/schematic/ModeToggle'
import { useState } from 'react'
import { SLOT_OPTIONS, SLOTS, type SlotId } from '../content/slots'

/**
 * A9 — the four registrars behind one setup(host) contract. Toggle a slot to
 * read its mount point, exact props, and the walls the host keeps.
 */
export function SlotsSection() {
  const [slot, setSlot] = useState<SlotId>('composer')
  const spec = SLOTS[slot]

  return (
    <Section
      id="slots"
      index="05"
      eyebrow="the slots"
      title="one setup(host), four registrars."
      lede="registration goes through the per-script host only — every call returns an unregister fn and is auto-tracked, so the loader disposes exactly what the script registered on reload, delete, and worker disconnect."
    >
      <ModeToggle value={slot} onChange={setSlot} options={SLOT_OPTIONS} className="mb-5 flex-wrap" />

      <div className="grid grid-cols-1 @4xl:grid-cols-2 gap-4 items-start">
        <div className="flex flex-col gap-4 min-w-0">
          <div className="border border-rule bg-bg p-5">
            <div className="font-mono text-[15px] font-semibold text-ink lowercase mb-2">{spec.title}</div>
            <p className="font-mono text-[13px] leading-[1.7] text-ink-faint lowercase">{spec.mount}</p>
          </div>
          <div className="border border-rule bg-bg p-5">
            <div className="font-mono text-[11px] uppercase tracking-[0.14em] text-ink-faint mb-3">
              host-owned, not touchable
            </div>
            <ul className="flex flex-col gap-y-2">
              {spec.walls.map((wall) => (
                <li key={wall} className="flex items-baseline gap-x-2.5">
                  <span aria-hidden className="font-mono text-[12px] text-alert shrink-0">
                    ×
                  </span>
                  <span className="font-mono text-[12.5px] leading-[1.6] text-ink-faint lowercase">{wall}</span>
                </li>
              ))}
            </ul>
          </div>
        </div>

        <CodeBlock title={`${SLOT_OPTIONS.find((o) => o.value === slot)?.label} — the exact props`}>
          {spec.code}
        </CodeBlock>
      </div>

      <div className="mt-6 flex flex-wrap items-center gap-2">
        <span className="font-mono text-[11px] uppercase tracking-[0.06em] text-ink-ghost mr-1">
          everything a script touches:
        </span>
        <FnChip>host.iii.trigger()</FnChip>
        <FnChip>host.iii.on()</FnChip>
        <FnChip>host.components</FnChip>
        <FnChip>host.useTheme()</FnChip>
        <FnChip tone="accent">setup(host)</FnChip>
      </div>
      <p className="mt-3 font-mono text-[12px] text-ink-faint lowercase">
        full worker-side walkthrough →{' '}
        <a href="#/authoring" className="text-ink underline decoration-rule underline-offset-4 hover:text-accent">
          the author&apos;s chair
        </a>
      </p>
    </Section>
  )
}

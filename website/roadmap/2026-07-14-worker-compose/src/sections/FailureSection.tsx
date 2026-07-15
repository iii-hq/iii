import { FanOut } from '@lib/components/diagrams/FanOut'
import { Section } from '@lib/components/Section'
import { StatusPanel } from '@lib/components/schematic/StatusPanel'
import { FAILURE_HANDLERS, FAILURE_NOTES, FAILURE_SOURCE, FAILURE_TRIGGER } from '../content/failure'

/**
 * A7 — a post-ready crash fans out to an ordered local stop. The deep story
 * (daemon crash + restart reconciliation) lives on the crash-restart page.
 */
export function FailureSection() {
  return (
    <Section
      id="failure"
      index="09"
      eyebrow="failure"
      title="a crash stops its dependents, not your data."
      lede="when a dependency dies post-ready, its local dependents stop in reverse order with the cause in the logs — instead of pushing work into a corpse."
    >
      <FanOut
        title="one crash, an ordered stop"
        source={FAILURE_SOURCE}
        trigger={FAILURE_TRIGGER}
        handlers={FAILURE_HANDLERS}
      />

      <div className="mt-6 grid grid-cols-1 @4xl:grid-cols-[minmax(0,1fr)_340px] gap-4 items-start">
        <StatusPanel
          variant="alert"
          headline="why cascade at all"
          detail={FAILURE_NOTES[0]}
        />
        <div className="border border-rule bg-bg px-4 py-3.5">
          <div className="font-mono text-[11px] uppercase tracking-[0.14em] text-ink-faint mb-2">scope</div>
          {FAILURE_NOTES.slice(1).map((note, i) => (
            <p key={i} className="font-mono text-[12px] leading-[1.7] text-ink-faint lowercase mt-1.5 first:mt-0">
              {note}
            </p>
          ))}
          <a
            href="#/crash-restart"
            className="mt-3 inline-block font-mono text-[12px] lowercase text-ink hover:text-accent transition-colors"
          >
            deep dive: the daemon itself crashes →
          </a>
        </div>
      </div>
    </Section>
  )
}

import { Section } from '@lib/components/Section'
import { StatusPanel } from '@lib/components/schematic/StatusPanel'
import { WHY_CARDS, WHY_NOTE } from '../content/why'

/**
 * A2 — the problem. The four compile-time extension paths, each with its
 * file:line citation, plus the "not implemented" registry sketch.
 */
export function WhySection() {
  return (
    <Section
      id="why"
      index="01"
      eyebrow="the problem"
      title="every extension path is compile-time."
      lede="four ways ui gets into the console today; all of them end in &quot;edit console source, rebuild, release&quot;."
    >
      <div className="grid grid-cols-1 @2xl:grid-cols-2 gap-px bg-rule border border-rule">
        {WHY_CARDS.map((card) => (
          <div key={card.title} className="bg-bg p-6 min-w-0">
            <div className="font-mono text-[15px] font-semibold text-alert lowercase mb-2.5">{card.title}</div>
            <p className="font-mono text-[13px] leading-[1.7] text-ink-faint lowercase">{card.body}</p>
          </div>
        ))}
      </div>

      <StatusPanel
        className="mt-6"
        variant="warn"
        headline={WHY_NOTE.headline}
        detail={WHY_NOTE.detail}
      />
    </Section>
  )
}

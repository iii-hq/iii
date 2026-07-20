import { Section } from '@lib/components/Section'
import { StatusPanel } from '@lib/components/schematic/StatusPanel'
import { LIMITS } from '../content/limits'

/**
 * The honesty beat — v1 boundaries stated as the spec states them, not
 * softened. Admitted limits are what make the rest of the claims credible.
 */
export function LimitsSection() {
  return (
    <Section
      id="limits"
      index="09"
      eyebrow="non-goals"
      title="what v1 does not do."
      lede="deliberate boundaries from the spec's non-goals — each one is a scoping decision with a named v2 path, not an oversight."
    >
      <div className="grid grid-cols-1 @3xl:grid-cols-2 gap-3">
        {LIMITS.map((limit) => (
          <StatusPanel
            key={limit.headline}
            variant={limit.variant}
            headline={limit.headline}
            detail={limit.detail}
          />
        ))}
      </div>
    </Section>
  )
}

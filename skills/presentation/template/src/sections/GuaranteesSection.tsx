import { Funnel } from '@/components/diagrams/Funnel'
import { Section } from '@/components/Section'
import { FUNNEL_PATHS, FUNNEL_REJECT, FUNNEL_TARGET } from '@/content/example'

/**
 * A8 — a convergence funnel. Many fragile ways to do a thing collapse into one
 * guaranteed mechanism; an optional dashed path shows what was removed. Use for
 * "many cases → one correct path" arguments.
 */
export function GuaranteesSection() {
  return (
    <Section
      id="guarantees"
      index="05"
      eyebrow="correct by construction"
      title="many ways to lose work — collapsed into one that can't."
      lede="every fragile path funnels into a single durable mechanism. the failure mode isn't handled; it's designed out."
    >
      <Funnel
        title="many fragile paths → one durable entry"
        paths={FUNNEL_PATHS}
        target={FUNNEL_TARGET}
        reject={FUNNEL_REJECT}
      />
    </Section>
  )
}

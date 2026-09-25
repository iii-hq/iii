import { PayoffScorecard, PayoffTable } from "@lib/components/Payoff"
import { Section } from "@lib/components/Section"
import { PAYOFF_METRICS, PAYOFF_SOLVES } from "../content/payoff"

/**
 * A11 — the payoff. Closes the persuasion loop: a before/after scorecard, then
 * a problem → answer table. The before column is the engine-native world; the
 * after is the proxy.
 */
export function PayoffSection() {
  return (
    <Section
      id="payoff"
      index="08"
      eyebrow="the payoff"
      title="smaller blast radius, complete discovery filtering, zero engine changes."
      lede="the same rbac contract, moved to a better home. measured against an engine-native listener, with the proxy's numbers from the spec."
    >
      <PayoffScorecard metrics={PAYOFF_METRICS} valueClassName="text-[24px]" wrap={false} />
      <PayoffTable rows={PAYOFF_SOLVES} problemHeading="the problem" answerHeading="the answer" />
    </Section>
  )
}

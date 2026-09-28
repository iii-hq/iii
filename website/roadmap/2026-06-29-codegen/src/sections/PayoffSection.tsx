import { PayoffScorecard, PayoffTable } from "@lib/components/Payoff"
import { Section } from "@lib/components/Section"
import { PAYOFF_METRICS, PAYOFF_SOLVES } from "../content/payoff"

/**
 * A11 - the payoff. Closes the loop: a before/after scorecard, then a problem →
 * answer table. The reader should leave convinced the discipline calling a
 * worker used to need is now gone by construction.
 */
export function PayoffSection() {
  return (
    <Section
      id="payoff"
      index="08"
      eyebrow="the payoff"
      title="runtime typos become compile errors."
      lede="the same integration, measured before and after. the failure modes that used to need discipline are now caught by the compiler, or by ci."
    >
      <PayoffScorecard metrics={PAYOFF_METRICS} />
      <PayoffTable rows={PAYOFF_SOLVES} problemHeading="the problem" answerHeading="the answer" />
    </Section>
  )
}

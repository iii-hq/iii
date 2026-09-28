import { PayoffScorecard, PayoffTable } from "@lib/components/Payoff"
import { Section } from "@lib/components/Section"
import { PAYOFF_METRICS, PAYOFF_SOLVES } from "../content/payoff"

/**
 * A11 — the payoff. Before/after scorecard measured against the console as it
 * exists today, then the four problems from section 01 answered one by one.
 */
export function PayoffSection() {
  return (
    <Section
      id="payoff"
      index="10"
      eyebrow="the payoff"
      title="ship ui with the worker."
      lede="measured against the console as it exists today; numbers from the spec. every row on the left is a citation from section 01."
    >
      <PayoffScorecard metrics={PAYOFF_METRICS} />
      <PayoffTable rows={PAYOFF_SOLVES} problemHeading="today" answerHeading="with injectable ui" answerClassName="" />
    </Section>
  )
}

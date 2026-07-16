import { SequencePlayer } from '@lib/components/diagrams/SequencePlayer'
import { Section } from '@lib/components/Section'
import { SpecRow, SpecSheet } from '@lib/components/SpecSheet'
import { QUAL_LANES, QUAL_STEPS } from '../content/flows'
import { TRUST_RULES } from '../content/protocols'

const BUDGETS = [
  { name: 'max_cycles · attempt_timeout_ms', type: 'hard', desc: 'the evaluator refuses work beyond them and calls harness::stop at the deadline.' },
  { name: 'max_total_tokens · max_cost_usd', type: 'soft ceiling', desc: 'visible only after a model turn; may overshoot by one bounded turn, recorded, never mislabeled as hard.' },
  { name: 'max_browser_actions · max_network_requests', type: 'soft ceiling', desc: 'checked before the next unit; a fixture can pin network activity to zero.' },
  { name: 'attempts', type: '1–10 per leg', desc: 'comparison legs run in a persisted, seed-derived interleaved schedule, reproducible pair by pair.' },
] as const

/**
 * A5 — one agent-quality attempt: start, send, validate, feed back or publish.
 * The claim: real model, bounded feedback, independent judgment.
 */
export function QualityLoopSection() {
  return (
    <Section
      id="quality"
      index="08"
      eyebrow="agent quality · a run"
      title="a real model, a bounded loop, and validators it cannot see."
      lede="the harness-eval worker persists before every step, survives duplicate and missing completion events, and lets exactly one thing extend a run: an open validator with feedback and budget remaining."
    >
      <SequencePlayer title="one attempt, manifest to report" lanes={QUAL_LANES} steps={QUAL_STEPS} />

      <div className="mt-6 grid grid-cols-1 @4xl:grid-cols-2 gap-4">
        <SpecSheet title="budgets, honestly labeled" meta="hard vs soft">
          <div className="flex flex-col">
            {BUDGETS.map((row) => (
              <SpecRow key={row.name} name={row.name} type={row.type}>
                {row.desc}
              </SpecRow>
            ))}
          </div>
        </SpecSheet>

        <SpecSheet title="trust boundaries" meta="the subject stays a subject">
          <div className="flex flex-col">
            {TRUST_RULES.map((row) => (
              <SpecRow key={row.name} name={row.name}>
                {row.desc}
              </SpecRow>
            ))}
          </div>
        </SpecSheet>
      </div>

      <div className="mt-6">
        <a
          href="#/agent-quality-protocol"
          className="inline-flex h-10 items-center bg-bg text-ink border border-ink px-4 font-mono text-[13px] lowercase transition-colors hover:bg-ink hover:text-bg"
        >
          the manifest, error codes, recovery table, and corpus →
        </a>
      </div>
    </Section>
  )
}

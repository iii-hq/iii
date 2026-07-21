import { SequencePlayer } from '@lib/components/diagrams/SequencePlayer'
import { Section } from '@lib/components/Section'
import { SpecRow, SpecSheet } from '@lib/components/SpecSheet'
import { QUAL_LANES, QUAL_STEPS } from '../content/flows'
import { TRUST_RULES } from '../content/protocols'

const BUDGETS = [
  { name: 'vitest timeout + session registry', type: 'hard', desc: 'a test deadline bounds every case; afterEach stops every tracked non-terminal session with harness::stop, and a cleanup miss is itself a typed failure class.' },
  { name: 'tokens · cost', type: 'asserted ceiling', desc: 'expect() over harness::metrics totals — post-turn checks that may overshoot by one bounded turn, never mislabeled as hard preemption.' },
  { name: 'feedback loops', type: 'bounded in code', desc: 'an ordinary loop with its maximum iteration count declared in the test file; each retry is a public send in the same session.' },
  { name: 'no test-layer retry', type: 'policy', desc: 'subject calls are never retried by the test layer; timeout or transport retry lives only in the configured subject runtime, only for idempotent operations, recorded in stack.json.' },
] as const

/**
 * A5 — one e2e test: send, await, read evidence, assert.
 * The claim: real model, plain tests, evidence it cannot fake.
 */
export function E2ESection() {
  return (
    <Section
      id="e2e"
      index="09"
      eyebrow="e2e tests · a test"
      title="a real model, plain tests, and evidence it cannot fake."
      lede="an ordinary vitest file drives the production path through worker.trigger, waits for durable terminal state, and grades outcomes with explicit assertions over versioned evidence the harness serves by default. helpers coordinate only what a single public call cannot."
    >
      <SequencePlayer title="one test, send to verdict" lanes={QUAL_LANES} steps={QUAL_STEPS} width={1030} />

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

        <SpecSheet title="trust boundaries" meta="no self-grading, no model judges">
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
          href="#/e2e-protocol"
          className="inline-flex h-10 items-center bg-bg text-ink border border-ink px-4 font-mono text-[13px] lowercase transition-colors hover:bg-ink hover:text-bg"
        >
          the test file, helpers, evidence contracts, and corpus →
        </a>
      </div>
    </Section>
  )
}

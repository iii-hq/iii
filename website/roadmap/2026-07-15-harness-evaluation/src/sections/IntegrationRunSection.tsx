import { SequencePlayer } from '@lib/components/diagrams/SequencePlayer'
import { Section } from '@lib/components/Section'
import { SpecRow, SpecSheet } from '@lib/components/SpecSheet'
import { CONF_LANES, CONF_STEPS } from '../content/flows'

const PROFILE_REAL = [
  { name: 'engine + worker manager', type: 'pinned artifact', desc: 'function registration, triggers, channels, worker lifecycle; digests recorded before boot.' },
  { name: 'queue · iii-state · session-manager', type: 'real, isolated dirs', desc: 'the durable turn loop runs on the same machinery production uses.' },
  { name: 'context-manager', type: 'real and required', desc: 'context assembly and token preflight fail closed without it. it is not optional and has no harness-side fallback.' },
  { name: 'cron · stream · observability · directory', type: 'real', desc: 'packaged dependencies; traces stay diagnostic, never a readiness requirement.' },
] as const

const PROFILE_REPLACED = [
  { name: 'scripted router', type: 'controlled', desc: 'owns the six fixed router::* ids; the real llm-router is absent so ownership cannot be boot-order dependent.' },
  { name: 'recorder', type: 'controlled', desc: 'run-scoped target calls, lifecycle events, barriers: the test-owned evidence.' },
  { name: 'provider-anthropic / provider-openai', type: 'absent', desc: 'no production network or model behavior in this track.' },
  { name: 'shell · web · browser', type: 'absent in the first slice', desc: 'added only when a later stack profile exercises them, never silently skipped.' },
] as const

/**
 * A5 — one integration scenario, end to end. The claim: the stack is real and
 * the entry is public; only the model boundary is scripted.
 */
export function IntegrationRunSection() {
  return (
    <Section
      id="run"
      index="04"
      eyebrow="integration · a run"
      title="everything is real except the model."
      lede="press play, or step it yourself. one fresh isolated stack per scenario: the real engine, queue, and durable turn loop, with a strict script where the model would be."
    >
      <SequencePlayer title="one scenario, allocate to teardown" lanes={CONF_LANES} steps={CONF_STEPS} />

      <div className="mt-6 grid grid-cols-1 @4xl:grid-cols-2 gap-4">
        <SpecSheet title="what runs for real" meta="system profile">
          <div className="flex flex-col">
            {PROFILE_REAL.map((row) => (
              <SpecRow key={row.name} name={row.name} type={row.type}>
                {row.desc}
              </SpecRow>
            ))}
          </div>
        </SpecSheet>

        <SpecSheet title="what is controlled or absent" meta="the one replaced boundary">
          <div className="flex flex-col">
            {PROFILE_REPLACED.map((row) => (
              <SpecRow key={row.name} name={row.name} type={row.type}>
                {row.desc}
              </SpecRow>
            ))}
          </div>
        </SpecSheet>
      </div>
    </Section>
  )
}

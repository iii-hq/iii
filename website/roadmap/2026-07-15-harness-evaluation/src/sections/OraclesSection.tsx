import { Section } from '@lib/components/Section'
import { StatusPanel } from '@lib/components/schematic/StatusPanel'
import { ORACLES } from '../content/oracles'

/**
 * A2-shaped grid, positive form — the eight evidence sources a scenario is
 * graded against. The claim: no single oracle is sufficient.
 */
export function OraclesSection() {
  return (
    <Section
      id="oracles"
      index="07"
      eyebrow="integration · evidence"
      title="no single oracle is sufficient."
      lede="a scenario passes only when independent evidence sources agree. the lifecycle event is not trusted alone; the transcript is not trusted alone; the subject is never asked to grade itself."
    >
      <div className="grid grid-cols-1 @2xl:grid-cols-2 @4xl:grid-cols-4 gap-px bg-rule border border-rule">
        {ORACLES.map((o, i) => (
          <div key={o.name} className="bg-bg p-5 min-w-0">
            <div className="flex items-baseline gap-x-2.5 mb-2">
              <span className="font-mono text-[11px] text-ink-ghost tabular-nums">0{i + 1}</span>
              <span className={`font-mono text-[13.5px] font-semibold lowercase ${'diagnostic' in o && o.diagnostic ? 'text-ink-faint' : 'text-ink'}`}>
                {o.name}
              </span>
            </div>
            <p className="font-mono text-[12px] leading-[1.65] text-ink-faint lowercase">{o.proves}</p>
          </div>
        ))}
      </div>

      <div className="mt-6">
        <StatusPanel
          variant="warn"
          icon={<span>!</span>}
          headline="private state never decides a public-contract scenario"
          detail="it may be copied after a failure for diagnosis, but an ordinary scenario is graded on public, durable evidence only: the same surfaces a production caller can reach."
        />
      </div>
    </Section>
  )
}

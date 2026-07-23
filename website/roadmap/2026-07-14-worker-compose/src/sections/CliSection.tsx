import { Section } from '@lib/components/Section'
import { C, CodeBlock, K, M, S } from '@lib/components/schematic/CodeBlock'
import { Cell } from '@lib/components/schematic/Cell'
import { FnChip } from '@lib/components/schematic/FnChip'
import { CLI_NOTES, CLI_STANDARD, CLI_TODAY } from '../content/cli'

/**
 * Before/after — 41 binaries and three conventions on the left, one contract
 * on the right. The daemon can only inject what workers agree to read.
 */
export function CliSection() {
  return (
    <Section
      id="cli"
      index="07"
      eyebrow="the contract"
      title="41 binaries. one way in."
      lede="a supervisor can only inject what workers agree to read — and today they agree on nothing. the standard is three parameters, each a flag with an env fallback."
    >
      <div className="grid grid-cols-1 @4xl:grid-cols-2 gap-4">
        <div className="min-w-0">
          <div className="font-mono text-[11px] uppercase tracking-[0.14em] text-ink-faint mb-3">today</div>
          <div className="border border-rule bg-bg flex flex-col">
            {CLI_TODAY.map((row) => (
              <div key={row.worker} className="px-4 py-3 border-b border-rule-2 last:border-b-0">
                <div className="font-mono text-[13px] text-ink lowercase">{row.worker}</div>
                <div className="mt-1 font-mono text-[11px] text-ink-faint lowercase">
                  reads {row.reads} · env {row.env} · default {row.default}
                </div>
              </div>
            ))}
          </div>
        </div>

        <div className="min-w-0">
          <div className="font-mono text-[11px] uppercase tracking-[0.14em] text-ink-faint mb-3">the standard</div>
          <div className="border border-rule bg-bg flex flex-col">
            {CLI_STANDARD.map((row) => (
              <div key={row.param} className="flex items-baseline gap-x-3 px-4 py-3 border-b border-rule-2 last:border-b-0">
                <span className="font-mono text-[13px] text-ink lowercase flex-1 min-w-0">{row.param}</span>
                <FnChip>{row.flag}</FnChip>
                <FnChip tone="faint">{row.env}</FnChip>
              </div>
            ))}
          </div>
          <div className="mt-4">
            <CodeBlock title="worker code, complete">
              <div>
                <K>import</K> {'{ registerWorker }'} <K>from</K> <S>'iii'</S>
              </div>
              <div>&nbsp;</div>
              <div>
                <C>{'// no url. no namespace. no config path.'}</C>
              </div>
              <div>
                <C>{'// the env contract fills all three — under compose, iii worker, or by hand.'}</C>
              </div>
              <div>
                <K>const</K> iii = <M>registerWorker</M>()
              </div>
            </CodeBlock>
          </div>
        </div>
      </div>

      <div className="mt-4 grid grid-cols-1 @2xl:grid-cols-2 gap-px bg-rule border border-rule">
        {CLI_NOTES.map((note, i) => (
          <Cell key={i} className="border-0" bodyClassName="max-w-none">
            {note}
          </Cell>
        ))}
      </div>
    </Section>
  )
}

import { Section } from '@lib/components/Section'
import { SpecSheet } from '@lib/components/SpecSheet'
import { FnChip } from '@lib/components/schematic/FnChip'
import { C001_MATCH, C002_INVARIANTS, FRAME_WALK, MATCHER_MODES } from '../content/contract'

/**
 * A13 — the scripted-router contract, shown through the first committed
 * scenario. The claim: every field is matched explicitly; there is no runner
 * default to hide behind.
 */
export function ContractSection() {
  return (
    <Section
      id="contract"
      index="06"
      eyebrow="integration tests · the script"
      title="every router field is matched explicitly. there is no runner default."
      lede="the first scenario, C-E2E-001, streams the phrase “fixture complete” through eight frozen frames. authors write one concise rust builder module; the compiler expands it into an explicit matcher for all twelve router::chat fields, including five explicit absences."
    >
      <div className="grid grid-cols-1 @4xl:grid-cols-[minmax(0,1fr)_minmax(0,1fr)] gap-4">
        <div className="border border-rule bg-bg">
          <div className="bg-panel px-3.5 py-2 border-b border-rule font-mono text-[11px] font-medium uppercase tracking-[0.06em] text-ink-faint">
            C-E2E-001 · the eight frames, in order
          </div>
          <div className="flex flex-col">
            {FRAME_WALK.map((f, i) => (
              <div
                key={`${f.frame}-${i}`}
                className="grid grid-cols-[24px_110px_minmax(0,1fr)] gap-x-3 items-baseline px-4 py-2 border-b border-rule-2 last:border-b-0"
              >
                <span className="font-mono text-[11px] text-ink-ghost tabular-nums">{i + 1}</span>
                <span className={`font-mono text-[12.5px] ${f.frame === 'done' ? 'text-accent' : 'text-ink'}`}>
                  {f.frame}
                </span>
                <span className="font-mono text-[12px] leading-[1.6] text-ink-faint min-w-0">
                  <span className="text-ink-faint">{f.payload}</span>
                  <span className="text-ink-ghost lowercase"> · {f.note}</span>
                </span>
              </div>
            ))}
          </div>
          <div className="px-4 py-3 border-t border-rule font-mono text-[11.5px] leading-[1.6] text-ink-faint lowercase">
            fifteen variants are frozen; only <span className="text-ink">done</span> and{' '}
            <span className="text-ink">error</span> are terminal, and streamed usage, the terminal{' '}
            <span className="text-ink">done</span>, and the response must agree exactly. the invariants then check
            the durable side: one user message, one assistant message reading exactly{' '}
            <span className="text-ink">fixture complete</span>, no partial-created duplicates, one generation
            consumed.
          </div>
        </div>

        <div className="flex flex-col gap-4 min-w-0">
          <div className="border border-rule bg-bg">
            <div className="bg-panel px-3.5 py-2 border-b border-rule font-mono text-[11px] font-medium uppercase tracking-[0.06em] text-ink-faint">
              six matcher modes, twelve matched fields
            </div>
            <div className="flex flex-wrap gap-2 px-4 pt-3.5">
              {MATCHER_MODES.map((m) => (
                <span key={m.mode} className="inline-flex items-baseline gap-x-1.5 border border-rule px-2 py-1">
                  <span className="font-mono text-[12px] text-ink">{m.mode}</span>
                  <span className="font-mono text-[10.5px] text-ink-ghost lowercase">{m.desc}</span>
                </span>
              ))}
            </div>
            <div className="flex flex-col px-4 py-3.5">
              {C001_MATCH.map((row) => (
                <div
                  key={row.field}
                  className="flex flex-wrap items-baseline gap-x-3 py-1.5 border-b border-rule-2 last:border-b-0"
                >
                  <FnChip>{row.field}</FnChip>
                  <span className="font-mono text-[11.5px] text-ink-faint">{row.matcher}</span>
                </div>
              ))}
            </div>
          </div>

          <SpecSheet title="C-E2E-002 · allowed function, exactly once" meta="two generations">
            <p className="font-mono text-[12px] leading-[1.7] text-ink-faint lowercase pb-2">
              generation one returns a function_call; the recorder answers; generation two must contain that exact
              durable result; the tools array is asserted against the recorder&apos;s own registration, not a prose
              approximation.
            </p>
            <div className="flex flex-col">
              {C002_INVARIANTS.map((inv) => (
                <div key={inv} className="flex items-baseline gap-x-2.5 py-1.5 border-b border-rule-2 last:border-b-0">
                  <span className="font-mono text-[12px] text-accent shrink-0">✓</span>
                  <span className="font-mono text-[12px] leading-[1.6] text-ink-faint lowercase">{inv}</span>
                </div>
              ))}
            </div>
          </SpecSheet>
        </div>
      </div>

      <div className="mt-6">
        <a
          href="#/integration-contracts"
          className="inline-flex h-10 items-center bg-bg text-ink border border-ink px-4 font-mono text-[13px] lowercase transition-colors hover:bg-ink hover:text-bg"
        >
          the compiler, script schema, recorder, and supervisor →
        </a>
      </div>
    </Section>
  )
}

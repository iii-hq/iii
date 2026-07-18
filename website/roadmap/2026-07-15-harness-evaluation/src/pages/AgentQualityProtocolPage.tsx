import { PageShell } from '@lib/components/PageShell'
import { SpecRow, SpecSheet } from '@lib/components/SpecSheet'
import { C, CodeBlock, K, M, S } from '@lib/components/schematic/CodeBlock'
import { FnChip } from '@lib/components/schematic/FnChip'
import { ERROR_CODES, EVAL_SURFACE, GENERATED_VALIDATOR_RULES, RECOVERY_TABLE, SCENARIO_CORPUS } from '../content/protocols'

/**
 * A14 — deep dive on the harness-eval protocol: the worker surface, the strict
 * manifest, durable recovery, and the initial scenario corpus.
 */
export function AgentQualityProtocolPage() {
  return (
    <PageShell
      eyebrow="deep dive · agent quality"
      title="the harness-eval protocol"
      description="one dedicated worker with a durable run record, a strict versioned manifest, a validation protocol every grader implements, and a recovery table that makes restarts deterministic. all proposed api, protocol_version 1, unknown fields denied."
      related={[{ slug: 'conformance-contracts', label: 'conformance contracts' }]}
    >
      <div>
        <div className="font-mono text-[11px] uppercase tracking-[0.14em] text-ink-faint mb-3">the worker surface</div>
        <div className="border border-rule bg-bg flex flex-col">
          {EVAL_SURFACE.map((row) => (
            <div key={row.fn} className="px-4 py-3 border-b border-rule-2 last:border-b-0">
              <FnChip>{row.fn}</FnChip>
              <p className="mt-1.5 font-mono text-[12px] leading-[1.6] text-ink-faint lowercase">{row.does}</p>
            </div>
          ))}
        </div>
      </div>

      <div>
        <div className="font-mono text-[11px] uppercase tracking-[0.14em] text-ink-faint mb-3">
          a scenario manifest, abridged
        </div>
        <CodeBlock title="fan-out-security-scan · strict yaml, frozen before start">
          <K>subject</K>:{'\n  '}message: <S>&quot;Inspect the fixture repository and persist the findings.&quot;</S>
          {'\n  '}model: <S>pinned-provider-model</S>
          {'\n  '}continuation: <S>explicit_override</S>
          {'   '}
          <C>{'# multi-cycle ⇒ every option pinned'}</C>
          {'\n  '}options: <M>{'{ '}</M>mode: <S>agent</S>, max_turns: <K>40</K>, functions: <M>{'{ '}</M>allow:{' '}
          <M>[</M>
          <S>state::get</S>, <S>database::query</S>, <S>harness::spawn</S>
          <M>]</M>, deny: <M>[</M>
          <S>harness-eval::*</S>
          <M>]</M> <M>{'} }'}</M>
          {'\n'}
          <K>fixtures</K>: <M>{'{ '}</M>profile: <S>repository-security-v1</S>, setup/teardown fns + timeouts,
          capability ids <M>{'}'}</M>
          {'\n'}
          <K>validators</K>:{'\n  '}- <S>validation::workflow::all-files-processed</S>
          {'   '}
          <C>{'# open · after_turn · continue_with_feedback'}</C>
          {'\n  '}- <S>eval-private::workflow::no-duplicate-results</S>
          {'  '}
          <C>{'# held_out · final · stop'}</C>
          {'\n  '}- source: <M>{'{ '}</M>kind: <S>generated</S>, goal: <S>&quot;…&quot;</S>, generator:{' '}
          <S>pinned-generator-model</S> <M>{'}'}</M>
          {'  '}
          <C>{'# agent_authored · held_out · advisory'}</C>
          {'\n'}
          <K>limits</K>: <M>{'{ '}</M>max_cycles: <K>4</K>, attempt_timeout_ms: <K>300000</K>, max_total_tokens:{' '}
          <K>100000</K>, max_cost_usd: <K>10</K>, max_network_requests: <K>0</K> <M>{'}'}</M>
        </CodeBlock>
        <p className="mt-3 font-mono text-[12px] leading-[1.7] text-ink-faint lowercase max-w-[72ch]">
          harness defaults are re-resolved on every send, so a multi-cycle scenario cannot truthfully promise to
          preserve an omitted value: <span className="text-ink">explicit_override</span> pins the prompt and every
          option; anything relying on a default must declare <span className="text-ink">single_cycle</span> with
          max_cycles 1. the manifest validator enforces this, and a requested function not declared by the fixture
          or dependency list makes the manifest invalid.
        </p>
      </div>

      <div>
        <div className="font-mono text-[11px] uppercase tracking-[0.14em] text-ink-faint mb-3">
          generated validators — authored on the fly, frozen before the turn
        </div>
        <div className="border border-rule bg-bg flex flex-col">
          {GENERATED_VALIDATOR_RULES.map((row) => (
            <div
              key={row.name}
              className="grid grid-cols-1 @2xl:grid-cols-[220px_minmax(0,1fr)] gap-x-4 px-4 py-2.5 border-b border-rule-2 last:border-b-0"
            >
              <span className="font-mono text-[12.5px] text-ink lowercase">{row.name}</span>
              <span className="font-mono text-[12px] leading-[1.6] text-ink-faint lowercase">{row.desc}</span>
            </div>
          ))}
        </div>
        <p className="mt-3 font-mono text-[12px] leading-[1.7] text-ink-faint lowercase max-w-[72ch]">
          the criterion is authored by an agent, but the trust shape is unchanged: the subject never sees, picks,
          or edits its judges, and the code digest that judged a run is auditable in the report.
        </p>
      </div>

      <div className="grid grid-cols-1 @4xl:grid-cols-2 gap-4">
        <SpecSheet title="stable error codes" meta="code: message on the bus" defaultOpen>
          <div className="flex flex-col">
            {ERROR_CODES.map((row) => (
              <SpecRow key={row.code} name={row.code}>
                {row.meaning}
              </SpecRow>
            ))}
          </div>
        </SpecSheet>

        <SpecSheet title="restart recovery, state by state" meta="single durable writer" defaultOpen>
          <div className="flex flex-col">
            {RECOVERY_TABLE.map((row) => (
              <SpecRow key={row.state} name={row.state}>
                {row.action}
              </SpecRow>
            ))}
          </div>
        </SpecSheet>
      </div>

      <div>
        <div className="font-mono text-[11px] uppercase tracking-[0.14em] text-ink-faint mb-3">
          the initial scenario corpus
        </div>
        <div className="border border-rule bg-bg flex flex-col">
          {SCENARIO_CORPUS.map((row) => (
            <div
              key={row.family}
              className="grid grid-cols-1 @2xl:grid-cols-[220px_minmax(0,1fr)] gap-x-4 px-4 py-2.5 border-b border-rule-2 last:border-b-0"
            >
              <span className="font-mono text-[12.5px] text-ink lowercase">{row.family}</span>
              <span className="font-mono text-[12px] leading-[1.6] text-ink-faint lowercase">{row.outcome}</span>
            </div>
          ))}
          <div className="px-4 py-3 border-t border-rule font-mono text-[11.5px] leading-[1.6] text-ink-faint lowercase">
            subjective model or visual graders arrive only where deterministic state cannot express the objective,
            and stay advisory until calibrated against a human-reviewed set with measured false rates.
          </div>
        </div>
      </div>
    </PageShell>
  )
}

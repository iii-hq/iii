import { PageShell } from '@lib/components/PageShell'
import { SpecRow, SpecSheet } from '@lib/components/SpecSheet'
import { C, CodeBlock, K, M, S } from '@lib/components/schematic/CodeBlock'
import { FnChip } from '@lib/components/schematic/FnChip'
import { AUTHORING_RULES, EVIDENCE_CONTRACTS, FUTURE_WORK, QUALITY_HELPERS, SCENARIO_CORPUS } from '../content/protocols'

/**
 * A14 — deep dive on the agent-quality suite: the test file as the protocol,
 * the minimal helpers, the proposed default evidence contracts, and the corpus.
 */
export function AgentQualityProtocolPage() {
  return (
    <PageShell
      eyebrow="deep dive · agent quality"
      title="the test file is the protocol"
      description="an ordinary vitest suite over public iii and harness functions. no scenario manifest, no wrapper dsl, no evaluator state machine: a developer reading a test file sees exactly how to orchestrate a headless harness programmatically. revised 2026-07-20 after design review."
      related={[{ slug: 'integration-contracts', label: 'integration contracts' }]}
    >
      <div>
        <div className="font-mono text-[11px] uppercase tracking-[0.14em] text-ink-faint mb-3">
          a test file, abridged
        </div>
        <CodeBlock title="order-refund-flow.test.ts · vitest + trigger(), nothing else">
          <K>const</K> subject <M>=</M> <M>{'{ '}</M>model: <S>&apos;pinned-provider-model&apos;</S>, options:{' '}
          <M>{'{ '}</M>system_prompt, system_prompt_strategy: <S>&apos;override&apos;</S>, functions <M>{'} }'}</M>
          {'\n\n'}
          <K>const</K> <M>{'{ '}</M>session_id<M>{' }'}</M> <M>=</M> <K>await</K> trigger(
          <S>&apos;harness::send&apos;</S>, <M>{'{ '}</M>...subject, message, idempotency_key: <S>`$&#123;RUN&#125;:refund:1`</S>
          <M>{' }'}</M>)
          {'\n'}
          <K>await</K> awaitTerminal(session_id)
          {'   '}
          <C>{'// events signal, durable status decides'}</C>
          {'\n\n'}
          <K>await</K> trigger(<S>&apos;harness::send&apos;</S>, <M>{'{ '}</M>...subject, session_id, message: next
          <M>{' }'}</M>)
          {'   '}
          <C>{'// a sequence is just the next send'}</C>
          {'\n'}
          <K>await</K> awaitTerminal(session_id)
          {'\n\n'}
          <K>const</K> refunds <M>=</M> <K>await</K> trigger(<S>&apos;database::query&apos;</S>, <M>{'{ '}</M>sql
          <M>{' }'}</M>)
          {'   '}
          <C>{'// durable outcome, not the agent’s claim'}</C>
          {'\n'}
          expect(refunds).toHaveLength(<K>1</K>)
          {'\n\n'}
          <K>const</K> metrics <M>=</M> <K>await</K> sessionMetrics(session_id)
          {'   '}
          <C>{'// whole session tree, or a typed throw'}</C>
          {'\n'}
          expect(metrics.totals.function_call_errors).toBe(<K>0</K>)
          {'\n'}
          expect(metrics.totals.cost_usd).toBeLessThan(<K>5</K>)
          {'\n\n'}
          <K>const</K> triggered <M>=</M> <K>await</K> triggeredWork(session_id)
          {'   '}
          <C>{'// trace spans — fails closed when missing'}</C>
          {'\n'}
          expect(triggered.function_call_errors).toBe(<K>0</K>)
        </CodeBlock>
        <p className="mt-3 font-mono text-[12px] leading-[1.7] text-ink-faint lowercase max-w-[72ch]">
          harness defaults re-resolve on every send, so the subject object pins model, provider, prompt strategy,
          and every option once, and every send spreads it verbatim. every idempotency and fixture key derives
          from the launcher-supplied run id, so a retried run cannot double-apply side effects.
        </p>
      </div>

      <div>
        <div className="font-mono text-[11px] uppercase tracking-[0.14em] text-ink-faint mb-3">
          the helpers — only where real platform work exists
        </div>
        <div className="border border-rule bg-bg flex flex-col">
          {QUALITY_HELPERS.map((row) => (
            <div key={row.fn} className="px-4 py-3 border-b border-rule-2 last:border-b-0">
              <FnChip>{row.fn}</FnChip>
              <p className="mt-1.5 font-mono text-[12px] leading-[1.6] text-ink-faint lowercase">{row.does}</p>
            </div>
          ))}
        </div>
        <p className="mt-3 font-mono text-[12px] leading-[1.7] text-ink-faint lowercase max-w-[72ch]">
          a helper never wraps an existing public api. when one proves generally useful it graduates into default
          harness api — the same read paths production orchestration code around the harness needs anyway.
        </p>
      </div>

      <div>
        <div className="font-mono text-[11px] uppercase tracking-[0.14em] text-ink-faint mb-3">
          evidence the harness builds by default
        </div>
        <div className="border border-rule bg-bg flex flex-col">
          {EVIDENCE_CONTRACTS.map((row) => (
            <div
              key={row.name}
              className="grid grid-cols-1 @2xl:grid-cols-[220px_minmax(0,1fr)] gap-x-4 px-4 py-2.5 border-b border-rule-2 last:border-b-0"
            >
              <span className="font-mono text-[12.5px] text-ink lowercase">
                {row.name}
                <span className="block font-mono text-[10.5px] uppercase tracking-[0.08em] text-ink-faint mt-0.5">
                  {row.type}
                </span>
              </span>
              <span className="font-mono text-[12px] leading-[1.6] text-ink-faint lowercase">{row.desc}</span>
            </div>
          ))}
        </div>
      </div>

      <div className="grid grid-cols-1 @4xl:grid-cols-2 gap-4">
        <SpecSheet title="authoring rules" meta="enforced in review" defaultOpen>
          <div className="flex flex-col">
            {AUTHORING_RULES.map((row) => (
              <SpecRow key={row.name} name={row.name}>
                {row.desc}
              </SpecRow>
            ))}
          </div>
        </SpecSheet>

        <SpecSheet title="future work — deferred, not redesigned" meta="enabled by the default assets" defaultOpen>
          <div className="flex flex-col">
            {FUTURE_WORK.map((row) => (
              <SpecRow key={row.name} name={row.name}>
                {row.desc}
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
            each row is one test file. no llm judges the suite: an llm is only ever the subject, and every check
            stays explicit code a reviewer can read.
          </div>
        </div>
      </div>
    </PageShell>
  )
}

import { PageShell } from '@lib/components/PageShell'
import { SpecRow, SpecSheet } from '@lib/components/SpecSheet'
import { C, CodeBlock, K, M, S } from '@lib/components/schematic/CodeBlock'
import { FnChip } from '@lib/components/schematic/FnChip'
import {
  AUTHORING_RULES,
  EVIDENCE_CONTRACTS,
  EXCLUDED_CAPABILITIES,
  POST_V1_CORPUS,
  QUALITY_HELPERS,
  V1_CORPUS,
} from '../content/protocols'

/**
 * A14 — deep dive on the e2e suite: the test file as the protocol, the
 * helpers, the versioned evidence contracts, the artifact layout, and the
 * corpus. In the canonical markdown this track is agent-quality.md.
 */
export function E2EProtocolPage() {
  return (
    <PageShell
      eyebrow="deep dive · e2e tests"
      title="the test file is the protocol"
      description="an ordinary vitest suite over public iii and harness functions. no scenario manifest, no wrapper dsl, no evaluator state machine: subject configuration, prompt sequence, fixtures, assertions, and budgets all live in the file. version 1 computes no aggregate quality score."
      related={[{ slug: 'integration-contracts', label: 'integration contracts' }]}
    >
      <div>
        <div className="font-mono text-[11px] uppercase tracking-[0.14em] text-ink-faint mb-3">
          a test file, abridged
        </div>
        <CodeBlock title="single-function-refund.test.ts · vitest + worker.trigger, nothing else">
          <K>const</K> RUN <M>=</M> runId()
          {'   '}
          <C>{'// stack-scoped identity from the launcher'}</C>
          {'\n'}
          <K>const</K> iii <M>=</M> registerWorker(III_URL, <M>{'{ '}</M>workerName: <S>`agent-quality-$&#123;RUN&#125;`</S>
          <M>{' }'}</M>)
          {'\n'}
          <K>const</K> sessions <M>=</M> createSessionRegistry(iii)
          {'\n\n'}
          <K>const</K> subject <M>=</M> <M>{'{ '}</M>model, provider, options: <M>{'{ '}</M>system_prompt,
          system_prompt_strategy: <S>&apos;override&apos;</S>, functions <M>{'} }'}</M>
          {'\n\n'}
          <K>const</K> first <M>=</M> <K>await</K> iii.trigger(<M>{'{ '}</M>function_id:{' '}
          <S>&apos;harness::send&apos;</S>,
          {'\n  '}payload: <M>{'{ '}</M>...subject, message, idempotency_key: <S>`$&#123;RUN&#125;:refund:1`</S>
          <M>{' } }'}</M>)
          {'\n'}
          sessions.track(first.session_id)
          {'   '}
          <C>{'// tracked before anything can fail'}</C>
          {'\n'}
          <K>await</K> awaitTerminal(iii, first.session_id, first.turn_id)
          {'\n\n'}
          <K>const</K> refunds <M>=</M> <K>await</K> iii.trigger(<M>{'{ '}</M>function_id:{' '}
          <S>&apos;database::query&apos;</S>, payload: <M>{'{ '}</M>sql<M>{' } }'}</M>)
          {'\n'}
          expect(refunds).toHaveLength(<K>1</K>)
          {'   '}
          <C>{'// durable outcome, not the agent’s claim'}</C>
          {'\n\n'}
          <K>const</K> metrics <M>=</M> <K>await</K> sessionMetrics(iii, first.session_id)
          {'   '}
          <C>{'// whole session tree, or a typed throw'}</C>
          {'\n'}
          expect(metrics.totals.function_call_errors).toBe(<K>0</K>)
          {'\n'}
          expect(metrics.totals.cost_usd).toBeLessThan(<K>5</K>)
          {'\n\n'}
          <K>const</K> triggered <M>=</M> <K>await</K> triggeredWork(iii, first.session_id)
          {'   '}
          <C>{'// polls until complete; fails closed'}</C>
          {'\n'}
          expect(triggered.spans.every((s) <M>=&gt;</M> s.status <M>===</M> <S>&apos;ok&apos;</S>)).toBe(
          <K>true</K>)
        </CodeBlock>
        <p className="mt-3 font-mono text-[12px] leading-[1.7] text-ink-faint lowercase max-w-[72ch]">
          harness defaults re-resolve on every send, so the subject object pins model, provider, prompt strategy,
          and every option once, and every send spreads it verbatim. every idempotency and fixture key derives
          from the launcher-supplied run id, so a retried run cannot double-apply side effects. all direct test
          calls are synchronous invocations; the harness queue stays part of the subject path.
        </p>
      </div>

      <div>
        <div className="font-mono text-[11px] uppercase tracking-[0.14em] text-ink-faint mb-3">
          the helpers — coordination a single public call cannot express
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
          a helper never renames or wraps a single existing public call. default platform reads belong in the
          harness api; helpers only coordinate lifecycle signals, pagination, completeness checks, artifact
          persistence, and test cleanup. helpers never create a hidden sdk connection — the client is always
          passed explicitly.
        </p>
      </div>

      <div>
        <div className="font-mono text-[11px] uppercase tracking-[0.14em] text-ink-faint mb-3">
          the versioned evidence contracts
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

      <div>
        <div className="font-mono text-[11px] uppercase tracking-[0.14em] text-ink-faint mb-3">
          what a run leaves behind
        </div>
        <CodeBlock title="target/agent-quality/<run_id>/ · digests are sha-256 of exact bytes">
          stack.json
          {'        '}
          <C>{'// requested + resolved model ids, route, digests'}</C>
          {'\n'}
          results.json
          {'      '}
          <C>{'// AgentQualityResultV1: pass/fail + artifact digests'}</C>
          {'\n'}
          tests/<S>&lt;test-id&gt;</S>/
          {'\n  '}sends.json
          {'      '}
          <C>{'// every send request/response pair'}</C>
          {'\n  '}failure.json
          {'    '}
          <C>{'// on failure: ordered records, class + phase + code'}</C>
          {'\n  '}status.json <M>·</M> session-tree.json <M>·</M> metrics.json
          {'\n  '}transcript.json
          {'  '}
          <C>{'// all pages, all sessions in the tree'}</C>
          {'\n  '}triggered-spans.json <M>·</M> evidence/
        </CodeBlock>
        <p className="mt-3 font-mono text-[12px] leading-[1.7] text-ink-faint lowercase max-w-[72ch]">
          ci publishes results.json for every run and uploads full evidence for non-pass runs with 14-day
          retention. provider credentials enter only through an environment allowlist and never appear in config
          or artifacts.
        </p>
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

        <SpecSheet title="excluded from version 1" meta="the raw assets already allow manual comparison" defaultOpen>
          <div className="flex flex-col">
            {EXCLUDED_CAPABILITIES.map((row) => (
              <SpecRow key={row.name} name={row.name}>
                {row.desc}
              </SpecRow>
            ))}
          </div>
        </SpecSheet>
      </div>

      <div>
        <div className="font-mono text-[11px] uppercase tracking-[0.14em] text-ink-faint mb-3">
          the corpus — four version 1 files, then the expansion
        </div>
        <div className="border border-rule bg-bg flex flex-col">
          <div className="bg-panel px-4 py-2 border-b border-rule font-mono text-[10px] uppercase tracking-[0.14em] text-ink-faint">
            version 1 · the gate
          </div>
          {V1_CORPUS.map((row) => (
            <div
              key={row.family}
              className="grid grid-cols-1 @2xl:grid-cols-[220px_minmax(0,1fr)] gap-x-4 px-4 py-2.5 border-b border-rule-2"
            >
              <span className="font-mono text-[12.5px] text-ink lowercase">{row.family}</span>
              <span className="font-mono text-[12px] leading-[1.6] text-ink-faint lowercase">{row.outcome}</span>
            </div>
          ))}
          <div className="bg-panel px-4 py-2 border-b border-rule font-mono text-[10px] uppercase tracking-[0.14em] text-ink-faint">
            post-v1 expansion
          </div>
          {POST_V1_CORPUS.map((row) => (
            <div
              key={row.family}
              className="grid grid-cols-1 @2xl:grid-cols-[220px_minmax(0,1fr)] gap-x-4 px-4 py-2.5 border-b border-rule-2 last:border-b-0"
            >
              <span className="font-mono text-[12.5px] text-ink-faint lowercase">{row.family}</span>
              <span className="font-mono text-[12px] leading-[1.6] text-ink-faint lowercase">{row.outcome}</span>
            </div>
          ))}
          <div className="px-4 py-3 border-t border-rule font-mono text-[11.5px] leading-[1.6] text-ink-faint lowercase">
            version 1 is accepted when all four real-model tests pass through public boundaries, session-tree
            metrics include at least one sub-agent, the triggered-work test verifies complete error-free spans,
            and withheld spans produce evidence_error. each row is one test file; no llm judges the suite.
          </div>
        </div>
      </div>
    </PageShell>
  )
}

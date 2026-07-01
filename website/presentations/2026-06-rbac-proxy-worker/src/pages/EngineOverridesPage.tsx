import { StepReveal } from '@lib/components/diagrams/StepReveal'
import { PageShell } from '@lib/components/PageShell'
import { SpecRow, SpecSheet } from '@lib/components/SpecSheet'
import { C, CodeBlock, K, M } from '@lib/components/schematic/CodeBlock'
import { FnChip } from '@lib/components/schematic/FnChip'
import { OVERRIDE_STAGES, OVERRIDE_TABLE } from '../content/flow'

/**
 * A14 — deep dive on engine:: discovery overrides: the override flow, the full
 * rewrite table, the caches the rewrites depend on, and the leak policy.
 */
export function EngineOverridesPage() {
  return (
    <PageShell
      eyebrow="deep dive"
      title="engine:: discovery overrides"
      description="the addition over the engine's rbac contract. the engine still computes each answer; the proxy intercepts the request to mark it, lets the engine compute the full result, then filters that result to the caller's boundaries with the same is_function_allowed used on the invoke path."
      related={[{ slug: 'rbac-contract', label: 'the rbac contract' }]}
    >
      <StepReveal title="engine::workers::list, filtered client-side" stages={OVERRIDE_STAGES} />

      <div>
        <div className="font-mono text-[11px] uppercase tracking-[0.14em] text-ink-faint mb-3">the eight rewrites</div>
        <div className="border border-rule bg-bg overflow-x-auto">
          <div className="min-w-[680px] flex flex-col">
            <div className="grid grid-cols-[240px_1fr_200px] gap-x-4 bg-panel px-4 py-2.5 border-b border-rule font-mono text-[10px] uppercase tracking-[0.12em] text-ink-faint">
              <span>function</span>
              <span>result rewrite</span>
              <span>empty / denied</span>
            </div>
            {OVERRIDE_TABLE.map((row) => (
              <div
                key={row.fn}
                className="grid grid-cols-[240px_1fr_200px] gap-x-4 px-4 py-2.5 border-b border-rule-2 last:border-b-0"
              >
                <span className="min-w-0">
                  <FnChip tone="ink" className="max-w-full overflow-hidden">
                    {row.fn}
                  </FnChip>
                </span>
                <span className="font-mono text-[12px] leading-[1.5] text-ink-faint lowercase">{row.rewrite}</span>
                <span className="font-mono text-[12px] leading-[1.5] text-ink-ghost lowercase">{row.policy}</span>
              </div>
            ))}
          </div>
        </div>
      </div>

      <div className="grid grid-cols-1 @4xl:grid-cols-2 gap-4">
        <CodeBlock title="the two caches, over the control connection">
          <C>{'// function catalog, for metadata filters + workers::list'}</C>
          {'\n'}
          <K>function_id</K> <M>→</M> <M>{'{'}</M> worker_name, metadata <M>{'}'}</M>
          {'\n\n'}
          <C>{'// binding index, for triggers::info instance_count'}</C>
          {'\n'}
          <K>registered_trigger_id</K> <M>→</M> <M>{'{'}</M> trigger_type, function_id <M>{'}'}</M>
          {'\n'}
          <K>trigger_type</K> <M>→</M> <M>[</M> function_id <M>]</M>
        </CodeBlock>

        <SpecSheet title="cache-miss semantics" meta="not uniformly fail-closed" defaultOpen>
          <div className="flex flex-col">
            <SpecRow name="wildcard filter" type="allow on cold cache">
              match(&quot;api::*&quot;) needs only the id string, never the catalog, so a never-seen api::new is
              correctly allowed on a miss.
            </SpecRow>
            <SpecRow name="metadata filter" type="fail closed on miss">
              needs the cached metadata, so a freshly-registered metadata-gated function is briefly invisible until the
              catalog refreshes, never wrongly exposed.
            </SpecRow>
            <SpecRow name="refresh" type="ttl + trigger">
              lazy on a short ttl, proactively on engine::functions-available, mirroring console&apos;s function-list
              cache.
            </SpecRow>
          </div>
        </SpecSheet>
      </div>

      <SpecSheet title="worker-internals leak policy" meta="expose_worker_internals">
        <div className="flex flex-col">
          <SpecRow name="false (default)" type="strip">
            drop ip_address + isolation from WorkerSummary, and additionally pid + internal + latest_metrics from
            WorkerDetailEnvelope. the caller still sees name, version, status, recomputed function_count,
            connected_at_ms.
          </SpecRow>
          <SpecRow name="true" type="pass through">
            single-tenant or operator-trusted deployments where the full picture is wanted.
          </SpecRow>
          <SpecRow name="per-session" type="optional">
            key the decision off AuthResult.context (e.g. context.admin === true) in a small policy hook instead of the
            global knob.
          </SpecRow>
        </div>
      </SpecSheet>

      <SpecSheet title="prefix in results" meta="strip own namespace">
        <p className="font-mono text-[13px] leading-[1.7] text-ink-faint lowercase">
          a session&apos;s own functions live in the registry as {'{prefix}::{id}'} and surface that way in discovery.
          the worker registered them bare and must see them bare, so the proxy strips its own session prefix from every
          id and worker name in a result. a foreign id is shown canonically. this is symmetric with the dispatch-path
          strip: the worker only ever sees foo, never tenant1::foo.
        </p>
      </SpecSheet>
    </PageShell>
  )
}

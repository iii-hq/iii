import { StepReveal } from '@lib/components/diagrams/StepReveal'
import { Section } from '@lib/components/Section'
import { SpecSheet } from '@lib/components/SpecSheet'
import { FnChip } from '@lib/components/schematic/FnChip'
import { StatusPanel } from '@lib/components/schematic/StatusPanel'
import { OVERRIDE_STAGES, OVERRIDE_TABLE } from '../content/flow'

/**
 * A6 — the engine:: discovery overrides. The addition over the engine's RBAC
 * contract: step the override flow, then drill the per-function rewrite table.
 * Carries the honest trade-off beat.
 */
export function OverridesSection() {
  return (
    <Section
      id="overrides"
      index="06"
      eyebrow="discovery overrides"
      title="discovery shows only what you can call."
      lede="an engine rbac listener gates invocation but only partly filters discovery: 2 of 8 functions, and only in-process. the proxy lets the engine compute the full result, then rewrites all eight to the caller's boundaries before they reach the worker."
    >
      <StepReveal title="engine::workers::list, filtered client-side" stages={OVERRIDE_STAGES} />

      <div className="mt-6">
        <StatusPanel
          variant="warn"
          icon={<span>!</span>}
          headline="the honest limit: a shared expose list can still leak a foreign prefix"
          detail="with expose [{ metadata: { public: true } }], another tenant's tenant2::foo tagged public passes the filter and is shown. hard per-tenant isolation must live in the auth function, not a global metadata filter."
        />
      </div>

      <div className="mt-6 grid grid-cols-1 gap-4">
        <SpecSheet title="the eight rewrites" meta="filter table">
          <div className="overflow-x-auto">
            <div className="min-w-[680px] flex flex-col">
              <div className="grid grid-cols-[240px_1fr_200px] gap-x-4 px-1 pb-2 border-b border-rule font-mono text-[10px] uppercase tracking-[0.12em] text-ink-faint">
                <span>function</span>
                <span>result rewrite</span>
                <span>empty / denied</span>
              </div>
              {OVERRIDE_TABLE.map((row) => (
                <div
                  key={row.fn}
                  className="grid grid-cols-[240px_1fr_200px] gap-x-4 px-1 py-2.5 border-b border-rule-2 last:border-b-0"
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
        </SpecSheet>

        <SpecSheet title="why not just patch the engine?" meta="the design choice">
          <p className="font-mono text-[13px] leading-[1.7] text-ink-faint lowercase">
            making all eight discovery functions session-aware in the engine is the smaller change if rbac stays
            in-engine, and it is the right fix for worker-gateway. but it does nothing for a proxy fronting a remote or
            managed engine you cannot patch. filtering in the proxy makes discovery correct regardless of engine version
            or operator, relying only on the stable engine::* shapes. if the engine later gains full session-aware
            discovery, the overrides become a harmless redundant layer that can be dropped per function.
          </p>
        </SpecSheet>
      </div>

      <div className="mt-6">
        <a
          href="#/engine-overrides"
          className="inline-flex h-10 items-center bg-bg text-ink border border-ink px-4 font-mono text-[13px] lowercase transition-colors hover:bg-ink hover:text-bg"
        >
          the caches, the leak policy, the prefix strip →
        </a>
      </div>
    </Section>
  )
}

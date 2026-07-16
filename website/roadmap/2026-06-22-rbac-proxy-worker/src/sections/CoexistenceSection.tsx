import { Section } from '@lib/components/Section'
import { SpecRow, SpecSheet } from '@lib/components/SpecSheet'
import { COMPARE_ROWS } from '../content/coexistence'

/**
 * A11-style contrast — the adoption beat. The engine-native worker-gateway is
 * the foil, not the enemy: same contract, different home. Migration is a config
 * copy, so the perceived risk of adopting is low.
 */
export function CoexistenceSection() {
  return (
    <Section
      id="coexistence"
      index="07"
      eyebrow="coexistence"
      title="run the gateway, the proxy, or both."
      lede="this is not a re-implementation for its own sake. it re-homes the same contract out-of-process, and the two do not collide: the engine runs its trusted internal listener with no rbac block, and the proxy is the only public door."
    >
      <div className="overflow-x-auto">
        <div className="min-w-[620px] grid grid-cols-[minmax(0,1.1fr)_minmax(0,1fr)_minmax(0,1fr)] gap-px bg-rule border border-rule">
          <div className="bg-panel px-4 py-2.5 font-mono text-[10px] uppercase tracking-[0.14em] text-ink-faint">
            property
          </div>
          <div className="bg-panel px-4 py-2.5 font-mono text-[10px] uppercase tracking-[0.14em] text-ink-faint">
            worker-gateway
          </div>
          <div className="bg-panel px-4 py-2.5 font-mono text-[10px] uppercase tracking-[0.14em] text-accent">
            rbac-proxy
          </div>
          {COMPARE_ROWS.map((row) => (
            <div key={row.property} className="contents">
              <div className="bg-bg px-4 py-3.5 font-mono text-[12.5px] leading-[1.5] text-ink lowercase">
                {row.property}
              </div>
              <div className="bg-bg px-4 py-3.5 font-mono text-[12.5px] leading-[1.5] text-ink-faint lowercase">
                {row.gateway}
              </div>
              <div className="bg-bg px-4 py-3.5 min-w-0">
                <span
                  className={`font-mono text-[12.5px] leading-[1.5] lowercase ${
                    row.win ? 'text-accent' : 'text-ink-faint'
                  }`}
                >
                  {row.proxy}
                </span>
              </div>
            </div>
          ))}
        </div>
      </div>

      <div className="mt-6">
        <SpecSheet title="migration is a config copy" meta="same field set">
          <p className="font-mono text-[13px] leading-[1.7] text-ink-faint lowercase mb-4">
            the proxy&apos;s WorkerConfig is intentionally the same field set as the engine&apos;s gateway: block, so an
            operator&apos;s mental model transfers and moving between the two is a copy, not a rewrite.
          </p>
          <div className="flex flex-col">
            <SpecRow name="host · port · engine_url" type="listener">
              where the public door binds and which engine it fronts.
            </SpecRow>
            <SpecRow name="middleware_function_id" type="wrap">
              the optional middleware over every allowed, non-engine:: call.
            </SpecRow>
            <SpecRow name="expose_worker_internals" type="leak knob">
              strip pid / ip / metrics from engine::workers::* results by default.
            </SpecRow>
            <SpecRow name="rbac.auth_function_id" type="auth">
              the per-upgrade auth function that returns the session boundaries.
            </SpecRow>
            <SpecRow name="rbac.expose_functions" type="filters">
              the wildcard and metadata filters, identical to the engine matcher.
            </SpecRow>
            <SpecRow name="rbac.on_*_registration" type="hooks">
              the three registration hooks, ported over unchanged.
            </SpecRow>
          </div>
        </SpecSheet>
      </div>
    </Section>
  )
}

import { Section } from '@lib/components/Section'
import { SpecSheet } from '@lib/components/SpecSheet'
import { FnChip } from '@lib/components/schematic/FnChip'
import { DISCOVERY_LEAK, PAIN_CARDS } from '../content/why'

/**
 * A2 — the problem. Name today's failures as concrete cards, then prove the
 * worst one (the discovery leak) with a function-by-function datasheet.
 */
export function WhySection() {
  const sessionAware = DISCOVERY_LEAK.filter((r) => r.sessionAware).length

  return (
    <Section
      id="why"
      index="01"
      eyebrow="the problem"
      title="engine-native rbac can only guard an engine you own."
      lede="the same access-control surface already ships inside the engine, as engine config. that buys a lot, and it rules out everything below."
    >
      <div className="grid grid-cols-1 @2xl:grid-cols-2 gap-px bg-rule border border-rule">
        {PAIN_CARDS.map((card) => (
          <div key={card.n} className="bg-bg p-6 @3xl:p-7 min-w-0">
            <div className="flex items-baseline gap-x-2.5 mb-3">
              <span className="font-mono text-[11px] text-alert tabular-nums">{card.n}</span>
              <span className="font-mono text-[15px] font-semibold text-ink lowercase">{card.title}</span>
            </div>
            <p className="font-mono text-[13px] leading-[1.7] text-ink-faint lowercase max-w-[46ch]">{card.body}</p>
          </div>
        ))}
      </div>

      <div className="mt-8">
        <SpecSheet
          title="the discovery leak, function by function"
          meta={`${sessionAware} of ${DISCOVERY_LEAK.length} session-aware`}
        >
          <div className="overflow-x-auto">
            <div className="min-w-[560px] flex flex-col">
              <div className="grid grid-cols-[260px_88px_1fr] gap-x-4 px-1 pb-2 border-b border-rule font-mono text-[10px] uppercase tracking-[0.12em] text-ink-faint">
                <span>discovery function</span>
                <span>per-session</span>
                <span>still leaks</span>
              </div>
              {DISCOVERY_LEAK.map((row) => (
                <div
                  key={row.fn}
                  className="grid grid-cols-[260px_88px_1fr] gap-x-4 items-center px-1 py-2 border-b border-rule-2 last:border-b-0"
                >
                  <FnChip tone={row.sessionAware ? 'ink' : 'alert'}>{row.fn}</FnChip>
                  <span className={`font-mono text-[12px] ${row.sessionAware ? 'text-accent' : 'text-alert'}`}>
                    {row.sessionAware ? '✓ partial' : '✗ none'}
                  </span>
                  <span className="font-mono text-[12px] leading-[1.5] text-ink-faint lowercase">{row.leaks}</span>
                </div>
              ))}
            </div>
          </div>
          <p className="mt-4 font-mono text-[12px] leading-[1.7] text-ink-faint lowercase">
            and even the two session-aware functions filter against an in-process session a remote proxy never receives.
            so a gated worker can read schemas, owners, and host metadata for functions it can never invoke.
          </p>
        </SpecSheet>
      </div>
    </Section>
  )
}

import { Section } from '@lib/components/Section'
import { PAYOFF_STATS, SOLVES_ROWS } from '../content/payoff'

export function PayoffSection() {
  return (
    <Section
      id="payoff"
      index="11"
      eyebrow="the payoff"
      title="green means the substrate held and the pinned agent delivered."
      lede="integration gives a reproducible answer about harness contracts. quality/E2E gives an evidence-backed answer about real-model outcomes and resource use. together they provide release confidence without confusing determinism with capability."
    >
      <div className="grid grid-cols-2 gap-px border border-rule bg-rule @3xl:grid-cols-3 @5xl:grid-cols-6">
        {PAYOFF_STATS.map((stat) => (
          <div key={stat.label} className="min-w-0 bg-bg px-4 py-5">
            <div className="font-mono text-[10px] leading-[1.4] tracking-[0.08em] text-ink-ghost uppercase">
              {stat.before}
            </div>
            <div className="mt-3 font-mono text-[26px] leading-none font-semibold text-accent tabular-nums">
              {stat.after}
            </div>
            <div className="mt-2 font-mono text-[11px] leading-[1.45] text-ink-faint lowercase">{stat.label}</div>
          </div>
        ))}
      </div>

      <div className="mt-8 border border-rule">
        <div className="grid grid-cols-2 border-b border-rule bg-panel px-4 py-2.5">
          <span className="font-mono text-[10px] tracking-[0.12em] text-alert uppercase">risk</span>
          <span className="font-mono text-[10px] tracking-[0.12em] text-accent uppercase">answer</span>
        </div>
        <div className="grid grid-cols-1 gap-px bg-rule">
          {SOLVES_ROWS.map((row) => (
            <div key={row.problem} className="grid grid-cols-1 bg-bg @3xl:grid-cols-2">
              <p className="border-b border-rule px-4 py-4 font-mono text-[12.5px] leading-[1.6] text-ink-faint lowercase @3xl:border-r @3xl:border-b-0">
                {row.problem}
              </p>
              <p className="px-4 py-4 font-mono text-[12.5px] leading-[1.6] text-ink lowercase">{row.answer}</p>
            </div>
          ))}
        </div>
      </div>

      <div className="mt-8 flex flex-wrap items-center gap-3">
        <a
          href="#/protocol"
          className="inline-flex h-11 items-center border border-ink bg-ink px-5 font-mono text-[14px] text-bg lowercase transition-colors hover:bg-bg hover:text-ink"
        >
          integration protocol →
        </a>
        <a
          href="#/quality"
          className="inline-flex h-11 items-center border border-ink bg-bg px-5 font-mono text-[14px] text-ink lowercase transition-colors hover:bg-ink hover:text-bg"
        >
          quality protocol
        </a>
        <a
          href="#/scenarios"
          className="font-mono text-[13px] text-ink-faint lowercase transition-colors hover:text-accent"
        >
          compare all scenarios →
        </a>
        <a href="#/spec" className="font-mono text-[13px] text-ink-faint lowercase transition-colors hover:text-accent">
          read the full spec →
        </a>
      </div>
    </Section>
  )
}

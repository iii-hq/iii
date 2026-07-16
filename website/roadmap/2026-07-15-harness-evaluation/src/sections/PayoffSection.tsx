import { Section } from '@lib/components/Section'
import { OPEN_QUESTIONS, PAYOFF_METRICS, PAYOFF_SOLVES } from '../content/payoff'

/**
 * A11 — the payoff. A before/after scorecard, the problem → answer table, and
 * the honest limits: this is a proposed architecture with open questions.
 */
export function PayoffSection() {
  return (
    <Section
      id="payoff"
      index="10"
      eyebrow="the payoff"
      title="a gate that is earned, not assumed."
      lede="deterministic contract regression on every pull request, real-model workflow measurement on a schedule, and a promotion policy that makes the gate prove itself before it can block anyone."
    >
      {/* scorecard */}
      <div className="grid grid-cols-2 @3xl:grid-cols-4 border-x border-t border-rule bg-rule gap-px">
        {PAYOFF_METRICS.map((m) => (
          <div key={m.label} className="bg-bg px-4 py-5 min-w-0">
            <div className="flex items-baseline gap-x-2 flex-wrap">
              <span className="font-mono text-[13px] text-ink-ghost line-through">{m.before}</span>
              <span className="font-mono text-[12px] text-ink-ghost">→</span>
              <span className="font-mono text-[20px] @3xl:text-[24px] font-semibold text-accent leading-none">
                {m.after}
              </span>
            </div>
            <div className="mt-2 font-mono text-[10px] uppercase tracking-[0.06em] text-ink-faint">{m.label}</div>
          </div>
        ))}
      </div>

      {/* problem → answer */}
      <div className="mt-8 grid grid-cols-1 @3xl:grid-cols-2 border border-rule bg-rule gap-px">
        <div className="bg-panel px-4 py-2.5 font-mono text-[10px] uppercase tracking-[0.14em] text-ink-faint">
          the problem
        </div>
        <div className="bg-panel px-4 py-2.5 font-mono text-[10px] uppercase tracking-[0.14em] text-ink-faint hidden @3xl:block">
          the answer
        </div>
        {PAYOFF_SOLVES.map((row) => (
          <div key={row.problem} className="contents">
            <div className="bg-bg px-4 py-4 min-w-0">
              <div className="font-mono text-[13px] text-alert lowercase">{row.problem}</div>
            </div>
            <div className="bg-bg px-4 py-4 min-w-0">
              <div className="font-mono text-[13px] text-accent lowercase">{row.answer}</div>
              <div className="mt-1 font-mono text-[12px] leading-[1.6] text-ink-faint lowercase">{row.detail}</div>
            </div>
          </div>
        ))}
      </div>

      {/* honest limits */}
      <div className="mt-8 border border-rule bg-bg">
        <div className="bg-panel px-3.5 py-2 border-b border-rule font-mono text-[11px] font-medium uppercase tracking-[0.06em] text-ink-faint">
          honest limits · proposed architecture, implementation not started
        </div>
        <div className="grid grid-cols-1 @3xl:grid-cols-3 gap-px bg-rule">
          {OPEN_QUESTIONS.map((q) => (
            <div key={q.q} className="bg-bg px-4 py-4 min-w-0">
              <div className="font-mono text-[12.5px] font-semibold text-ink lowercase">{q.q}</div>
              <p className="mt-1.5 font-mono text-[12px] leading-[1.65] text-ink-faint lowercase">{q.detail}</p>
            </div>
          ))}
        </div>
        <div className="px-4 py-3 border-t border-rule font-mono text-[11.5px] leading-[1.6] text-ink-faint lowercase">
          none of these block the first implementation slice. the markdown spec remains canonical.{' '}
          <a href="#/spec" className="text-ink hover:text-accent transition-colors">
            read it in full →
          </a>
        </div>
      </div>
    </Section>
  )
}

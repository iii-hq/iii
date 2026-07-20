import { Funnel } from '@lib/components/diagrams/Funnel'
import { Section } from '@lib/components/Section'
import { NEVER_A_PASS, STATUS_MEANINGS, VERDICT_PATHS, VERDICT_REJECT, VERDICT_TARGET } from '../content/verdicts'

const TONE_TEXT = {
  accent: 'text-accent',
  alert: 'text-alert',
  warn: 'text-warn',
} as const

/**
 * A8 — every level of the run folds through the same precedence rule; the
 * eliminated path is a weighted composite score.
 */
export function VerdictsSection() {
  return (
    <Section
      id="verdicts"
      index="10"
      eyebrow="agent quality · verdicts"
      title="explicit assertions, complete evidence, nothing silent."
      lede="every check is an expect() a reviewer can read in the file. helpers throw on incomplete evidence, so a partial sum is never graded — and no model grader or blended score sits between the evidence and the verdict."
    >
      <Funnel
        title="how a test earns its verdict"
        paths={VERDICT_PATHS}
        target={VERDICT_TARGET}
        reject={VERDICT_REJECT}
      />

      <div className="mt-6 grid grid-cols-1 @4xl:grid-cols-[minmax(0,1fr)_minmax(0,340px)] gap-4">
        <div className="border border-rule bg-bg">
          <div className="bg-panel px-3.5 py-2 border-b border-rule font-mono text-[11px] font-medium uppercase tracking-[0.06em] text-ink-faint">
            what each status means
          </div>
          <div className="grid grid-cols-1 @2xl:grid-cols-2 gap-px bg-rule">
            {STATUS_MEANINGS.map((s) => (
              <div key={s.id} className="bg-bg px-4 py-3.5 min-w-0">
                <span className={`font-mono text-[13px] font-semibold ${TONE_TEXT[s.tone]}`}>{s.id}</span>
                <p className="mt-1.5 font-mono text-[12px] leading-[1.6] text-ink-faint lowercase">{s.desc}</p>
              </div>
            ))}
          </div>
          <div className="px-4 py-3 border-t border-rule font-mono text-[11.5px] leading-[1.6] text-ink-faint lowercase">
            custom domain checks are ordinary iii functions the test calls with{' '}
            <span className="text-ink">trigger()</span> and asserts on — not a validator protocol with its own
            lifecycle.
          </div>
        </div>

        <div className="border border-rule bg-bg self-start">
          <div className="bg-panel px-3.5 py-2 border-b border-rule font-mono text-[11px] font-medium uppercase tracking-[0.06em] text-ink-faint">
            never converted to a subject pass
          </div>
          <div className="flex flex-col">
            {NEVER_A_PASS.map((item) => (
              <div key={item} className="flex items-baseline gap-x-2.5 px-4 py-2.5 border-b border-rule-2 last:border-b-0">
                <span className="font-mono text-[12px] text-alert shrink-0">✗</span>
                <span className="font-mono text-[12.5px] text-ink-faint lowercase">{item}</span>
              </div>
            ))}
          </div>
        </div>
      </div>
    </Section>
  )
}

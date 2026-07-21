import { Funnel } from '@lib/components/diagrams/Funnel'
import { Section } from '@lib/components/Section'
import { FAILURE_CLASSES, NEVER_A_PASS, VERDICT_PATHS, VERDICT_REJECT, VERDICT_TARGET } from '../content/verdicts'

const TONE_TEXT = {
  accent: 'text-accent',
  alert: 'text-alert',
  warn: 'text-warn',
} as const

/**
 * A8 — every evidence stream folds through the same completeness rule; the
 * eliminated path is a weighted composite score. Below it, the six typed
 * failure classes of AgentQualityFailureReportV1.
 */
export function VerdictsSection() {
  return (
    <Section
      id="verdicts"
      index="10"
      eyebrow="e2e tests · verdicts"
      title="explicit assertions, complete evidence, typed failure."
      lede="every check is an expect() a reviewer can read in the file, and every red run carries a class and a phase. helpers throw on incomplete evidence, so a partial sum is never graded — and no model grader or blended score sits between the evidence and the verdict."
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
            AgentQualityFailureReportV1 · six classes, ordered by phase
          </div>
          <div className="grid grid-cols-1 @2xl:grid-cols-2 gap-px bg-rule">
            {FAILURE_CLASSES.map((c) => (
              <div key={c.id} className="bg-bg px-4 py-3.5 min-w-0">
                <div className="flex items-baseline justify-between gap-x-3">
                  <span className={`font-mono text-[13px] font-semibold ${TONE_TEXT[c.tone]}`}>{c.id}</span>
                  <span className="font-mono text-[10.5px] text-ink-ghost lowercase">phase: {c.phase}</span>
                </div>
                <p className="mt-1.5 font-mono text-[12px] leading-[1.6] text-ink-faint lowercase">{c.desc}</p>
              </div>
            ))}
          </div>
          <div className="px-4 py-3 border-t border-rule font-mono text-[11.5px] leading-[1.6] text-ink-faint lowercase">
            every failed test writes <span className="text-ink">failure.json</span>: ordered records by phase,
            exact engine/sdk codes preserved, credentials redacted. any record fails the vitest test and the ci
            run.
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
          <div className="px-4 py-3 border-t border-rule font-mono text-[11.5px] leading-[1.6] text-ink-faint lowercase">
            custom domain checks are registered iii functions the test calls with{' '}
            <span className="text-ink">worker.trigger</span> and asserts on — not a validator protocol with its
            own lifecycle.
          </div>
        </div>
      </div>
    </Section>
  )
}

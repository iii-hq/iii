import { Section } from '@/components/Section'
import { ReadinessGraph } from '@/components/diagrams/ReadinessGraph'
import { FnChip } from '@/components/schematic/FnChip'
import { StatusPanel } from '@/components/schematic/StatusPanel'
import { READINESS_LEVELS, TOPO_PIPELINE } from '@/content/changes'

const SUBHEAD = 'font-mono text-[11px] uppercase tracking-[0.14em] text-ink-faint'

export function ReadinessSection() {
  return (
    <Section
      id="readiness"
      index="06"
      eyebrow="killing the function-not-found race"
      title="start-ordered, gated on readiness."
      lede="depends_on defaults to ready (l1) — process up + ws-connected + functions registered — strictly stronger than docker's started."
    >
      <div className="grid grid-cols-1 @4xl:grid-cols-2 gap-6 items-start">
        <div className="min-w-0">
          <ReadinessGraph />
        </div>

        <div className="min-w-0 border border-rule">
          <div className="bg-panel border-b border-rule px-4 py-2.5">
            <span className={SUBHEAD}>readiness contract</span>
          </div>
          {READINESS_LEVELS.map((row) => (
            <div
              key={row.level}
              className={
                'border-b border-rule-2 last:border-b-0 p-3.5' +
                (row.isDefault ? ' border-l-2 border-l-accent bg-panel' : '')
              }
            >
              <div className="flex items-baseline gap-x-2.5">
                <span className="font-mono text-[12px] text-ink-ghost tabular-nums">
                  {row.level}
                </span>
                <span className="font-semibold lowercase text-ink">{row.name}</span>
                {row.isDefault && (
                  <span className="font-mono text-[10px] uppercase tracking-[0.14em] text-accent">
                    DEFAULT
                  </span>
                )}
              </div>
              <p className="mt-2 text-[12px] text-ink-faint lowercase leading-relaxed">
                {row.met}
              </p>
              <div className="mt-3">
                <FnChip tone={row.isDefault ? 'accent' : 'faint'}>{row.condition}</FnChip>
              </div>
            </div>
          ))}
        </div>
      </div>

      <div className="mt-8">
        <span className={SUBHEAD}>what compose::up runs</span>
        <ol className="mt-3 border border-rule divide-y divide-rule-2">
          {TOPO_PIPELINE.map((step, i) => (
            <li key={i} className="flex gap-x-3 px-4 py-2.5">
              <span className="font-mono text-[12px] text-ink-ghost tabular-nums shrink-0">
                {String(i + 1).padStart(2, '0')}
              </span>
              <span className="text-ink-faint lowercase leading-relaxed">{step}</span>
            </li>
          ))}
        </ol>
      </div>

      <div className="mt-8">
        <StatusPanel
          variant="info"
          headline="implicit always-ready roots"
          detail="configuration, iii-process-daemon, and iii-worker-ops are valid depends_on targets and ready before any user worker. if configuration never readies, up hard-fails."
        />
      </div>
    </Section>
  )
}

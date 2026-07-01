import { Section } from '@/components/Section'
import { Cell } from '@/components/schematic/Cell'
import { FnChip } from '@/components/schematic/FnChip'
import { StatusDot } from '@/components/schematic/StatusDot'
import { WHY_BULLETS } from '@/content/changes'

const TANGLE = [
  'process management',
  'config.yaml',
  'iii-worker-manager',
  'iii-exec',
  'sandboxes',
  '~30 cli leaf-commands',
]

const SPINE = [
  'one worker-compose.yml',
  'engine — connection plane',
  'iii-worker-ops — the brain',
  'iii-process-daemon — the pid parent',
  'iii-sandbox',
  'configuration store',
]

const SUBHEAD = 'font-mono text-[11px] uppercase tracking-[0.14em] text-ink-faint'

export function WhySection() {
  return (
    <Section
      id="why"
      index="01"
      eyebrow="the problem"
      title="today everything is understood and operated separately."
      lede='orphans and zombies by design, a "function not found" startup race, misleading names, and inline config sprawl — six structural failures, one re-wiring.'
    >
      {/* tangle → spine */}
      <div className="grid grid-cols-1 @3xl:grid-cols-2 gap-px bg-rule border border-rule">
        <div className="bg-bg p-6">
          <div className="font-mono text-[16px] font-semibold lowercase text-ink mb-4">
            today — the tangle
          </div>
          <ul className="flex flex-col gap-y-2.5">
            {TANGLE.map((item) => (
              <li key={item} className="flex items-center gap-x-2.5">
                <StatusDot tone="ghost" />
                <span className="font-mono text-[13px] text-ink-faint">{item}</span>
              </li>
            ))}
          </ul>
        </div>
        <div className="bg-bg p-6">
          <div className="font-mono text-[16px] font-semibold lowercase text-ink mb-4">
            after — the spine
          </div>
          <ul className="flex flex-col gap-y-2.5">
            {SPINE.map((item) => (
              <li key={item} className="flex items-center gap-x-2.5">
                <StatusDot tone="accent" />
                <span className="font-mono text-[13px] text-ink">{item}</span>
              </li>
            ))}
          </ul>
        </div>
      </div>

      {/* six structural failures */}
      <div className="mt-10">
        <div className={SUBHEAD}>six structural failures</div>
        <div className="mt-4 grid grid-cols-1 @2xl:grid-cols-2 @4xl:grid-cols-3 gap-px bg-rule border border-rule">
          {WHY_BULLETS.map((b) => (
            <Cell key={b.title} title={b.title} className="border-0" bodyClassName="max-w-[40ch]">
              <p>{b.body}</p>
              <div className="mt-3">
                <FnChip tone="ghost">{b.cite}</FnChip>
              </div>
            </Cell>
          ))}
        </div>
      </div>
    </Section>
  )
}

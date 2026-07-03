import { PageShell } from '@/components/PageShell'
import { SpecRow, SpecSheet } from '@/components/SpecSheet'
import { CliPlayground } from '@/components/diagrams/CliPlayground'
import { FnChip } from '@/components/schematic/FnChip'
import { DAY2_OPS, GOLDEN_PATH, TRACKS, type PlayCommand } from '@/content/playground'

const SUBHEAD = 'font-mono text-[11px] uppercase tracking-[0.14em] text-ink-faint'

function ContractTable({ title, steps }: { title: string; steps: PlayCommand[] }) {
  return (
    <div>
      <div className={SUBHEAD}>{title}</div>
      <div className="mt-3 overflow-x-auto border border-rule">
        <div className="min-w-[680px] divide-y divide-rule-2">
          {steps.map((s) => (
            <div key={s.id} className="px-4 py-3">
              <FnChip tone="ink">{s.cmd}</FnChip>
              <div className="mt-1.5 flex items-start gap-x-1.5 font-mono text-[11.5px] leading-[1.5] text-ink-ghost">
                <span className="text-accent shrink-0">ƒ</span>
                <span>{s.fn}</span>
              </div>
            </div>
          ))}
        </div>
      </div>
    </div>
  )
}

const EXIT_CODES: Array<{ name: string; type: string; desc?: string }> = [
  { name: 'W100 InvalidName', type: 'exit 2' },
  { name: 'lock-drift (--frozen)', type: 'exit 3', desc: "preserves sync --frozen's contract — CI keeps failing on drift." },
  { name: 'W110 NotFound', type: 'exit 4' },
  { name: 'W104 ConsentRequired', type: 'exit 5' },
  { name: 'W120 LockBusy', type: 'exit 6' },
  { name: 'W161 StartTimeout', type: 'exit 7' },
  { name: 'Internal / IO / Registry', type: 'exit 1' },
]

export function PlaygroundPage() {
  return (
    <PageShell
      eyebrow="the playground"
      title="every command is an iii function."
      description="the cli is a thin wrapper: parse → typed options → invoke a backing function over the same ws transport iii trigger uses → render. no business logic in the cli."
    >
      <CliPlayground tracks={TRACKS} />

      <div className="flex flex-col gap-8">
        <ContractTable title="golden path — command → function → owner" steps={GOLDEN_PATH} />
        <ContractTable title="day-2 ops — command → function → owner" steps={DAY2_OPS} />
      </div>

      <SpecSheet title="canonical exit codes" meta="cli errors == function errors" defaultOpen>
        {EXIT_CODES.map((e) => (
          <SpecRow key={e.name} name={e.name} type={e.type}>
            {e.desc}
          </SpecRow>
        ))}
        <div className="mt-3 font-mono text-[12px] leading-[1.6] text-ink-faint lowercase">
          under --json, errors also emit {'{ error: { code, kind, message } }'}.
        </div>
      </SpecSheet>
    </PageShell>
  )
}

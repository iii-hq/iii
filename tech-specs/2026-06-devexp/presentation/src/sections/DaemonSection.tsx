import { Section } from '@/components/Section'
import { SpecSheet } from '@/components/SpecSheet'
import { FnChip } from '@/components/schematic/FnChip'
import { SpawnFunnel, SupervisionTiers } from '@/components/diagrams/DaemonModel'
import { ZOMBIE_ROWS } from '@/content/changes'

export function DaemonSection() {
  return (
    <Section
      id="daemon"
      index="05"
      eyebrow="zero zombies by construction"
      title="one long-lived parent, wait()-reaped, no setsid."
      lede="four detached spawn paths collapse into one reaping parent. the only way to make a zombie — detach and drop the handle — is removed everywhere."
    >
      <div className="grid gap-6 items-start @5xl:grid-cols-[minmax(0,1fr)_320px]">
        <SpawnFunnel />
        <div>
          <div className="font-mono text-[11px] uppercase tracking-[0.14em] text-ink-faint">
            the bottom turtle
          </div>
          <div className="mt-4">
            <SupervisionTiers />
          </div>
        </div>
      </div>

      <div className="mt-8">
        <SpecSheet
          title="zombie root-cause → mechanism"
          meta={`${ZOMBIE_ROWS.length} sources`}
          defaultOpen={false}
        >
          <div>
            {ZOMBIE_ROWS.map((row) => (
              <div
                key={row.cite}
                className="py-2 border-b border-rule-2 last:border-b-0"
              >
                <div className="flex flex-wrap items-baseline gap-x-2.5 gap-y-1">
                  <span className="font-mono text-[12.5px] leading-[1.55] text-ink">
                    {row.cause}
                  </span>
                  <FnChip tone="ghost">{row.cite}</FnChip>
                </div>
                <div className="mt-1 font-mono text-[12px] leading-[1.6] text-ink-faint">
                  → {row.mechanism}
                </div>
              </div>
            ))}
          </div>
        </SpecSheet>
      </div>
    </Section>
  )
}

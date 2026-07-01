import { Section } from '@/components/Section'
import { SpecRow, SpecSheet } from '@/components/SpecSheet'
import { Cell } from '@/components/schematic/Cell'
import { BEFORE_AFTER, MIGRATION_PHASES, REMOVED, RENAMES } from '@/content/changes'

const SUBHEAD = 'font-mono text-[11px] uppercase tracking-[0.14em] text-ink-faint'

export function RemovedSection() {
  return (
    <Section
      id="removed"
      index="09"
      eyebrow="the deletions"
      title="config.yaml, iii-worker-manager, iii-exec, the detached spawn paths."
      lede="what goes, why, and what replaces it — plus the canonical-names table that's fixed across every file."
    >
      {/* before → after */}
      <div className={SUBHEAD}>before → after</div>
      <div className="mt-4 overflow-x-auto border border-rule">
        <div className="min-w-[760px]">
          <div className="grid grid-cols-[150px_1fr_1fr] gap-px bg-rule">
            {['aspect', 'before', 'after'].map((h) => (
              <div
                key={h}
                className="bg-panel px-3 py-2 font-mono text-[10px] uppercase tracking-[0.1em] text-ink-faint"
              >
                {h}
              </div>
            ))}
            {BEFORE_AFTER.map((r) => (
              <div key={r.aspect} className="contents">
                <div className="bg-bg px-3 py-3">
                  <div className="font-mono text-[12.5px] font-semibold lowercase text-ink">
                    {r.aspect}
                  </div>
                  <div className="mt-1 font-mono text-[10.5px] leading-[1.5] text-ink-ghost lowercase">
                    {r.why}
                  </div>
                </div>
                <div className="bg-bg px-3 py-3 font-mono text-[12px] leading-[1.6] text-ink-faint lowercase">
                  {r.before}
                </div>
                <div className="bg-bg px-3 py-3 font-mono text-[12px] leading-[1.6] text-ink lowercase">
                  {r.after}
                </div>
              </div>
            ))}
          </div>
        </div>
      </div>

      {/* removed */}
      <div className="mt-10">
        <div className={SUBHEAD}>removed</div>
        <div className="mt-4 grid grid-cols-1 @2xl:grid-cols-2 @4xl:grid-cols-3 gap-px bg-rule border border-rule">
          {REMOVED.map((r) => (
            <Cell key={r.item} title={r.item} className="border-0" bodyClassName="max-w-[42ch]">
              <p>{r.why}</p>
              <div className="mt-3">
                <div className="font-mono text-[10px] uppercase tracking-[0.1em] text-ink-faint mb-1">
                  replacement
                </div>
                <div className="font-mono text-[12px] leading-[1.6] text-ink">→ {r.replacement}</div>
              </div>
            </Cell>
          ))}
        </div>
      </div>

      {/* canonical names */}
      <div className="mt-8">
        <SpecSheet title="canonical names" meta={`${RENAMES.length} fixed names`}>
          {RENAMES.map((r) => (
            <SpecRow key={r.concept} name={r.concept} type={r.name}>
              {r.note}
            </SpecRow>
          ))}
        </SpecSheet>
      </div>

      {/* six-phase migration */}
      <div className="mt-4">
        <SpecSheet title="six-phase migration" meta="config.yaml + compose coexist ≥3 releases">
          <div className="divide-y divide-rule-2">
            {MIGRATION_PHASES.map((p) => (
              <div key={p.n} className="flex flex-wrap items-baseline gap-x-3 gap-y-1 px-1 py-2">
                <span className="font-mono text-[11px] uppercase tracking-[0.06em] text-ink-ghost tabular-nums shrink-0">
                  phase {p.n}
                </span>
                <span className="font-mono text-[13px] font-semibold lowercase text-ink">
                  {p.title}
                </span>
                {p.shipped ? (
                  <span className="font-mono text-[9px] uppercase tracking-[0.1em] text-accent border border-accent px-1 py-0.5">
                    shipped in part
                  </span>
                ) : null}
                <span className="w-full font-mono text-[12px] leading-[1.6] text-ink-faint lowercase">
                  {p.body}
                </span>
              </div>
            ))}
          </div>
        </SpecSheet>
        <a
          href="#/migration"
          className="mt-6 inline-flex items-center border border-rule bg-bg px-3 py-2 font-mono text-[13px] lowercase text-ink hover:bg-panel transition-colors"
        >
          read the full migration plan →
        </a>
      </div>
    </Section>
  )
}

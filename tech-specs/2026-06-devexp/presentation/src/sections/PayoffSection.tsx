import { Section } from '@/components/Section';
import { Cell } from '@/components/schematic/Cell';
import { FnChip } from '@/components/schematic/FnChip';
import { StatusDot } from '@/components/schematic/StatusDot';
import { SCORECARD, VERBS, ERROR_GALLERY, PRINCIPLES } from '@/content/changes';

const SUBHEAD = 'font-mono text-[11px] uppercase tracking-[0.14em] text-ink-faint';

function outputTone(line: string): string {
  const t = line.trimStart();
  if (t.startsWith('✓')) return 'text-accent';
  if (t.startsWith('✗')) return 'text-alert';
  if (t.startsWith('!')) return 'text-warn';
  return 'text-ink-faint';
}

export function PayoffSection() {
  return (
    <Section
      id="payoff"
      index="10"
      eyebrow="the result"
      title="one file, one command, one terminal, zero zombies."
      lede="~5 commands across 2 terminals collapse to 4 in 1; the cli becomes a small verb surface; and every failure mode has a calm, scriptable recovery."
    >
      {/* 1) before/after scorecard */}
      <div className="overflow-x-auto border border-rule">
        <div className="min-w-[640px]">
          <div className="grid grid-cols-[1fr_1fr_1fr] gap-px bg-rule">
            <div className="bg-panel p-3 font-mono text-[11px] uppercase tracking-[0.14em] text-ink-faint">
              metric
            </div>
            <div className="bg-panel p-3 font-mono text-[11px] uppercase tracking-[0.14em] text-ink-faint">
              before
            </div>
            <div className="bg-panel p-3 font-mono text-[11px] uppercase tracking-[0.14em] text-ink-faint">
              after
            </div>
          </div>
          <div className="grid grid-cols-[1fr_1fr_1fr] gap-px bg-rule text-[12.5px]">
            {SCORECARD.map((row) => (
              <div key={row.metric} className="contents">
                <div className="bg-bg p-3 text-ink lowercase">{row.metric}</div>
                <div className="bg-bg p-3 text-ink-faint">{row.before}</div>
                <div className="bg-bg p-3 text-ink">
                  <span className="inline-flex items-center gap-2">
                    <StatusDot tone="accent" />
                    {row.after}
                  </span>
                </div>
              </div>
            ))}
          </div>
        </div>
      </div>

      {/* 2) consolidated verb surface */}
      <div className="mt-8">
        <div className={SUBHEAD}>the consolidated verb surface</div>
        <div className="mt-4 grid @2xl:grid-cols-2 @4xl:grid-cols-4 gap-px bg-rule border border-rule">
          {VERBS.map((v) => (
            <div key={v.verb} className="bg-bg p-4">
              <FnChip tone="ink">{v.verb}</FnChip>
              <div className="mt-2 text-[11.5px] text-ink-ghost">→ {v.fn}</div>
            </div>
          ))}
        </div>
      </div>

      {/* 3) error & recovery */}
      <div className="mt-8">
        <div className={SUBHEAD}>error &amp; recovery, by design</div>
        <div className="mt-4 grid @2xl:grid-cols-2 @3xl:grid-cols-3 gap-px bg-rule border border-rule">
          {ERROR_GALLERY.map((e) => (
            <div key={e.scenario} className="bg-bg p-4">
              <div className="font-semibold lowercase text-ink">{e.scenario}</div>
              <div className="text-[11px] text-ink-ghost lowercase">{e.trigger}</div>
              <div className="mt-2 border border-rule bg-panel p-2 font-mono text-[11px] leading-relaxed">
                {e.output.map((line, i) => (
                  <div key={i} className={`${outputTone(line)} whitespace-pre-wrap`}>
                    {line}
                  </div>
                ))}
              </div>
              <div className="mt-2 text-[12px] text-ink-faint lowercase">{e.resolution}</div>
            </div>
          ))}
        </div>
      </div>

      {/* 4) the principles */}
      <div className="mt-8">
        <div className={SUBHEAD}>the principles that hold it together</div>
        <div className="mt-4 grid @2xl:grid-cols-2 @4xl:grid-cols-3 gap-px bg-rule border border-rule">
          {PRINCIPLES.map((p) => (
            <Cell
              key={p.n}
              className="border-0"
              title={
                <span>
                  <span className="text-ink-ghost tabular-nums">{p.n}</span> {p.title}
                </span>
              }
            >
              <div className="text-[12.5px] text-ink-faint">{p.body}</div>
            </Cell>
          ))}
        </div>
      </div>
    </Section>
  );
}

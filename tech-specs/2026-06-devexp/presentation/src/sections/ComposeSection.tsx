import { Section } from '@/components/Section';
import { SpecSheet, SpecRow } from '@/components/SpecSheet';
import { CodeBlock, K, S, C } from '@/components/schematic/CodeBlock';
import { MERGE_CHAIN } from '@/content/changes';

const SUBHEAD = 'font-mono text-[11px] uppercase tracking-[0.14em] text-ink-faint';

export function ComposeSection() {
  return (
    <Section
      id="compose"
      index="07"
      eyebrow="desired state vs resolved state"
      title="the package.json / lockfile split."
      lede="compose is human-authored desired state; iii.lock is machine-written resolved state, keyed by (package, version). up replays the lock; update is the explicit re-pin."
    >
      {/* 1) compose yaml + merge chain */}
      <div className="grid @4xl:grid-cols-2 gap-6 items-start">
        {/* LEFT — the compose file at a glance */}
        <CodeBlock title="worker-compose.yml — at a glance">
          <K>version</K>: <S>"1"</S>{'\n'}
          <K>port</K>: 49134{'                                  '}<C># the ws gateway port (bootstrap)</C>{'\n'}
          configuration:{'                               '}<C># the config store's own location (bootstrap)</C>{'\n'}
          {'  '}adapter: fs{'\n'}
          {'  '}directory: ./data/configuration{'\n'}
          {'\n'}
          <K>workers</K>:{'\n'}
          {'  '}math-worker:{'                               '}<C># local source worker</C>{'\n'}
          {'    '}runtime: {'{ '}workspace: ./workers/math-worker {'}'}{'\n'}
          {'    '}scripts: {'{ '}install: npm install, start: npm run dev {'}'}{'\n'}
          {'\n'}
          {'  '}caller-worker:{'\n'}
          {'    '}runtime: {'{ '}workspace: ./workers/caller-worker {'}'}{'\n'}
          {'    '}depends_on: [math-worker]{'                '}<C># start-ordered, gated on readiness</C>{'\n'}
          {'    '}environment: {'{ '}LOG_LEVEL: debug {'}'}{'        '}<C># overrides iii.worker.yaml (config is NOT here)</C>{'\n'}
          {'\n'}
          {'  '}state:{'                                     '}<C># remote registry worker, pinned in iii.lock</C>{'\n'}
          {'    '}runtime: {'{ '}package: <S>workers.iii.dev/iii-state:latest</S> {'}'}{'\n'}
        </CodeBlock>

        {/* RIGHT — the 4-layer merge chain */}
        <div>
          <div className={SUBHEAD}>compose overrides the manifest (runtime / scripts / env), never the reverse</div>
          <div className="mt-4 space-y-0">
            {MERGE_CHAIN.map((band, i) => (
              <div key={band.n}>
                {i > 0 && (
                  <div className="py-1.5 text-center font-mono text-[11px] text-ink-ghost">
                    ↓ deep_merge per key; lists &amp; scalars replace
                  </div>
                )}
                <div
                  className={`border border-rule p-3.5 ${
                    band.wins ? 'border-l-2 border-l-accent bg-panel' : ''
                  }`}
                >
                  <div className="flex items-baseline gap-2">
                    <span className="font-mono text-[11px] tabular-nums text-ink-ghost">
                      {band.n}
                    </span>
                    <span className="font-semibold lowercase text-ink">{band.label}</span>
                  </div>
                  <div className="mt-1 text-[12px] text-ink-faint">{band.note}</div>
                </div>
              </div>
            ))}
          </div>
        </div>
      </div>

      {/* 2) compose vs iii.lock datasheet */}
      <div className="mt-8">
        <SpecSheet title="compose vs iii.lock" meta="two files, two jobs">
          <SpecRow name="worker-compose.yml" type="desired · human-authored">
            port + store location + worker topology + depends_on. the only file you edit.
          </SpecRow>
          <SpecRow name="iii.lock" type="resolved · machine-written">
            concrete versions + sha256, keyed by (package, version). replayed by up (npm ci semantics).
          </SpecRow>
          <SpecRow name="local workspace workers" type="not locked">
            source, not artifact — presence-locked, never pinned.
          </SpecRow>
          <SpecRow name="iii update" type="the re-pin">
            the explicit way to move a floating tag forward.
          </SpecRow>
        </SpecSheet>
      </div>

      {/* 3) schema reference link */}
      <a
        href="#/compose"
        className="inline-flex items-center border border-rule bg-bg px-3 py-2 font-mono text-[13px] lowercase text-ink hover:bg-panel transition-colors mt-6"
      >
        open the full schema reference →
      </a>
    </Section>
  );
}

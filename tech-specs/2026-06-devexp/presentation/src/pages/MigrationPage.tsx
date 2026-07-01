import { PageShell } from '@/components/PageShell'
import { SpecRow, SpecSheet } from '@/components/SpecSheet'
import { FnChip } from '@/components/schematic/FnChip'
import { StatusPanel } from '@/components/schematic/StatusPanel'
import { MIGRATION_PHASES } from '@/content/changes'

const SUBHEAD = 'font-mono text-[11px] uppercase tracking-[0.14em] text-ink-faint'

const DEST: Array<[string, string, string]> = [
  ['iii-worker-manager', 'port, host, rbac, middleware', 'compose + DELETE'],
  ['configuration', 'adapter{fs,directory}, ttl', 'compose'],
  ['iii-observability', 'exporter, endpoint, logs', 'store (restart-tier)'],
  ['iii-state', 'adapter{kv,file_path}', 'store (done)'],
  ['iii-stream', 'port 3112, adapter', 'store (data-plane)'],
  ['iii-http', 'port 3111, cors, timeout', 'store'],
  ['iii-queue / iii-pubsub / iii-cron', 'per-worker adapters', 'store'],
  ['iii-exec', 'watch, exec:[cmd]', 'DELETE + daemon'],
  ['modules: key', 'list of built-ins', 'DELETE (auto-injected)'],
]

export function MigrationPage() {
  return (
    <PageShell
      eyebrow="the migration"
      title="production migrates without an outage."
      description="config.yaml and worker-compose.yml coexist for ≥3 releases behind a format-detection bridge; the end state is reached in six phases, cloud cutover last."
    >
      {/* six phases */}
      <div>
        <div className={SUBHEAD}>six phases</div>
        <div className="mt-4 border border-rule divide-y divide-rule-2">
          {MIGRATION_PHASES.map((p) => (
            <div key={p.n} className="flex flex-wrap items-baseline gap-x-3 gap-y-1 px-4 py-3.5">
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
              {p.n === '3' ? (
                <span className="font-mono text-[9px] uppercase tracking-[0.1em] text-warn border border-warn px-1 py-0.5">
                  the risky middle
                </span>
              ) : null}
              <span className="w-full font-mono text-[12px] leading-[1.6] text-ink-faint lowercase">
                {p.body}
              </span>
            </div>
          ))}
        </div>
      </div>

      <StatusPanel
        variant="success"
        headline="phase 2 is already shipped in part"
        detail="all seven built-in workers self-register their schema + initial value at boot via configuration::register and read the configuration store — there is no seed and no config: block to migrate. configuration is owned end-to-end by the configuration worker."
      />

      {/* coexistence bridge */}
      <div>
        <div className={SUBHEAD}>the coexistence bridge</div>
        <div className="mt-4">
          <SpecSheet title="how the two formats coexist" meta="format detection" defaultOpen>
            <SpecRow name="dual-parser">
              compose is lowered to today's EngineConfig — no behavior change (Phase 0).
            </SpecRow>
            <SpecRow name="--compose / --config">
              the engine accepts either file during coexistence; --config still reads config.yaml.
            </SpecRow>
            <SpecRow name="iii migrate" type="migrate::config_yaml">
              one-shot: config.yaml → worker-compose.yml, emitting runtime / topology only; config is not touched (workers re-register defaults at boot, operators re-apply tuned values with iii worker config set); renames config.yaml → .bak.
            </SpecRow>
            <SpecRow name="--frozen drift gate" type="exit 3">
              CI keeps failing on lock drift through the whole transition.
            </SpecRow>
          </SpecSheet>
        </div>
      </div>

      {/* destination mapping */}
      <div>
        <div className={SUBHEAD}>config.yaml entry → destination</div>
        <div className="mt-4 overflow-x-auto border border-rule">
          <div className="min-w-[640px]">
            <div className="grid grid-cols-[180px_1fr_140px] gap-px bg-rule">
              {['entry', 'what it holds', 'destination'].map((h) => (
                <div
                  key={h}
                  className="bg-panel px-2.5 py-2 font-mono text-[10px] uppercase tracking-[0.1em] text-ink-faint"
                >
                  {h}
                </div>
              ))}
              {DEST.map(([entry, holds, dest]) => (
                <div key={entry} className="contents">
                  <div className="bg-bg px-2.5 py-2.5">
                    <FnChip tone="faint">{entry}</FnChip>
                  </div>
                  <div className="bg-bg px-2.5 py-2.5 font-mono text-[12px] leading-[1.5] text-ink-faint lowercase">
                    {holds}
                  </div>
                  <div className="bg-bg px-2.5 py-2.5 font-mono text-[12px] leading-[1.5] text-ink lowercase">
                    {dest}
                  </div>
                </div>
              ))}
            </div>
          </div>
        </div>
      </div>

      <StatusPanel
        variant="warn"
        headline="the cloud cutover is last"
        detail="multi-host orchestration stays iii cloud's job; the process-daemon is a single-host model. the cloud canary in phase 4 gates the gateway bake-in."
      />
    </PageShell>
  )
}

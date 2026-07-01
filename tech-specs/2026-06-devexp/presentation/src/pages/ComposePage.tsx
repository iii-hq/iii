import { PageShell } from '@/components/PageShell';
import { SpecSheet, SpecRow } from '@/components/SpecSheet';
import { CodeBlock, S, C } from '@/components/schematic/CodeBlock';

const SUBHEAD = 'font-mono text-[11px] uppercase tracking-[0.14em] text-ink-faint';

export function ComposePage() {
  return (
    <PageShell
      eyebrow="worker-compose.yml"
      title="the one file a human edits."
      description="the boot egg: the ws gateway port, the configuration-store location, and the worker list + topology. everything else resolves into iii.lock or the configuration store."
    >
      {/* 1) the canonical shape */}
      <CodeBlock title="the canonical shape">
        version: <S>"1"</S>{'\n'}
        port: 49134{'                                  '}<C># the ws gateway port (bootstrap)</C>{'\n'}
        configuration:{'                               '}<C># the config store's own location (bootstrap)</C>{'\n'}
        {'  '}adapter: fs{'\n'}
        {'  '}directory: ./data/configuration{'\n'}
        {'\n'}
        workers:{'\n'}
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

      {/* 2) per-worker fields datasheet */}
      <div className="mt-10">
        <div className={SUBHEAD}>the per-worker block — field by field</div>
        <div className="mt-4">
          <SpecSheet
            title="per-worker fields"
            meta="merge: maps deep-merge; lists & scalars replace"
            defaultOpen={true}
          >
            <SpecRow name="runtime.workspace" type="path · replace · NEW">
              local source worker; was config.yaml worker_path:.
            </SpecRow>
            <SpecRow name="runtime.package" type="string · replace · NEW">
              remote registry worker; was config.yaml image:. mutually exclusive with workspace.
            </SpecRow>
            <SpecRow name="runtime.base_image" type="string · replace">
              existing.
            </SpecRow>
            <SpecRow name="runtime.cpus" type="uint · default 2 · cap 4">
              existing resources.cpus.
            </SpecRow>
            <SpecRow name="runtime.memory" type="uint MiB · default 2048 · cap 4096">
              existing resources.memory.
            </SpecRow>
            <SpecRow name="runtime.egress" type="bool · default false">
              RENAMED from runtime.network — vm outbound internet, not inter-worker networking.
            </SpecRow>
            <SpecRow name="scripts.setup / install / start" type="string · deep-merge">
              install: "" opts out.
            </SpecRow>
            <SpecRow name="environment" type="map · deep-merge">
              RENAMED from env.
            </SpecRow>
            <SpecRow name="env_file" type="list<path> · replace · NEW">
              loaded by the daemon at launch; later-listed file wins.
            </SpecRow>
            <SpecRow name="depends_on" type="list · replace · NEW">
              start-ordering; distinct from manifest dependencies.
            </SpecRow>
            <SpecRow name="(no config field)" type="lives in the configuration worker">
              per-worker configuration is not a compose field — each worker registers its own schema + initial value at boot via configuration::register; tune live values with iii worker config set.
            </SpecRow>
            <SpecRow name="healthcheck" type="object · NEW">
              l2 readiness probe.
            </SpecRow>
          </SpecSheet>
        </div>
      </div>

      {/* 3) two copies of one worker */}
      <div className="mt-10">
        <div className={SUBHEAD}>two copies of one worker</div>
        <div className="mt-4">
          <CodeBlock title="instance id is decoupled from package name">
            workers:{'\n'}
            {'  '}scraper-us:{'\n'}
            {'    '}runtime: {'{ '}package: <S>workers.iii.dev/scraper:1.4.2</S> {'}'}{'\n'}
            {'    '}environment: {'{ '}REGION: us-east-1 {'}'}{'\n'}
            {'  '}scraper-eu:{'\n'}
            {'    '}runtime: {'{ '}package: <S>workers.iii.dev/scraper:1.4.2</S> {'}'}{'\n'}
            {'    '}environment: {'{ '}REGION: eu-west-1 {'}'}{'\n'}
          </CodeBlock>
        </div>
        <ul className="mt-4 space-y-1.5 text-[12px] lowercase text-ink-faint">
          <li>the map key &lt;id&gt; is the instance id, decoupled from package name scraper — two ids, one package.</li>
          <li>the artifact scraper@1.4.2 downloads once (machine-global) and is shared.</li>
          <li>iii.lock is keyed by (package, version) — so the two copies can even diverge in version.</li>
        </ul>
      </div>

      {/* 4) validation error catalog */}
      <div className="mt-10">
        <SpecSheet title="validation error catalog" meta="fail before anything starts">
          <SpecRow name="C020 UnknownDependency">
            depends_on names a worker that isn't declared (+ did-you-mean).
          </SpecRow>
          <SpecRow name="C021 DependencyCycle">
            lists the cycle.
          </SpecRow>
          <SpecRow name="C022 SelfDependency" />
          <SpecRow name="C060 LockDrift" type="exit 3">
            compose changed but iii.lock is stale (--frozen).
          </SpecRow>
        </SpecSheet>
      </div>
    </PageShell>
  );
}

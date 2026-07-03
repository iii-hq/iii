import { Section } from '@/components/Section'
import { Cell } from '@/components/schematic/Cell'
import { StatusPanel } from '@/components/schematic/StatusPanel'
import { FnChip } from '@/components/schematic/FnChip'
import { BOOT_SEQUENCE } from '@/content/changes'

const subHead = 'font-mono text-[11px] uppercase tracking-[0.14em] text-ink-faint'

const SELF_REGISTER = [
  {
    n: '1',
    label: 'register at boot',
    note: 'each worker calls configuration::register(id, schema, initial_value) from its own code/sdk. nothing in compose, nothing in iii.worker.yaml.',
  },
  {
    n: '2',
    label: 'the store persists the value',
    note: 'the configuration worker owns the entry config-worker:<id> (entry id == worker id) and writes it to ./data/configuration.',
  },
  {
    n: '3',
    label: 'read + hot-reload',
    note: 'the worker reads via configuration::get and hot-reloads off the configuration trigger — no restart.',
  },
]

export function ConfigSection() {
  return (
    <Section
      id="config"
      index="08"
      eyebrow="config has one home"
      title="per-worker config lives end-to-end in the configuration worker."
      lede="there is no config in iii.worker.yaml and none in worker-compose.yml. each worker registers its own schema + initial value at boot; the store persists it; runtime changes go through iii worker config set. config-worker:<id> is only the addressing scheme, resolved inside the configuration worker."
    >
      <div className={subHead}>the three-fact bootstrap floor</div>
      <div className="mt-4 grid @3xl:grid-cols-3 gap-px bg-rule border border-rule">
        <Cell
          className="border-0"
          title="B1 · the ws gateway port"
        >
          the single tcp port sdk workers connect to (default 49134). you cannot
          read the port from the thing the port lets you reach.
        </Cell>
        <Cell
          className="border-0"
          title="B2 · the store location"
        >
          where the fs adapter persists (./data/configuration), resolved relative
          to the compose-file dir. the store cannot read its own location from itself.
        </Cell>
        <Cell
          className="border-0"
          title="B3 · restart-tier boot-read"
        >
          config consumed before any worker exists (e.g. logging) is read straight
          off disk with ${'{VAR}'} expansion. depends on B2.
        </Cell>
      </div>

      <div className="mt-8 grid @4xl:grid-cols-[minmax(0,1fr)_300px] gap-6 items-start">
        <div className="border border-rule">
          <div className="bg-panel px-4 py-2 border-b border-rule">
            <span className={subHead}>engine boot — seven steps</span>
          </div>
          <div className="divide-y divide-rule-2">
            {BOOT_SEQUENCE.map((step) => (
              <div key={step.n} className="flex gap-4 px-4 py-3">
                <span className="tabular-nums text-[12px] text-ink-faint pt-px shrink-0">
                  {String(step.n).padStart(2, '0')}
                </span>
                <div className="min-w-0">
                  <div className="font-semibold lowercase text-ink">{step.title}</div>
                  <div className="mt-1 text-[12px] text-ink-faint lowercase">{step.body}</div>
                </div>
              </div>
            ))}
          </div>
        </div>

        <div>
          <div className={subHead}>config is self-registered — no merge</div>
          <div className="mt-4 flex flex-col gap-2">
            {SELF_REGISTER.map((band) => (
              <div key={band.n} className="border border-rule p-3">
                <div className="flex gap-2 items-baseline">
                  <span className="tabular-nums text-[12px] text-ink-faint shrink-0">{band.n}</span>
                  <span className="text-[13px] text-ink lowercase">{band.label}</span>
                </div>
                <div className="mt-1 pl-5 text-[12px] text-ink-faint lowercase">{band.note}</div>
              </div>
            ))}
          </div>
          <div className="mt-4 flex flex-wrap gap-1.5">
            {['register', 'set', 'get', 'list', 'schema'].map((f) => (
              <FnChip key={f} tone="faint">{`configuration::${f}`}</FnChip>
            ))}
          </div>
        </div>
      </div>

      <div className="mt-8">
        <StatusPanel
          variant="info"
          headline="one source of truth — no seed, no pointer"
          detail="configuration is not a merged field: it never appears in worker-compose.yml or iii.worker.yaml. workers own their defaults via configuration::register; operators tune live values with iii worker config set (→ configuration::set), validated against the schema the worker registered and hot-reloaded with no restart."
        />
      </div>
    </Section>
  )
}

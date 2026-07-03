/**
 * playground.ts — drives the simulated terminal in section 02 and on the
 * /playground deep-dive. Each command carries its exact form, the backing
 * function + owner, and verbatim multi-line output. Condensed from
 * cli-and-functions.md + lifecycle-and-onboarding.md.
 */

export interface PlayCommand {
  id: string
  cmd: string
  /** backing function + owner — the "everything is an iii function" contract */
  fn: string
  output: string[]
  /** optional one-line note rendered under the output */
  note?: string
  /** an exit code worth surfacing (drift gate, errors) */
  exit?: number
}

export interface PlayTrack {
  id: string
  label: string
  blurb: string
  steps: PlayCommand[]
}

export const GOLDEN_PATH: PlayCommand[] = [
  {
    id: 'install',
    cmd: 'curl -fsSL https://install.iii.dev/iii/main/install.sh | sh',
    fn: 'installer — out of band (no iii function)',
    output: [
      'downloading iii (main)…',
      '  resolved: iii 0.12.0 (darwin-arm64)',
      '  installed: /usr/local/bin/iii',
      '',
      '✓ iii 0.12.0 installed. run `iii init` to scaffold a project.',
    ],
  },
  {
    id: 'init',
    cmd: 'iii init quickstart',
    fn: 'scaffolder-core::TemplateFetcher (iii-worker-ops) — replaces iii project init / iii worker init',
    output: [
      'scaffolding quickstart (template: quickstart) …',
      '  ✓ worker-compose.yml',
      '  ✓ .gitignore',
      '  ✓ workers/math-worker/iii.worker.yaml',
      '  ✓ workers/math-worker/math_worker.py',
      '  ✓ workers/caller-worker/iii.worker.yaml',
      '  ✓ workers/caller-worker/src/worker.ts',
      '',
      '✓ created quickstart/  ·  cd quickstart && iii up',
    ],
  },
  {
    id: 'up',
    cmd: 'iii up',
    fn: 'compose::up on iii-worker-ops (streaming alias of `iii worker compose up`) → engine binds worker-gateway from compose.port → process::start per node on iii-process-daemon, gated on status::watch_until_ready',
    output: [
      '→ no engine on :49134, cold-starting iii',
      '  engine ready: worker-gateway bound on 0.0.0.0:49134',
      '  configuration store: ready',
      '  iii-worker-ops: ready (builtin)',
      '  iii-process-daemon: ready (pid 41201)',
      'compose::up: parsing worker-compose.yml',
      '  topo-sort: [configuration] → [math-worker] → [caller-worker]',
      '  resolve  math-worker    workspace ./workers/math-worker (presence-lock)',
      '  resolve  caller-worker  workspace ./workers/caller-worker',
      '  resolve  state          workers.iii.dev/iii-state:latest → 1.2.3 (replayed from iii.lock)',
      '  start    math-worker    pid 41310  instance_token 7f3c… managed  ✓ ready',
      '  start    caller-worker  pid 41311  instance_token a91e… managed  (depends_on: math-worker) ✓ ready',
      '  start    state          pid 41312  instance_token c402… managed  ✓ ready',
      'all workers READY (3 managed, 0 independent) on port 49134',
      'attached — Ctrl-C to compose::down',
    ],
    note: 'depends_on gates each start on L1 readiness — process up + ws-connected + functions registered. no more "function not found" race.',
  },
  {
    id: 'ps',
    cmd: 'iii ps',
    fn: 'process::ps on iii-process-daemon (top-level alias)',
    output: [
      'ID              SOURCE   STATE    PID     UPTIME   RESTARTS   FNS',
      'math-worker     local    ready    41310   2m14s    0          3',
      'caller-worker   local    ready    41311   2m10s    0          1',
      'state           remote   ready    41312   2m08s    0          5',
    ],
  },
  {
    id: 'logs',
    cmd: 'iii logs -f',
    fn: 'process::logs on iii-process-daemon (streaming; reads the per-process ring buffer)',
    output: [
      'math-worker   | listening, health::check ok',
      'caller-worker | connected to engine on 49134',
      'state         | store opened',
      'math-worker   | POST math::add_two_numbers 200 3ms',
    ],
  },
  {
    id: 'trigger',
    cmd: 'iii trigger math::add_two_numbers a=10 b=20',
    fn: 'invocation routing on the engine gateway (the same WS transport every CLI command uses)',
    output: ['{ "c": 30 }'],
    note: 'the CLI is a thin wrapper over functions — `iii trigger` and every other verb ride the same gateway.',
  },
]

export const DAY2_OPS: PlayCommand[] = [
  {
    id: 'worker-add',
    cmd: 'iii worker add workers.iii.dev/iii-cache:2.1.0',
    fn: 'worker::add on iii-worker-ops (sources: Vec<WorkerSource>) — edits worker-compose.yml + iii.lock; does NOT auto-start',
    output: [
      '→ resolving iii-cache:2.1.0 … 2.1.0 (sha256:9f3a…)',
      '→ wrote workers.cache to worker-compose.yml',
      '→ wrote iii-cache@2.1.0 to iii.lock',
      'not started. run `iii up` (or `iii worker add … --up`) to start.',
    ],
    note: 'add is declarative-only now — one source of truth (compose + lock), one reconcile path on up.',
  },
  {
    id: 'config-set',
    cmd: 'iii worker config --set caller-worker LOG_LEVEL=trace',
    fn: 'configuration::set on the configuration worker (entry config-worker:caller-worker)',
    output: [
      '✓ caller-worker  LOG_LEVEL = trace   (live)',
      '  validated against the schema the worker registered · hot-reloaded, no restart',
    ],
  },
  {
    id: 'down',
    cmd: 'iii down',
    fn: 'compose::down on iii-worker-ops → process::stop on iii-process-daemon (reverse topo order)',
    output: [
      '→ reverse topo order: [caller-worker] → [math-worker] → [state]',
      '  caller-worker  ✓ stopped (pid 41311 reaped)',
      '  math-worker    ✓ stopped (pid 41310 reaped)',
      '  state          ✓ stopped (pid 41312 reaped)',
      '✓ down — 3 workers stopped (port 49134)',
    ],
    note: 'every pid is wait()-reaped by the one parent. nothing is left detached.',
  },
  {
    id: 'migrate',
    cmd: 'iii migrate --in config.yaml --out worker-compose.yml',
    fn: 'migrate::config_yaml on the migrate one-shot tool',
    output: [
      'read config.yaml (EngineConfig, 4 workers)',
      'wrote worker-compose.yml (port 49134, 4 workers) — runtime / topology only',
      '  iii-exec block "cron" → scripts.start',
      '  skipped: iii-worker-manager (deleted), iii-worker-ops (auto-injected)',
      '  config: not emitted — workers re-register defaults at boot; re-apply tuned values with iii worker config set',
      'renamed config.yaml → config.yaml.bak',
      'review worker-compose.yml, then: iii up',
    ],
  },
  {
    id: 'frozen',
    cmd: 'iii worker compose up --frozen',
    fn: 'compose::up --frozen on iii-worker-ops (CI lock-drift gate; preserves sync --frozen’s contract)',
    output: [
      '✗ C060 LockDrift: worker-compose.yml changed but iii.lock is stale.',
      '  run `iii up` (without --frozen) to re-resolve, then commit iii.lock.',
    ],
    exit: 3,
    note: 'lock-drift keeps its dedicated exit code (3) so CI keeps failing on drift.',
  },
]

export const TRACKS: PlayTrack[] = [
  {
    id: 'golden',
    label: 'golden path',
    blurb: 'curl|sh → iii init → iii up → iii trigger, in one terminal',
    steps: GOLDEN_PATH,
  },
  {
    id: 'day2',
    label: 'day-2 ops',
    blurb: 'add a worker, change config live, tear down, migrate, gate CI',
    steps: DAY2_OPS,
  },
]

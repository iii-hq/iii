/**
 * changes.ts — the content bank for the home-page sections. Tables, lists, and
 * copy condensed from README.md, process-daemon.md, configuration-and-bootstrap.md,
 * worker-compose.md, lifecycle-and-onboarding.md, and migration.md. Every exact
 * identifier and number is preserved from the spec.
 */

export const HERO_THESIS =
  'today’s iii is a tangle — process management, config.yaml, iii-worker-manager, iii-exec, sandboxes, and ~30 cli leaf-commands, all understood and operated separately. this overhaul re-wires that sound machinery into four clean planes that touch in exactly three places, behind one declarative worker-compose.yml and a golden path that fits in a single terminal.'

export const GOLDEN_CHIPS = ['curl | sh', 'iii init', 'iii up', 'iii trigger'] as const

export const STATS = [
  { value: '1', label: 'file a human edits' },
  { value: '4', label: 'clean planes' },
  { value: '3', label: 'meeting points' },
  { value: '0', label: 'zombies, by construction' },
] as const

/** the structural failures of today — section 01 */
export const WHY_BULLETS = [
  {
    title: 'orphans & zombies by design',
    body: 'four detached spawn paths setsid()-detach and drop the child handle. the only way to make a zombie is built in, in four places.',
    cite: 'process-daemon.md',
  },
  {
    title: 'a "function not found" startup race',
    body: 'nothing waits for a dependency to be ready before calling it. there is no readiness gate, so callers race a worker that hasn’t registered yet.',
    cite: 'lifecycle-and-onboarding.md',
  },
  {
    title: 'killing a pid drops in-flight calls',
    body: 'on disconnect the engine halt_invocations every in-flight call to a worker, so a naive restart loses requests.',
    cite: 'engine/mod.rs:1700-1706',
  },
  {
    title: 'misleading names',
    body: 'iii-worker-manager never managed workers — it only opened a port. names that lie make the system harder to reason about.',
    cite: 'README.md',
  },
  {
    title: 'config sprawls inline in files',
    body: 'per-worker config lives as opaque blobs in config.yaml — no schema validation, no hot-reload, no audit; restart-only.',
    cite: 'configuration-and-bootstrap.md',
  },
  {
    title: 'the engine does too much',
    body: 'the connection plane is conflated with cmd.spawn — the engine binds sockets, routes invocations, AND owns os processes all at once.',
    cite: 'README.md',
  },
] as const

/** the design principles — section 01 / 10 */
export const PRINCIPLES = [
  {
    n: '01',
    title: 'one file is the boot egg',
    body: 'worker-compose.yml is the only file a human edits — port, store location, worker topology, and nothing else.',
  },
  {
    n: '02',
    title: 'desired vs resolved state',
    body: 'compose is human-authored desired state; iii.lock is machine-written resolved state. the package.json / lockfile split, never folded.',
  },
  {
    n: '03',
    title: 'override, never the reverse',
    body: 'a worker ships iii.worker.yaml defaults for runtime / scripts / environment; compose overrides them field-by-field. maps deep-merge; lists & scalars replace. per-worker configuration is not in this chain.',
  },
  {
    n: '04',
    title: 'config has one home',
    body: 'per-worker config lives end-to-end in the configuration worker — each worker registers its own schema + initial value at boot. compose carries no seed and no pointer.',
  },
  {
    n: '05',
    title: 'the engine never spawns',
    body: 'it is a pure connection / protocol / rbac / registry / routing plane. every pid is a child of one reaping iii-process-daemon.',
  },
  {
    n: '06',
    title: 'the cli is a thin wrapper',
    body: 'each command parses, invokes a backing function over the same ws transport iii trigger uses, and renders. no business logic in the cli.',
  },
  {
    n: '07',
    title: 'readiness is authorable',
    body: 'depends_on defaults to ready (l1) — process up + ws-connected + functions registered — strictly stronger than docker’s started.',
  },
] as const

/** the four claim cells — hero / section 01 */
export const CLAIMS = [
  {
    title: 'zero zombies by construction',
    body: 'four detached spawn paths collapse into one parent that wait()-reaps every child. orphans and zombies become impossible, not unlikely.',
  },
  {
    title: '~5 commands / 2 terminals → 4 / 1',
    body: 'the golden path is curl|sh → iii init → iii up → iii trigger, with depends_on gating that ends the startup race.',
  },
  {
    title: 'mostly a re-wiring, not a rewrite',
    body: 'sound machinery, re-homed across four planes — with one genuinely net-new build: the supervisor substrate in iii-process-daemon.',
  },
  {
    title: 'production migrates without an outage',
    body: 'config.yaml and worker-compose.yml coexist for ≥3 releases behind a format-detection bridge; the cloud cutover is last.',
  },
] as const

export interface BeforeAfterRow {
  aspect: string
  before: string
  after: string
  why: string
}

export const BEFORE_AFTER: BeforeAfterRow[] = [
  {
    aspect: 'process management',
    before:
      'the engine itself spawns os processes via cmd.spawn; four detached spawn paths can orphan / zombie children.',
    after:
      'the engine NEVER spawns. every pid is a direct child of one long-lived iii-process-daemon that wait()-reaps it.',
    why: 'orphans and zombies become impossible by construction; process ownership decoupled from engine hot-reload.',
  },
  {
    aspect: 'config.yaml',
    before:
      'a worker list + per-worker opaque config: seed blobs; hand-edited; restart-only.',
    after:
      'worker-compose.yml (port + store location + worker topology) + the configuration store (schema-validated, hot-reloadable, config-worker:<id>).',
    why: 'one declarative boot egg; runtime config gets validation, hot-reload, and audit.',
  },
  {
    aspect: 'the port / gateway',
    before:
      'a worker entry named iii-worker-manager opened the port (but never managed workers); port scanned out of WorkerManagerConfig.port.',
    after:
      'worker-gateway — an internal engine concept, no longer a worker. the engine binds the port from top-level compose.port (default 49134).',
    why: 'the name was misleading and the port is bootstrap floor, not a worker; removes an indirection layer.',
  },
  {
    aspect: 'spawn / reaping',
    before:
      'setsid()-detach + return 0 + pidfile handoff; liveness via kill(pid,0) polling + ps / proc scans against a recycle-prone pid.',
    after:
      'spawn_owned: setpgid(0,0), keep the Child forever, Stdio::piped, instance_token injected; stop_owned: killpg SIGTERM → grace → SIGKILL → wait().',
    why: 'detach-and-drop is the only way to make an orphan / zombie; deleting it everywhere + killpg-always makes both impossible.',
  },
  {
    aspect: 'cli surface',
    before:
      '~30 leaf-commands with business logic in the cli; status is always a tui; add auto-starts.',
    after:
      'a small verb set; each command is parse → typed options → invoke backing function over ws → render; iii up / down / ps / logs; everything is --json-able.',
    why: 'the cli is a thin wrapper over functions; one source of truth (compose + lock), one reconcile path.',
  },
  {
    aspect: 'config source',
    before:
      'per-worker config: blobs inline in config.yaml; static; resolved relative to the engine process cwd.',
    after:
      'configuration lives end-to-end in the configuration worker (each worker registers its schema + initial value at boot); the store directory resolves relative to the compose file’s directory.',
    why: 'config in the store, not files; project isolation keyed by port / compose dir.',
  },
]

export interface RemovedRow {
  item: string
  why: string
  replacement: string
}

export const REMOVED: RemovedRow[] = [
  {
    item: 'iii-exec (engine builtin)',
    why: 'arbitrary-process capability now belongs to the reaping parent; it registered zero functions and captured no logs (Stdio::inherit).',
    replacement: 'process::start{spec, watch} on iii-process-daemon; for cloud, a compose worker with scripts.start.',
  },
  {
    item: 'iii-worker-manager (worker) + its config entry',
    why: 'it never managed workers; it only opened a port.',
    replacement: 'worker-gateway — an internal engine concept; the port binds from compose.port.',
  },
  {
    item: 'config.yaml',
    why: 'replaced by one declarative, human-authored boot file.',
    replacement: 'worker-compose.yml + the configuration store; coexists ≥3 releases, removed in Phase 5.',
  },
  {
    item: 'the engine’s cmd.spawn paths',
    why: 'the four detached spawn paths are the zombie sources; the engine must be a pure connection plane.',
    replacement: 'one reaping parent — iii-process-daemon (spawn_owned / stop_owned).',
  },
  {
    item: 'pidfile machinery (vm.pid / watch.pid / <name>.pid)',
    why: 'per-worker cross-process disk handoff can deadlock on a stale lock; recycle-prone.',
    replacement: 'one atomic state.json snapshot under ~/.iii/daemon/<port>/.',
  },
  {
    item: 'runtime.network field',
    why: 'misleading — it is VM-egress, not inter-worker networking.',
    replacement: 'runtime.egress: bool.',
  },
  {
    item: 'docker-style networks: / service IPs',
    why: 'workers communicate only via functions over the engine bus; out of scope.',
    replacement: 'the function bus only; runtime.egress controls a sandbox VM’s outbound internet, nothing more.',
  },
  {
    item: 'iii worker add auto-start',
    why: 'it created two sources of truth.',
    replacement: 'declarative-only; --up reconciles immediately.',
  },
  {
    item: 'status-as-tui default',
    why: 'not scriptable.',
    replacement: 'iii ps / iii worker info data dumps; the live tui moves behind --watch.',
  },
]

export interface RenameRow {
  concept: string
  name: string
  note: string
}

export const RENAMES: RenameRow[] = [
  { concept: 'config-store worker', name: 'configuration', note: 'a config-worker worker does NOT exist.' },
  { concept: 'config addressing scheme', name: 'config-worker:<id>', note: 'entry id == worker id; resolved inside the configuration worker. not a compose field.' },
  { concept: 'pid-owning daemon', name: 'iii-process-daemon / process::*', note: 'reject iii-daemon / daemon::*.' },
  { concept: 'lifecycle / catalog brain', name: 'iii-worker-ops', note: 'namespaces worker::* + compose::*.' },
  { concept: 'ws listener / port', name: 'worker-gateway', note: 'internal engine concept; was the misnamed iii-worker-manager.' },
  { concept: 'arbitrary-process capability', name: 'process::start{spec, watch}', note: 'the iii-exec engine builtin is deleted.' },
  { concept: 'boot file', name: 'worker-compose.yml', note: 'replaces config.yaml’s worker list + the port indirection.' },
  { concept: 'resolved lockfile', name: 'iii.lock', note: 'keyed by (package, version).' },
  { concept: 'per-worker manifest', name: 'iii.worker.yaml', note: 'permissive; compose overrides its non-config fields field-by-field. carries no config.' },
  { concept: 'per-spawn key', name: 'instance_token', note: 'III_INSTANCE_TOKEN, plus III_COMPOSE_ID, IIIWORKER_PORT.' },
  { concept: 'top-level cli aliases', name: 'iii up / down / ps / logs', note: 'aliases for iii worker compose …; same backing functions.' },
  { concept: 'VM egress field', name: 'runtime.egress: bool', note: 'renamed from runtime.network.' },
  { concept: 'environment field', name: 'environment', note: 'renamed from env (deep-merged per key).' },
]

/** the zombie root-cause → mechanism table — section 05 */
export interface ZombieRow {
  cause: string
  cite: string
  mechanism: string
}

export const ZOMBIE_ROWS: ZombieRow[] = [
  {
    cause: 'engine holds the WRONG child handle — spawns a transient middle process, keeps its Child, never wait()s it → a true zombie parented to the live engine.',
    cite: 'registry_worker.rs:302-307',
    mechanism: 'delete path E; iii-worker-ops calls process::start over ws — no middle process; the daemon spawns the real worker as its own grouped child and wait()s it.',
  },
  {
    cause: 'detached real workers orphaned by design (setsid() + launcher returns 0).',
    cite: 'managed.rs:3431-3438',
    mechanism: 'stop detaching; spawn_owned uses setpgid(0,0) and keeps the Child → automatic wait() reaping; orphans impossible.',
  },
  {
    cause: 'stale pidfile → pid recycling → SIGKILL an innocent recycled pid (TOCTOU).',
    cite: 'managed.rs:2865-2876',
    mechanism: 'no pidfiles; identity is the in-memory table entry; liveness is pidfd/kqueue exit events, not kill(pid,0) polling → no read-then-signal race.',
  },
  {
    cause: 'crashed launcher leaves the watcher sidecar respawning untracked VMs.',
    cite: 'managed.rs:2918-2927',
    mechanism: 'the watcher becomes a daemon child; stopping a worker tears down the whole supervision unit; no standalone sidecar outlives its target.',
  },
  {
    cause: 'host-binary stop signals a SINGLE pid, not the group → a forking binary orphans grandchildren.',
    cite: 'managed.rs:3016-3034',
    mechanism: 'killpg always; stop_owned SIGTERMs the group, 3s grace, SIGKILL group, wait() the leader.',
  },
]

/** the four (really five) detached spawn paths that collapse — section 05 */
export const SPAWN_PATHS = [
  { id: 'A', label: 'VM dev spawn', detach: 'setsid, vm.pid, return 0', fate: 'move' },
  { id: 'B', label: 'OCI VM spawn', detach: 'non-child waitpid', fate: 'move' },
  { id: 'C', label: 'host binary spawn', detach: 'setsid + pidfile + return 0', fate: 'move' },
  { id: 'D', label: 'source watcher', detach: 'watch.pid + standalone respawn', fate: 'move' },
  { id: 'E', label: 'engine middle-child', detach: 'transient start, never wait()ed', fate: 'delete' },
] as const

/** supervision tiers — section 05 */
export const SUPERVISION_TIERS = [
  {
    tier: 'TIER 0',
    name: 'OS init',
    body: 'launchd (macOS, KeepAlive=true) / systemd user unit (Linux, Restart=always) / CLI foreground (dev). owns + restarts the daemon pid — the only turtle below it.',
  },
  {
    tier: 'TIER 1',
    name: 'iii-process-daemon',
    body: 'a separate long-lived process (iii __process-daemon). the direct parent of all host worker pids; wait()-reaps; holds the process table.',
  },
  {
    tier: 'TIER 2',
    name: 'every worker pid',
    body: 'host process / __vm-boot VMM / watcher / exec pipeline — all grouped children of the daemon.',
  },
] as const

/** the L0/L1/L2 readiness contract — section 06 */
export interface ReadinessLevel {
  level: string
  name: string
  met: string
  condition: string
  isDefault?: boolean
}

export const READINESS_LEVELS: ReadinessLevel[] = [
  {
    level: 'L0',
    name: 'spawned',
    met: 'the process has a pid (daemon process table).',
    condition: 'condition: started',
  },
  {
    level: 'L1',
    name: 'connected',
    met: 'ws-connected AND functions registered (engine registry + status::watch_until_ready).',
    condition: 'default — `ready`',
    isDefault: true,
  },
  {
    level: 'L2',
    name: 'healthy',
    met: 'a worker-declared healthcheck: passes (daemon polls the declared probe).',
    condition: 'condition: healthy',
  },
]

/** the dependency-ordered bring-up graph — section 06 */
export const READINESS_NODES = [
  { id: 'configuration', label: 'configuration', note: 'always-ready root', root: true },
  { id: 'state', label: 'state', note: 'remote · iii-state:latest' },
  { id: 'http', label: 'http', note: 'remote · iii-http:latest' },
  { id: 'math-worker', label: 'math-worker', note: 'local · 3 fns' },
  { id: 'caller-worker', label: 'caller-worker', note: 'local · depends_on math + state' },
] as const

/** the compose::up topology pipeline — section 06 */
export const TOPO_PIPELINE = [
  'build the graph from depends_on across the workers map',
  'validate every dep id resolves (C020) — implicit roots count',
  'detect cycles (C021 lists the cycle; self-ref → C022)',
  'topological sort → start order; within a layer, start in parallel',
  'before a dependent starts, await each dep’s readiness (watch_until_ready), bounded by a start timeout',
] as const

/** the four-layer merge chain — section 07 */
export interface StackLayer {
  n: number
  label: string
  note: string
  wins?: boolean
}

export const MERGE_CHAIN: StackLayer[] = [
  { n: 1, label: 'inferred defaults', note: 'infer_scripts + kind base image + resource caps' },
  { n: 2, label: 'iii.worker.yaml', note: 'the worker author’s manifest' },
  { n: 3, label: 'compose.defaults', note: 'project-wide partial block' },
  { n: 4, label: 'compose.workers.<id>', note: 'the operator override — authoritative', wins: true },
]

/** the eight-step boot sequence — section 08 */
export const BOOT_SEQUENCE = [
  { n: 1, title: 'parse worker-compose.yml', body: 'expand ${VAR:default}; read port, gateway, configuration, workers. no config.yaml read; no worker-entry port scan.' },
  { n: 2, title: 'bind the gateway', body: 'set_worker_manager_port(compose.port); TcpListener {host}:{port}; apply compose.gateway.rbac. the port is open; nothing else is started.' },
  { n: 3, title: 'boot-read restart-tier config off disk', body: 'read ./data/configuration/<id>.yaml; expand ${VAR}; catch_unwind a bad value → use the persisted value or skip; init logging/tracing — BEFORE the store worker is up.' },
  { n: 4, title: 'start the configuration worker', body: 'mandatory, auto-injected; adapter=fs at compose.configuration.directory; prime the in-memory store from disk.' },
  { n: 5, title: 'start remaining workers in depends_on order', body: 'each spawned by the process-daemon (not the engine), orchestrated by worker-ops compose::up. each worker registers its own config schema + initial value at boot via configuration::register.' },
  { n: 6, title: 'gate each on readiness', body: 'await L1 (ws-connected + functions registered) before starting dependents.' },
  { n: 7, title: 'serve', body: 'all workers ready; the gateway routes invocations; worker-ops holds the correlation map.' },
] as const

/** the six-phase migration — section 09 + /migration page */
export interface MigrationPhase {
  n: string
  title: string
  body: string
  shipped?: boolean
}

export const MIGRATION_PHASES: MigrationPhase[] = [
  { n: '0', title: 'dual-parser', body: 'compose is lowered to today’s EngineConfig (no behavior change, behind a --compose flag).' },
  { n: '1', title: 'iii.lock split + iii migrate', body: 'the resolved lockfile splits out; the one-shot migrate tool lands.' },
  { n: '2', title: 'per-worker config → store', body: 'one worker at a time, behind boot-read. largely landed — all seven built-ins self-register their schema + initial value and read the store.', shipped: true },
  { n: '3', title: 'the iii-process-daemon', body: 'flag-gated; the risky middle; ~10 PRs decomposing managed.rs.' },
  { n: '4', title: 'bake in the gateway', body: 'delete iii-worker-manager + iii-exec entries; cloud canary.' },
  { n: '5', title: 'remove config.yaml', body: 'cli / function alias cleanup; the coexistence bridge retires.' },
]

/** the error & recovery DX gallery — section 10 */
export interface ErrorCard {
  scenario: string
  trigger: string
  output: string[]
  resolution: string
}

export const ERROR_GALLERY: ErrorCard[] = [
  {
    scenario: 'crashed worker',
    trigger: 'a process exits with code 1',
    output: ['✗ caller-worker exited (code 1)', '  restarting (1/5)…', '  ✓ ready'],
    resolution: 'bounded exponential backoff; prints the last 10 log lines; after N stops → failed.',
  },
  {
    scenario: 'orphan sweep',
    trigger: 'the daemon was kill -9’d last run',
    output: ['! found 2 orphaned worker processes from a previous run', '  reclaimed — reattached by instance_token'],
    resolution: 'zombies are impossible; the startup sweep re-adopts the alive ones by token.',
  },
  {
    scenario: 'missing dependency',
    trigger: 'a typo in depends_on',
    output: ['✗ compose error: worker "caller-worker" depends_on', '  "mathh-worker" which is not declared', '  did you mean "math-worker"?'],
    resolution: 'validated up-front — nothing starts until the graph is sound.',
  },
  {
    scenario: 'dependency cycle',
    trigger: 'caller-worker → state → caller-worker',
    output: ['✗ dependency cycle detected:', '  caller-worker → state → caller-worker'],
    resolution: 'C021 lists the cycle; a self-reference is C022.',
  },
  {
    scenario: 'port conflict',
    trigger: 'another engine holds :49134',
    output: ['✗ cannot bind ws://localhost:49134', '  address already in use', '  try: iii down · iii ps · edit top-level port:'],
    resolution: 'project identity IS the port — each project is isolated under ~/.iii/daemon/<port>/.',
  },
  {
    scenario: 'install / version failure',
    trigger: 'the registry is unreachable',
    output: ['✗ state failed to resolve iii-state:latest (network)', '  using last-locked version from iii.lock: iii-state@1.4.2', '  ✓ state ready (offline, locked)'],
    resolution: 'the lock makes up reproducible — and survivable offline.',
  },
]

/** the consolidated verb surface — section 10 */
export const VERBS = [
  { verb: 'iii up / down', fn: 'compose::up / compose::down' },
  { verb: 'iii ps', fn: 'process::ps' },
  { verb: 'iii logs', fn: 'process::logs' },
  { verb: 'iii trigger', fn: 'invocation routing (gateway)' },
  { verb: 'iii init', fn: 'scaffolder (iii-worker-ops)' },
  { verb: 'iii worker add / remove', fn: 'worker::add / worker::remove' },
  { verb: 'iii worker config', fn: 'configuration::set / get' },
  { verb: 'iii migrate', fn: 'migrate::config_yaml' },
] as const

/** payoff scorecard — section 10 */
export const SCORECARD = [
  { metric: 'files a human edits', before: 'config.yaml + N config blobs', after: 'one worker-compose.yml' },
  { metric: 'commands to first call', before: '~5', after: '4' },
  { metric: 'terminals', before: '2', after: '1' },
  { metric: 'lifecycle owner', before: 'the engine (+ detached procs)', after: 'iii-process-daemon' },
  { metric: 'startup race', before: '"function not found"', after: 'depends_on readiness gate' },
  { metric: 'process safety', before: 'orphans / zombies possible', after: 'impossible by construction' },
  { metric: 'cli surface', before: '~30 leaf-commands', after: 'a small verb set, all functions' },
] as const

/** the `iii up` sequence for the meeting-points player — section 04 */
export interface SeqLane {
  id: string
  label: string
  x: number
}

export interface SeqStep {
  from: string
  to: string
  label: string
  title: string
  desc: string
  meeting?: string
  dashed?: boolean
}

export const SEQ_LANES: SeqLane[] = [
  { id: 'ops', label: 'iii-worker-ops', x: 120 },
  { id: 'daemon', label: 'iii-process-daemon', x: 360 },
  { id: 'sandbox', label: 'iii-sandbox', x: 600 },
  { id: 'worker', label: 'worker', x: 800 },
  { id: 'gw', label: 'worker-gateway', x: 980 },
]

export const SEQ_STEPS: SeqStep[] = [
  {
    from: 'ops',
    to: 'ops',
    label: 'compose::up',
    title: 'parse + topo-sort',
    desc: 'iii-worker-ops parses worker-compose.yml, builds the depends_on graph, validates + cycle-checks it, and topo-sorts the start order. the daemon never sees this graph.',
  },
  {
    from: 'ops',
    to: 'daemon',
    label: 'process::start { spec }',
    title: 'meeting point ①',
    desc: 'for each node in order, ops calls process::start on the daemon. the brain decides WHAT to run; the daemon owns the pid. these planes touch only here.',
    meeting: '①',
  },
  {
    from: 'daemon',
    to: 'sandbox',
    label: 'sandbox::create { image, keep_alive }',
    title: 'meeting point ②',
    desc: 'a sandboxed worker: the daemon calls sandbox::create, which boots a libkrun __vm-boot. keep_alive carves the worker out of the idle reaper. the daemon stays dumb about images.',
    meeting: '②',
  },
  {
    from: 'sandbox',
    to: 'daemon',
    label: '{ sandbox_id, vm_pid }',
    title: 'parent the vm pid',
    desc: 'sandbox returns the vm pid. the daemon parents it — spawn_owned: setpgid(0,0), keep the Child forever, inject III_INSTANCE_TOKEN. wait()-reaping is now automatic.',
    dashed: true,
  },
  {
    from: 'worker',
    to: 'gw',
    label: 'ws connect-back + instance_token',
    title: 'meeting point ③',
    desc: 'the worker boots and connects back to the gateway, presenting its daemon-minted instance_token. process identity (the daemon table) meets connection identity (the engine registry).',
    meeting: '③',
  },
  {
    from: 'gw',
    to: 'ops',
    label: 'worker_connected',
    title: 'correlate + mark managed',
    desc: 'ops correlates by (port, compose_id, instance_token). token present → managed (full control); absent → observe-only. once functions register, the node is L1-ready and the next node starts.',
  },
]

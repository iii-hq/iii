/**
 * map.ts — the four-planes system map. Geometry + datasheets for every node
 * and the edges between them (the three meeting points ①②③ + the bootstrap
 * floor). Condensed from README.md "Architecture: four planes, three meeting
 * points" + the component responsibility map.
 *
 * Palette discipline: the schematic is ink-on-cream with a single orange
 * accent for selection/active. The four-plane taxonomy (plane / brain / pid /
 * file / worker) is encoded by an uppercase corner label and a small kind dot —
 * the dot is the only rationed splash of plane-hue, used as a legend key.
 */

export type NodeKind = 'plane' | 'brain' | 'pid' | 'file' | 'wk'

export interface NodeSheet {
  role: string
  owns: string
  neverDoes: string
  functions: string
  install: string
}

export interface MapNode {
  id: string
  x: number
  y: number
  w: number
  h: number
  title: string
  sub: string
  kind: NodeKind
  kindLabel: string
  sheet: NodeSheet
}

export interface MapEdge {
  id: string
  from: string
  to: string
  d: string
  label: string
  lx: number
  ly: number
  anchor?: 'start' | 'middle' | 'end'
  dashed?: boolean
  bidir?: boolean
  meeting?: string
  dur?: number
}

/** the rationed plane-hue used only for the small kind dot + legend key */
export const KIND_HUE: Record<NodeKind, string> = {
  plane: '#22c55e',
  brain: '#f59e0b',
  pid: '#ef4444',
  file: '#60a5fa',
  wk: '#9ca3af',
}

export const KIND_LABEL: Record<NodeKind, string> = {
  plane: 'connection plane',
  brain: 'the brain',
  pid: 'pid owner',
  file: 'file',
  wk: 'worker',
}

export const MAP_NODES: MapNode[] = [
  {
    id: 'compose',
    x: 24,
    y: 56,
    w: 210,
    h: 64,
    title: 'worker-compose.yml',
    sub: 'desired state · human-authored',
    kind: 'file',
    kindLabel: 'FILE',
    sheet: {
      role: 'the single human-authored boot file — the "boot egg". the only file a human edits.',
      owns: 'the irreducible bootstrap floor: the ws gateway port, the configuration-store location, and the worker list + topology.',
      neverDoes: 'hold resolved versions/hashes; carry any per-worker configuration at all (no seed, no config.path) — config lives end-to-end in the configuration worker.',
      functions: '— (a file, not a worker)',
      install: 'iii init scaffolds it.',
    },
  },
  {
    id: 'lock',
    x: 24,
    y: 140,
    w: 210,
    h: 64,
    title: 'iii.lock',
    sub: 'resolved state · machine-written',
    kind: 'file',
    kindLabel: 'FILE',
    sheet: {
      role: 'machine-written resolved lockfile — the package.json / lockfile split.',
      owns: 'resolved concrete versions + sha256 hashes + pinned digests; keyed by (package, version).',
      neverDoes: 'get hand-edited; pin local workspace workers (source, not artifact).',
      functions: '—',
      install: 'written on first `iii up`; replayed offline (npm ci semantics).',
    },
  },
  {
    id: 'gw',
    x: 300,
    y: 44,
    w: 268,
    h: 84,
    title: 'worker-gateway',
    sub: 'ws port · protocol · rbac · registry · routing',
    kind: 'plane',
    kindLabel: 'ENGINE — CONNECTION PLANE',
    sheet: {
      role: 'an internal engine concept (NOT a worker). the engine as a pure connection plane.',
      owns: 'binds the ws gateway port from compose.port; iii/worker protocol; rbac handshake; the live-connection registry; invocation routing; the compose-file watcher.',
      neverDoes: 'spawn an OS process; resolve versions; read worker scripts; own a pid. the engine’s cmd.spawn paths are DELETED.',
      functions: 'routes all; owns none. registration gains compose_id, instance_token, managed.',
      install: 'baked into the engine binary; replaces the misnamed iii-worker-manager.',
    },
  },
  {
    id: 'config',
    x: 636,
    y: 44,
    w: 236,
    h: 84,
    title: 'configuration',
    sub: 'config store · schema · hot-reload',
    kind: 'plane',
    kindLabel: 'PLANE — MANDATORY',
    sheet: {
      role: 'the mandatory, auto-injected per-worker config store; the always-ready root of every depends_on graph.',
      owns: 'one entry per id; json-schema validation on set; ${VAR}/${VAR:default} expansion on read; the hot-reload trigger; the bootstrap-tier value-only boot-read off disk.',
      neverDoes: 'store its own location (→ compose). a `config-worker` worker does NOT exist — config-worker:<id> is only a URI scheme.',
      functions: 'configuration::{register,set,get,list,schema} + the new ConfigurationEntry.secret: bool',
      install: 'auto-injected, mandatory. if it never readies, `up` HARD-FAILS.',
    },
  },
  {
    id: 'ops',
    x: 300,
    y: 182,
    w: 268,
    h: 92,
    title: 'iii-worker-ops',
    sub: 'compose · topo-sort + readiness · iii.lock',
    kind: 'brain',
    kindLabel: 'THE BRAIN',
    sheet: {
      role: 'the brain. owns compose semantics; MAY be an in-engine builtin.',
      owns: 'parse + 4-layer merge (runtime / scripts / environment, not config) + env precedence; depends_on graph → validate → cycle-detect → topo-sort → readiness gate; resolve/install/version; iii.lock; reconcile desired↔actual; the correlation map.',
      neverDoes: 'hold a Child handle; bind a socket; spawn a pid.',
      functions: 'worker::{add,update,remove,clear,list,info,schema}; compose::{up,down,restart,status,validate}',
      install: 'may be in-engine; on `up` it topo-sorts and calls process::start per node.',
    },
  },
  {
    id: 'daemon',
    x: 300,
    y: 320,
    w: 268,
    h: 92,
    title: 'iii-process-daemon',
    sub: 'the pid parent · wait()-reaps everything',
    kind: 'pid',
    kindLabel: 'THE PID PARENT',
    sheet: {
      role: 'the pid parent — a separate long-lived process (iii __process-daemon). MUST NOT be in-engine.',
      owns: 'being the direct parent of every host pid; the process table; spawn / killpg-group / wait()-reap; log capture; instance_token mint+inject; crash recovery; startup orphan sweep; restart policy. absorbs iii-exec.',
      neverDoes: 'decide WHAT to run; resolve versions; speak the compose graph.',
      functions: 'process::{start,stop,restart,status,ps,logs,exec,signal,attach,reconcile}',
      install: 'launched via the hidden subcommand iii __process-daemon; lock + state under ~/.iii/daemon/<port>/.',
    },
  },
  {
    id: 'sandbox',
    x: 636,
    y: 320,
    w: 236,
    h: 92,
    title: 'iii-sandbox',
    sub: 'libkrun microVMs · OCI',
    kind: 'pid',
    kindLabel: 'SEPARATE PROCESS',
    sheet: {
      role: 'the microVM lifecycle owner — a separate long-lived process. public API unchanged.',
      owns: 'microVM lifecycle (libkrun __vm-boot); OCI image pull/catalog/overlay; in-VM exec + fs; an idle reaper WITH a keep_alive carve-out for compose-managed sandboxed workers. owns the runtime-adapter.',
      neverDoes: 'be the parent of host (non-VM) workers.',
      functions: 'sandbox::{create,run,exec,stop,list,catalog::list} + sandbox::fs::{…} (unchanged)',
      install: 'launched via the hidden subcommand iii __sandbox-daemon.',
    },
  },
  {
    id: 'w1',
    x: 120,
    y: 486,
    w: 190,
    h: 64,
    title: 'worker',
    sub: 'host process',
    kind: 'wk',
    kindLabel: 'MANAGED',
    sheet: {
      role: 'a daemon-managed host process worker.',
      owns: 'its own registered functions.',
      neverDoes: '—',
      functions: 'registers over ws with an instance_token → full control',
      install: 'spawned + wait()-reaped by the daemon (spawn_owned).',
    },
  },
  {
    id: 'w2',
    x: 360,
    y: 486,
    w: 190,
    h: 64,
    title: 'worker',
    sub: 'sandbox VM',
    kind: 'wk',
    kindLabel: 'MANAGED',
    sheet: {
      role: 'a daemon-managed worker running inside a libkrun microVM.',
      owns: 'its own registered functions.',
      neverDoes: '—',
      functions: 'registers over ws with an instance_token → full control',
      install: 'sandbox::create boots __vm-boot; the daemon parents that pid.',
    },
  },
  {
    id: 'w3',
    x: 636,
    y: 486,
    w: 236,
    h: 64,
    title: 'independent worker',
    sub: 'observe-only',
    kind: 'wk',
    kindLabel: 'UNTOKENED',
    sheet: {
      role: 'a developer-started worker (npm run dev) that connects with no token.',
      owns: 'its own registered functions.',
      neverDoes: 'be lifecycle-controlled by the engine.',
      functions: 'registers over ws, no instance_token → observe-only',
      install: 'started by hand; process::attach is the opt-in escape hatch.',
    },
  },
]

export const MAP_EDGES: MapEdge[] = [
  {
    id: 'compose-ops',
    from: 'compose',
    to: 'ops',
    d: 'M 234 96 C 270 104, 270 206, 300 214',
    label: 'workers map',
    lx: 250,
    ly: 168,
    anchor: 'start',
    dur: 2,
  },
  {
    id: 'compose-gw',
    from: 'compose',
    to: 'gw',
    d: 'M 234 74 L 300 76',
    label: 'bootstrap floor — port · store loc',
    lx: 240,
    ly: 60,
    anchor: 'start',
    dashed: true,
  },
  {
    id: 'lock-ops',
    from: 'lock',
    to: 'ops',
    d: 'M 234 172 C 272 178, 270 248, 300 252',
    label: 'replay ⇄ write',
    lx: 240,
    ly: 220,
    anchor: 'start',
    bidir: true,
    dur: 2.2,
  },
  {
    id: 'ops-daemon',
    from: 'ops',
    to: 'daemon',
    d: 'M 434 274 L 434 320',
    label: '① compose::up → process::start / node',
    lx: 446,
    ly: 300,
    anchor: 'start',
    meeting: '①',
    dur: 1.6,
  },
  {
    id: 'ops-gw',
    from: 'ops',
    to: 'gw',
    d: 'M 396 182 L 396 128',
    label: 'correlate compose_id ⇄ token ⇄ pid',
    lx: 386,
    ly: 158,
    anchor: 'end',
    dur: 2,
  },
  {
    id: 'ops-config',
    from: 'ops',
    to: 'config',
    d: 'M 568 206 C 660 198, 706 150, 728 128',
    label: 'reads',
    lx: 648,
    ly: 176,
    anchor: 'middle',
    dur: 1.8,
  },
  {
    id: 'daemon-sandbox',
    from: 'daemon',
    to: 'sandbox',
    d: 'M 568 360 L 636 360',
    label: '② sandbox::create',
    lx: 602,
    ly: 350,
    anchor: 'middle',
    meeting: '②',
    dur: 1.6,
  },
  {
    id: 'daemon-w1',
    from: 'daemon',
    to: 'w1',
    d: 'M 360 412 C 300 442, 250 462, 216 486',
    label: 'spawns + reaps',
    lx: 262,
    ly: 452,
    anchor: 'middle',
    dur: 2,
  },
  {
    id: 'daemon-w2',
    from: 'daemon',
    to: 'w2',
    d: 'M 455 412 L 455 486',
    label: 'spawns + reaps',
    lx: 467,
    ly: 452,
    anchor: 'start',
    dur: 2,
  },
  {
    id: 'sandbox-w2',
    from: 'sandbox',
    to: 'w2',
    d: 'M 700 412 C 640 448, 590 468, 552 488',
    label: 'boots __vm-boot',
    lx: 642,
    ly: 456,
    anchor: 'middle',
    dashed: true,
    dur: 2.4,
  },
  {
    id: 'w1-gw',
    from: 'w1',
    to: 'gw',
    d: 'M 215 486 C 252 420, 272 240, 300 124',
    label: '③ connect-back + instance_token',
    lx: 250,
    ly: 330,
    anchor: 'start',
    meeting: '③',
    dur: 2.8,
  },
  {
    id: 'w2-gw',
    from: 'w2',
    to: 'gw',
    d: 'M 548 506 C 602 470, 602 200, 568 124',
    label: '③ + token',
    lx: 610,
    ly: 320,
    anchor: 'start',
    meeting: '③',
    dur: 3,
  },
  {
    id: 'w3-gw',
    from: 'w3',
    to: 'gw',
    d: 'M 872 502 C 1000 482, 1000 22, 560 30 C 510 31, 470 38, 452 44',
    label: 'ws connect (no token)',
    lx: 720,
    ly: 26,
    anchor: 'middle',
    dur: 3.4,
  },
]

export interface MeetingPoint {
  badge: string
  from: string
  title: string
  detail: string
}

export const MEETING_POINTS: MeetingPoint[] = [
  {
    badge: '①',
    from: 'ops → daemon',
    title: 'compose::up calls process::start per node',
    detail:
      'iii-worker-ops topo-sorts the graph, resolves each worker, and calls process::start per node — gating each on readiness. the daemon never speaks the compose graph.',
  },
  {
    badge: '②',
    from: 'daemon → sandbox',
    title: 'process::start(sandboxed) calls sandbox::create',
    detail:
      'for a sandboxed worker the daemon calls sandbox::create, then parents the resulting __vm-boot pid. the daemon stays dumb about images; iii-sandbox owns libkrun / OCI.',
  },
  {
    badge: '③',
    from: 'worker → gateway',
    title: 'ws connect-back + instance_token',
    detail:
      'the connect-back joins process identity (the daemon’s table) to connection identity (the engine registry). a daemon-minted token = managed (full control); none = observe-only. ops correlates via (port, compose_id, instance_token).',
  },
]

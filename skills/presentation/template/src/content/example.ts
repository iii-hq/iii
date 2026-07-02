/**
 * example.ts — PLACEHOLDER DATA for the starter deck.
 *
 * The /presentation skill replaces this with data extracted from the tech-spec
 * (often split into per-topic modules). It exists so the template renders and
 * builds out of the box, and so there is a concrete, copyable example of the
 * shape each archetype expects. Delete it once you have real content.
 */
import type { CliTrack } from '@lib/components/diagrams/CliPlayground'
import type { FanHandler } from '@lib/components/diagrams/FanOut'
import type { FunnelPath } from '@lib/components/diagrams/Funnel'
import type { SeqLane, SeqStep } from '@lib/components/diagrams/SequencePlayer'
import type { RevealStage } from '@lib/components/diagrams/StepReveal'
import type { MapEdge, MapNode, MapNodeInfo } from '@lib/components/diagrams/SystemMap'

/* ---- hero ---- */

export const HERO_STATS = [
  { value: '4', label: 'standalone parts' },
  { value: '1', label: 'declarative file' },
  { value: '0', label: 'manual steps' },
  { value: '100%', label: 'typed contracts' },
] as const

export const HERO_CLAIMS = [
  {
    title: 'one file',
    body: 'the whole system is declared in one place. no scattered config, no second source of truth.',
  },
  {
    title: 'one command',
    body: 'bring the entire stack up with a single command. no two-terminal dance, no ordering races.',
  },
  {
    title: 'clean planes',
    body: 'each part owns exactly one job and talks only over typed contracts. no hidden coupling.',
  },
  {
    title: 'correct by construction',
    body: 'the failure modes that used to need discipline are now impossible by design.',
  },
] as const

/* ---- system map (A4) ---- */

export const MAP_NODES: MapNode[] = [
  { id: 'client', x: 40, y: 250, w: 170, h: 60, title: 'client', sub: 'any caller', kind: 'external' },
  { id: 'gateway', x: 320, y: 248, w: 200, h: 66, title: 'gateway', sub: 'routes calls', kind: 'primary' },
  { id: 'worker', x: 650, y: 120, w: 200, h: 66, title: 'worker', sub: 'runs the loop', kind: 'primary' },
  { id: 'store', x: 650, y: 376, w: 200, h: 66, title: 'store', sub: 'durable state', kind: 'secondary' },
  { id: 'watcher', x: 900, y: 248, w: 120, h: 66, title: 'watcher', sub: 'reacts', kind: 'optional' },
]

export const MAP_EDGES: MapEdge[] = [
  {
    id: 'request',
    from: 'client',
    to: 'gateway',
    d: 'M 210 280 L 320 281',
    label: 'request',
    lx: 265,
    ly: 272,
    dur: 2,
  },
  {
    id: 'dispatch',
    from: 'gateway',
    to: 'worker',
    d: 'M 520 268 C 590 250, 600 158, 650 152',
    label: 'dispatch',
    lx: 600,
    ly: 196,
    anchor: 'start',
    dur: 1.8,
  },
  {
    id: 'persist',
    from: 'gateway',
    to: 'store',
    d: 'M 520 296 C 590 320, 600 404, 650 410',
    label: 'persist',
    lx: 600,
    ly: 372,
    anchor: 'start',
    dur: 1.8,
  },
  {
    id: 'write',
    from: 'worker',
    to: 'store',
    d: 'M 750 186 L 750 376',
    label: 'write result',
    lx: 758,
    ly: 290,
    anchor: 'start',
    dur: 1.6,
  },
  {
    id: 'emit',
    from: 'store',
    to: 'watcher',
    d: 'M 850 404 C 920 388, 960 332, 960 314',
    label: 'emit event',
    lx: 968,
    ly: 360,
    anchor: 'start',
    dur: 2.2,
  },
]

export const MAP_INFO: Record<string, MapNodeInfo> = {
  client: {
    id: 'client',
    kindLabel: 'external',
    role: 'any caller — a web app, a cli, another service. it only knows the gateway contract.',
    sections: [
      {
        heading: 'calls',
        items: [{ name: 'gateway::request', desc: 'submit work and get an id back immediately.' }],
      },
    ],
    note: 'replace this node with the real entry points of your system.',
  },
  gateway: {
    id: 'gateway',
    kindLabel: 'core',
    role: 'the single front door: validates, routes, and hands work to the right plane.',
    sections: [
      {
        heading: 'surface',
        items: [
          { name: 'gateway::request', desc: 'accept work, enqueue it, return fast.' },
          { name: 'gateway::status', desc: 'point-in-time read of a request.' },
        ],
      },
    ],
    install: 'add gateway',
  },
  worker: {
    id: 'worker',
    kindLabel: 'core',
    role: 'runs the actual work loop — durable, resumable, one job at a time.',
    sections: [
      {
        heading: 'surface',
        items: [
          { name: 'worker::run', desc: 'execute one unit of work to completion.' },
          { name: 'worker::stop', desc: 'cancel an in-flight run.' },
        ],
      },
    ],
    bullets: ['every step is a durable entry — a crash resumes mid-run.'],
    install: 'add worker',
  },
  store: {
    id: 'store',
    kindLabel: 'supporting',
    role: 'the durable, reactive state — every write emits an event.',
    sections: [
      {
        heading: 'emits',
        dotted: true,
        items: [
          { name: 'store::written', desc: 'a record changed — bind and render live.' },
          { name: 'store::removed', desc: 'a record was deleted.' },
        ],
      },
    ],
    note: 'pure storage + notification. it runs no business logic of its own.',
  },
  watcher: {
    id: 'watcher',
    kindLabel: 'optional',
    role: 'an optional reactor: binds store events and does something useful — notify, index, escalate.',
    sections: [
      {
        heading: 'binds',
        items: [{ name: 'store::written', desc: 'the only thing it needs to know.' }],
      },
    ],
    note: 'optional by design — the core three run with or without it.',
  },
}

/* ---- sequence (A5) ---- */

export const SEQ_LANES: SeqLane[] = [
  { id: 'client', label: 'client', x: 110 },
  { id: 'gateway', label: 'gateway', x: 380 },
  { id: 'worker', label: 'worker', x: 640 },
  { id: 'store', label: 'store', x: 880 },
]

export const SEQ_STEPS: SeqStep[] = [
  {
    from: 'client',
    to: 'gateway',
    label: 'gateway::request',
    title: 'submit work',
    desc: 'the client drops a request and gets a request id back immediately — no blocking.',
  },
  {
    from: 'gateway',
    to: 'store',
    label: 'store::write',
    title: 'record it',
    desc: 'the gateway persists the new request; the write fires an event.',
    event: 'store::written',
  },
  {
    from: 'gateway',
    to: 'worker',
    label: 'worker::run',
    title: 'dispatch',
    desc: 'the gateway hands the queued work to a worker to execute.',
  },
  {
    from: 'worker',
    to: 'worker',
    label: 'do the work',
    title: 'run the loop',
    desc: 'the worker runs durable steps; a crash resumes from the last committed step.',
  },
  {
    from: 'worker',
    to: 'store',
    label: 'store::write',
    title: 'write result',
    desc: 'the worker commits the typed result; the store fires another event.',
    event: 'store::written',
  },
  {
    from: 'store',
    to: 'client',
    label: 'live update',
    title: 'render live',
    desc: 'the client was subscribed all along — the result appears with no polling.',
  },
]

/* ---- payoff (A11) ---- */

export const PAYOFF_METRICS = [
  { label: 'files to edit', before: '7', after: '1' },
  { label: 'commands to boot', before: '5', after: '1' },
  { label: 'terminals open', before: '2', after: '1' },
  { label: 'startup races', before: 'many', after: '0' },
] as const

export const PAYOFF_SOLVES = [
  {
    problem: 'config scattered across files',
    answer: 'one declarative file',
    detail: 'a single source of truth the whole stack reads from.',
  },
  {
    problem: 'manual, order-sensitive boot',
    answer: 'one command, topo-sorted',
    detail: 'dependencies bring up in the right order automatically.',
  },
  {
    problem: 'state changes you have to poll for',
    answer: 'events on every write',
    detail: 'bind once and render live — never chase status again.',
  },
] as const

/* ---- lifecycle / durability (A6) ---- */

export const REVEAL_STAGES: RevealStage[] = [
  {
    label: 'queued',
    tone: 'ink',
    caption: 'the request lands and is written as a durable entry before anything runs.',
    rows: [
      { k: 'status', v: 'queued' },
      { k: 'step', v: '0 / 3' },
    ],
  },
  {
    label: 'running',
    tone: 'ink',
    caption: 'a worker picks it up and commits each step as it goes.',
    rows: [
      { k: 'status', v: 'running' },
      { k: 'step', v: '2 / 3' },
    ],
  },
  {
    label: 'crash',
    tone: 'alert',
    caption: 'the worker dies mid-step. the in-flight step is lost — but the committed log is not.',
    rows: [
      { k: 'status', v: 'interrupted' },
      { k: 'step', v: '2 / 3' },
    ],
    note: 'a crash is a state transition, not lost work',
  },
  {
    label: 'resume',
    tone: 'warn',
    caption: 'a fresh worker reads the log and continues from the last committed step. nothing replays twice.',
    rows: [
      { k: 'status', v: 'resuming' },
      { k: 'step', v: '2 / 3' },
    ],
  },
  {
    label: 'done',
    tone: 'accent',
    caption: 'the final step commits and the typed result is written.',
    rows: [
      { k: 'status', v: 'done' },
      { k: 'step', v: '3 / 3' },
    ],
    note: 'work lost: none. steps replayed: zero.',
  },
]

/* ---- reactive fan-out (A7) ---- */

export const FAN_SOURCE = { label: 'store::write', sub: 'one commit, no publish step' }
export const FAN_TRIGGER = 'store::written'
export const FAN_HANDLERS: FanHandler[] = [
  { id: 'watcher', label: 'watcher::react', desc: 'escalate or act on the change' },
  { id: 'index', label: 'search::index', desc: 'keep the index current' },
  { id: 'metrics', label: 'metrics::count', desc: 'live dashboard tiles' },
]

/* ---- convergence funnel (A8) ---- */

export const FUNNEL_PATHS: FunnelPath[] = [
  { id: 'fire', label: 'fire and forget', desc: 'no record, no retry' },
  { id: 'thread', label: 'detached thread', desc: 'dies with the process' },
  { id: 'retry', label: 'naive retry', desc: 'duplicates on restart' },
]
export const FUNNEL_TARGET = { label: 'durable queue entry', sub: 'committed before ack — exactly-once, resumable' }
export const FUNNEL_REJECT = { label: 'in-memory only', desc: 'lost on crash — removed by design' }

/* ---- cli playground (A3) ---- */

export const CLI_TRACKS: CliTrack[] = [
  {
    id: 'first-run',
    label: 'first run',
    lines: [
      { cmd: 'your-cli up', out: ['→ reading your-spec.yml', '✓ stack up — gateway, worker, store'] },
      {
        cmd: 'your-cli request { input }',
        fn: 'gateway::request',
        out: ['→ { id: "r_8c2", accepted: true }'],
        exit: 0,
      },
      { cmd: 'your-cli status r_8c2', fn: 'gateway::status', out: ['✓ done — result ready'] },
    ],
  },
  {
    id: 'day-2',
    label: 'day-2 ops',
    lines: [
      { cmd: 'your-cli ps', out: ['ID        STATUS   UPTIME', 'worker-1  running  4h12m'] },
      {
        cmd: 'your-cli logs worker-1 --tail 2',
        fn: 'ops::logs',
        out: ['"handled r_8c1 in 12ms"', '"handled r_8c2 in 9ms"'],
        exit: 0,
      },
    ],
  },
]

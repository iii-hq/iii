/* architecture — the system map (A4). nodes + edges + per-node datasheets. */
import type { MapEdge, MapNode, MapNodeInfo } from '@lib/components/diagrams/SystemMap'

/* coordinate space: 1030 x 600 (the SystemMap default viewBox) */

export const MAP_NODES: MapNode[] = [
  {
    id: 'engine',
    x: 415,
    y: 48,
    w: 200,
    h: 70,
    title: 'iii engine',
    sub: 'route · arbitrate · buffer',
    kind: 'primary',
    tag: 'engine',
  },
  {
    id: 'config',
    x: 770,
    y: 52,
    w: 200,
    h: 62,
    title: 'configuration',
    sub: 'base configs · any adapter',
    kind: 'secondary',
    tag: 'builtin',
  },
  {
    id: 'daemon-a',
    x: 120,
    y: 240,
    w: 210,
    h: 70,
    title: 'compose daemon a',
    sub: 'id host-a · owns its children',
    kind: 'primary',
    tag: 'daemon',
  },
  {
    id: 'daemon-b',
    x: 690,
    y: 240,
    w: 200,
    h: 70,
    title: 'compose daemon b',
    sub: 'id host-b · another machine',
    kind: 'secondary',
    tag: 'daemon',
  },
  {
    id: 'database',
    x: 60,
    y: 460,
    w: 160,
    h: 58,
    title: 'database',
    sub: 'ns orders',
    kind: 'secondary',
    tag: 'child',
  },
  { id: 'api', x: 250, y: 460, w: 160, h: 58, title: 'api', sub: 'ns orders', kind: 'secondary', tag: 'child' },
  {
    id: 'state',
    x: 740,
    y: 460,
    w: 160,
    h: 58,
    title: 'state',
    sub: 'ns analytics',
    kind: 'secondary',
    tag: 'child',
  },
]

export const MAP_EDGES: MapEdge[] = [
  {
    id: 'a-engine',
    from: 'daemon-a',
    to: 'engine',
    d: 'M 225 240 L 225 180 L 468 180 L 468 118',
    label: 'control · compose::* (id host-a)',
    lx: 250,
    ly: 172,
    anchor: 'start',
  },
  {
    id: 'b-engine',
    from: 'daemon-b',
    to: 'engine',
    d: 'M 790 240 L 790 180 L 562 180 L 562 118',
    label: 'control · compose::* (id host-b)',
    lx: 782,
    ly: 172,
    anchor: 'end',
  },
  {
    id: 'engine-config',
    from: 'engine',
    to: 'config',
    d: 'M 615 83 L 770 83',
    label: 'fetch base',
    lx: 693,
    ly: 73,
    anchor: 'middle',
    dashed: true,
  },
  {
    id: 'a-database',
    from: 'daemon-a',
    to: 'database',
    d: 'M 180 310 L 180 400 L 140 400 L 140 460',
    label: 'supervise',
    lx: 168,
    ly: 394,
    anchor: 'end',
  },
  { id: 'a-api', from: 'daemon-a', to: 'api', d: 'M 270 310 L 270 400 L 330 400 L 330 460' },
  { id: 'b-state', from: 'daemon-b', to: 'state', d: 'M 790 310 L 790 385 L 820 385 L 820 460' },
  {
    id: 'api-engine',
    from: 'api',
    to: 'engine',
    d: 'M 410 489 L 495 489 L 495 118',
    label: 'register · ns orders',
    lx: 505,
    ly: 420,
    anchor: 'start',
    dashed: true,
    dur: 3.2,
  },
  {
    id: 'state-engine',
    from: 'state',
    to: 'engine',
    d: 'M 900 460 L 900 205 L 585 205 L 585 118',
    label: 'register · ns analytics',
    lx: 908,
    ly: 350,
    anchor: 'start',
    dashed: true,
    dur: 3.2,
  },
]

export const MAP_INFO: Record<string, MapNodeInfo> = {
  engine: {
    id: 'engine',
    kindLabel: 'engine · coordinator',
    role: 'the shared router and the single arbitration point. it never supervises a process.',
    sections: [
      {
        heading: 'does',
        dotted: true,
        items: [
          { name: 'route triggers', desc: 'function ids stay verbatim; namespace travels beside them.' },
          { name: 'reject collisions', desc: 'same name + same namespace = refused registration, not overwrite.' },
          { name: 'buffer registrations', desc: 'triggers for absent workers wait with a bounded ttl, then flush.' },
        ],
      },
    ],
    bullets: ['readiness truth: a worker is ready when its registration is visible here.'],
  },
  config: {
    id: 'config',
    kindLabel: 'engine builtin',
    role: 'holds base configurations by name; the storage behind it is an adapter (fs today, secrets manager or another iii in cloud).',
    sections: [
      {
        heading: 'contract',
        items: [
          { name: 'config_name', desc: 'compose containers reference configs by name, never by file path.' },
          { name: 'fetch-before-spawn', desc: 'the daemon pulls the base before any process exists.' },
        ],
      },
    ],
    note: 'names keep the compose file valid when storage moves — nothing in it points at a filesystem.',
  },
  'daemon-a': {
    id: 'daemon-a',
    kindLabel: 'compose daemon',
    role: 'one process per machine, bound to one compose file for its lifetime. the only supervisor of its own children.',
    sections: [
      {
        heading: 'remote surface',
        dotted: true,
        items: [
          { name: 'compose::up / down', desc: 'write — start or stop the graph, dependents ordered. addressed by id=host-a.' },
          { name: 'compose::list / status / logs', desc: 'read — the addressed daemon’s compose project only.' },
          { name: 'compose::validate', desc: 'read — revalidate the bound file.' },
        ],
      },
      {
        heading: 'per child',
        items: [
          { name: 'inject', desc: 'III_URL · III_NAMESPACE · III_CONFIG into every spawn and hook.' },
          { name: 'own', desc: 'process group / job object; sigterm → grace → sigkill.' },
        ],
      },
    ],
    install: 'iii compose --id host-a --file worker-compose.yaml',
  },
  'daemon-b': {
    id: 'daemon-b',
    kindLabel: 'compose daemon',
    role: 'a second daemon on a second machine, attached to the same engine. it cannot touch host-a’s children.',
    bullets: [
      'sharing a worker across files is a namespace decision, not a cross-file dependency.',
      'down here never cascades to another daemon’s processes.',
    ],
  },
  database: {
    id: 'database',
    kindLabel: 'child worker',
    role: 'a package:// binary. no run script — compose execs the resolved artifact with the standard flags.',
    sections: [
      {
        heading: 'start',
        items: [
          { name: 'implicit', desc: 'exec artifact + --url --namespace --config.' },
          { name: 'ready', desc: 'when its registration appears on the engine.' },
        ],
      },
    ],
  },
  api: {
    id: 'api',
    kindLabel: 'child worker',
    role: 'a path:// local worker. run: "pnpm dev" in the compose file — no manifest required.',
    sections: [
      {
        heading: 'hooks',
        items: [
          { name: 'pre_start', desc: 'prisma migrate, blocking, 60s default budget.' },
          { name: 'post_run', desc: 'cleanup script, fired after the run exits, never awaited.' },
        ],
      },
    ],
  },
  state: {
    id: 'state',
    kindLabel: 'child worker',
    role: 'the same published state worker another project also runs — different namespace, zero renames.',
    bullets: ['function ids identical to every other state instance; the namespace disambiguates.'],
  },
}

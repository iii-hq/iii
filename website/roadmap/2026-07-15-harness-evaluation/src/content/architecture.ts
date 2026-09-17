import type { MapEdge, MapNode, MapNodeInfo } from '@lib/components/diagrams/SystemMap'

export const MAP_NODES: MapNode[] = [
  { id: 'launcher', x: 26, y: 258, w: 150, h: 82, title: 'CLI / CI', sub: 'select one track', kind: 'external' },
  {
    id: 'integration',
    x: 228,
    y: 64,
    w: 190,
    h: 92,
    title: 'integration runner',
    sub: 'scripted boundary',
    kind: 'primary',
  },
  { id: 'quality', x: 228, y: 444, w: 190, h: 92, title: 'quality runner', sub: 'pinned subject', kind: 'primary' },
  { id: 'harness', x: 478, y: 254, w: 178, h: 92, title: 'harness', sub: 'public turn path', kind: 'primary' },
  {
    id: 'scripted',
    x: 790,
    y: 62,
    w: 194,
    h: 82,
    title: 'scripted router',
    sub: 'integration only',
    kind: 'secondary',
    tag: 'controlled',
  },
  {
    id: 'evidence',
    x: 790,
    y: 246,
    w: 194,
    h: 102,
    title: 'durable evidence',
    sub: 'status · transcript · traces',
    kind: 'secondary',
  },
  {
    id: 'provider',
    x: 790,
    y: 448,
    w: 194,
    h: 82,
    title: 'router + provider',
    sub: 'quality / e2e only',
    kind: 'secondary',
    tag: 'production',
  },
]

export const MAP_EDGES: MapEdge[] = [
  {
    id: 'launcher-integration',
    from: 'launcher',
    to: 'integration',
    d: 'M 176 280 C 198 232, 206 154, 228 118',
    label: 'PR regression',
    lx: 196,
    ly: 205,
  },
  {
    id: 'launcher-quality',
    from: 'launcher',
    to: 'quality',
    d: 'M 176 320 C 198 370, 206 448, 228 490',
    label: 'scheduled / RC',
    lx: 198,
    ly: 405,
  },
  {
    id: 'integration-harness',
    from: 'integration',
    to: 'harness',
    d: 'M 418 116 C 454 150, 482 214, 520 254',
    label: 'harness::send',
    lx: 470,
    ly: 188,
  },
  {
    id: 'quality-harness',
    from: 'quality',
    to: 'harness',
    d: 'M 418 490 C 454 452, 482 386, 520 346',
    label: 'harness::send',
    lx: 470,
    ly: 412,
  },
  {
    id: 'harness-scripted',
    from: 'harness',
    to: 'scripted',
    d: 'M 656 276 C 730 238, 724 118, 790 104',
    label: 'deterministic inference',
    lx: 726,
    ly: 194,
  },
  {
    id: 'harness-provider',
    from: 'harness',
    to: 'provider',
    d: 'M 656 326 C 730 366, 724 474, 790 490',
    label: 'real inference',
    lx: 726,
    ly: 410,
  },
  {
    id: 'harness-evidence',
    from: 'harness',
    to: 'evidence',
    d: 'M 656 300 L 790 297',
    label: 'public reads',
    lx: 724,
    ly: 288,
  },
  {
    id: 'evidence-integration',
    from: 'evidence',
    to: 'integration',
    d: 'M 824 246 C 720 174, 566 156, 418 132',
    label: 'exact contract oracle',
    lx: 610,
    ly: 166,
    dashed: true,
  },
  {
    id: 'evidence-quality',
    from: 'evidence',
    to: 'quality',
    d: 'M 824 348 C 720 422, 566 442, 418 478',
    label: 'outcome + budget oracle',
    lx: 610,
    ly: 438,
    dashed: true,
  },
]

export const MAP_INFO: Record<string, MapNodeInfo> = {
  launcher: {
    id: 'CLI / CI',
    kindLabel: 'track selector',
    role: 'chooses the gate, supplies its run configuration, and never changes the public harness entry point.',
    sections: [
      {
        heading: 'cadence',
        items: [
          { name: 'integration', desc: 'fast deterministic regression on pull requests' },
          { name: 'quality / e2e', desc: 'local, scheduled, on-demand, and release-candidate runs' },
        ],
      },
    ],
  },
  integration: {
    id: 'integration runner',
    kindLabel: 'deterministic track',
    role: 'allocates a fresh stack, controls router generations, grades exact contracts, and owns bounded teardown.',
    sections: [
      {
        heading: 'corpus',
        items: [
          { name: 'E2E-001 · E2E-002', desc: 'direct headless fixtures' },
          { name: 'UI-001 · UI-002', desc: 'Console/playground fixtures' },
        ],
      },
    ],
    install: 'harness/tests/e2e',
  },
  quality: {
    id: 'quality / e2e runner',
    kindLabel: 'real-model track',
    role: 'runs one unchanged corpus against one pinned subject and emits correctness plus raw benchmark dimensions.',
    sections: [
      {
        heading: 'subject identity',
        items: [
          { name: 'model · provider · prompt · skills' },
          { name: 'harness · workers · options', desc: 'resolved identities and digests' },
        ],
      },
    ],
  },
  harness: {
    id: 'harness',
    kindLabel: 'shared system under test',
    role: 'accepts harness::send, owns queue delivery and the durable turn loop, persists state, and emits lifecycle events.',
    sections: [
      {
        heading: 'public authority',
        items: [
          { name: 'harness::send · harness::status' },
          { name: 'session::messages', desc: 'durable active transcript path' },
        ],
      },
    ],
    note: 'neither track seeds private turn state or calls harness::turn as a continuation API.',
  },
  scripted: {
    id: 'scripted router',
    kindLabel: 'integration boundary',
    role: 'replaces only model inference and matches every expected router request before emitting ordered frames.',
    sections: [
      {
        heading: 'contract',
        items: [{ name: '6 router ids' }, { name: '12 request fields', desc: 'explicit per generation' }],
      },
    ],
  },
  provider: {
    id: 'production router + provider',
    kindLabel: 'quality boundary',
    role: 'executes the pinned real-model subject so quality, capability use, latency, tokens, and cost remain observable.',
    sections: [
      {
        heading: 'fixed per run',
        items: [
          { name: 'provider route + model' },
          { name: 'prompt strategy + bytes' },
          { name: 'skills + provider options' },
        ],
      },
    ],
  },
  evidence: {
    id: 'durable evidence',
    kindLabel: 'shared authority',
    role: 'supports exact contract assertions for integration and outcome, attribution, and budget assertions for quality.',
    sections: [
      {
        heading: 'shared',
        items: [
          { name: 'status · transcript · traces' },
          { name: 'domain state', desc: 'scenario-specific durable outcomes' },
        ],
      },
      {
        heading: 'quality composition',
        items: [{ name: 'session tree · metrics · triggered work' }],
      },
    ],
  },
}

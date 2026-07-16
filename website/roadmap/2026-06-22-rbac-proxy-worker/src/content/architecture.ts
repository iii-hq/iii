/* architecture — the system map (A4). nodes + edges + per-node datasheets. */
import type { MapEdge, MapNode, MapNodeInfo } from '@lib/components/diagrams/SystemMap'

/* coordinate space: 1030 x 600 (the SystemMap default viewBox) */

export const MAP_NODES: MapNode[] = [
  {
    id: 'clients',
    x: 36,
    y: 250,
    w: 168,
    h: 66,
    title: 'sdk workers',
    sub: 'bearer / api_key',
    kind: 'external',
    tag: 'untrusted',
  },
  {
    id: 'interceptor',
    x: 300,
    y: 120,
    w: 190,
    h: 68,
    title: 'interceptor',
    sub: 'gate · prefix · rewrite',
    kind: 'primary',
    tag: 'proxy',
  },
  { id: 'port', x: 300, y: 258, w: 190, h: 58, title: 'public port', sub: 'host:port', kind: 'primary', tag: 'proxy' },
  {
    id: 'channels',
    x: 300,
    y: 410,
    w: 190,
    h: 56,
    title: 'channel bridge',
    sub: '/ws/channels/{id}',
    kind: 'secondary',
    tag: 'proxy',
  },
  {
    id: 'control',
    x: 560,
    y: 410,
    w: 190,
    h: 56,
    title: 'control conn',
    sub: 'auth · hooks · caches',
    kind: 'primary',
    tag: 'proxy',
  },
  {
    id: 'policy',
    x: 560,
    y: 120,
    w: 190,
    h: 68,
    title: 'policy fns',
    sub: 'auth · mw · hooks',
    kind: 'external',
    tag: 'trusted',
  },
  {
    id: 'engine',
    x: 830,
    y: 252,
    w: 164,
    h: 68,
    title: 'engine',
    sub: ':49134 · no rbac',
    kind: 'primary',
    tag: 'trusted',
  },
  {
    id: 'config',
    x: 830,
    y: 410,
    w: 164,
    h: 56,
    title: 'configuration',
    sub: 'rbac-proxy config',
    kind: 'external',
    tag: 'trusted',
  },
]

export const MAP_EDGES: MapEdge[] = [
  {
    id: 'frames',
    from: 'clients',
    to: 'port',
    d: 'M 204 283 L 300 287',
    label: 'ws frames',
    lx: 252,
    ly: 276,
    dur: 1.8,
  },
  {
    id: 'pump',
    from: 'port',
    to: 'interceptor',
    d: 'M 395 258 L 395 188',
    label: 'per connection',
    lx: 403,
    ly: 226,
    anchor: 'start',
    dur: 1.6,
  },
  {
    id: 'upstream',
    from: 'interceptor',
    to: 'engine',
    d: 'M 490 150 C 700 150, 745 252, 830 268',
    label: 'upstream ws (rewritten)',
    lx: 660,
    ly: 168,
    dur: 2.2,
  },
  {
    id: 'chan-in',
    from: 'clients',
    to: 'channels',
    d: 'M 120 316 C 120 438, 210 438, 300 438',
    label: '/ws/channels/{id}',
    lx: 132,
    ly: 372,
    anchor: 'start',
    dashed: true,
    dur: 2.4,
  },
  {
    id: 'bridge',
    from: 'channels',
    to: 'engine',
    d: 'M 490 438 C 690 438, 770 360, 860 320',
    label: 'bridge 1:1',
    lx: 660,
    ly: 432,
    dur: 2.2,
  },
  {
    id: 'identity',
    from: 'control',
    to: 'engine',
    d: 'M 750 432 C 800 424, 850 360, 872 320',
    label: 'register · iii.trigger',
    lx: 812,
    ly: 410,
    anchor: 'start',
    dur: 2,
  },
  {
    id: 'invoke-policy',
    from: 'control',
    to: 'policy',
    d: 'M 655 410 L 655 188',
    label: 'auth / mw / hooks',
    lx: 663,
    ly: 304,
    anchor: 'start',
    dur: 1.8,
  },
  {
    id: 'cfg',
    from: 'control',
    to: 'config',
    d: 'M 750 438 L 830 438',
    label: 'config feed',
    lx: 790,
    ly: 430,
    dur: 1.8,
  },
]

export const MAP_INFO: Record<string, MapNodeInfo> = {
  clients: {
    id: 'sdk workers',
    kindLabel: 'untrusted',
    role: 'workers connecting over the iii worker protocol. each one authenticates on the public port; nothing reaches the engine until it does.',
    sections: [
      {
        heading: 'carries',
        items: [
          { name: 'authorization', desc: 'a bearer token, an api_key, whatever the auth function reads.' },
          { name: 'query + ip', desc: 'the auth function sees headers, query params, and the source ip.' },
        ],
      },
    ],
    note: 'the only thing on the untrusted network. it never sees the engine internal listener.',
  },
  interceptor: {
    id: 'interceptor',
    kindLabel: 'proxy · core',
    role: 'the per-connection gate. it parses every frame and decides: forward it, rewrite it, or answer it directly.',
    sections: [
      {
        heading: 'does',
        dotted: true,
        items: [
          { name: 'access resolution', desc: 'forbidden > allowed > carve-out > expose > deny, per call.' },
          { name: 'prefix apply / strip', desc: 'own registrations get a private namespace.' },
          { name: 'engine:: rewrite', desc: 'discovery results filtered to the caller.' },
          { name: 'synthesize FORBIDDEN', desc: 'a denied call never reaches the engine.' },
        ],
      },
    ],
    bullets: ['holds a ProxySession derived at upgrade for the life of the connection.'],
  },
  port: {
    id: 'public port',
    kindLabel: 'proxy · listener',
    role: 'the configurable public websocket port. the only door an untrusted worker knocks on.',
    sections: [
      {
        heading: 'serves',
        items: [
          { name: 'GET /', desc: 'the worker protocol route.' },
          { name: 'GET /ws/channels/{id}', desc: 'the channel bridge route.' },
        ],
      },
    ],
    note: 'host + port are hot-reloadable: a change rebinds the listener and keeps last-good on bind failure.',
    install: 'port: 49200',
  },
  channels: {
    id: 'channel bridge',
    kindLabel: 'proxy · relay',
    role: 'mounts /ws/channels/{id} on the proxy port and bridges frames 1:1 to the engine channel route.',
    sections: [
      {
        heading: 'relays',
        items: [
          { name: 'text ↔ text', desc: 'no buffering of its own; backpressure rides the socket.' },
          { name: 'binary ↔ binary', desc: 'otlp and channel payloads pass through.' },
          { name: 'close ↔ close', desc: 'close code and reason preserved.' },
        ],
      },
    ],
    note: 'the engine validates the access_key capability token; the proxy is a dumb relay and does not re-authenticate.',
  },
  control: {
    id: 'control conn',
    kindLabel: 'proxy · core',
    role: "the proxy's own worker identity. one persistent connection for everything that is not downstream data.",
    sections: [
      {
        heading: 'carries',
        items: [
          { name: 'rbac-proxy::status', desc: 'the one public health/identity probe.' },
          { name: 'auth · middleware · hooks', desc: 'the operator policy functions, by iii.trigger.' },
          { name: 'catalog + binding caches', desc: 'short-ttl reads the rewrites depend on.' },
        ],
      },
    ],
    bullets: ['fails closed: while it is down, new auth and gated calls are denied until it reconnects.'],
  },
  policy: {
    id: 'policy fns',
    kindLabel: 'trusted · operator',
    role: 'operator-registered engine functions: the auth function, optional middleware, and the three registration hooks. the proxy invokes them by id.',
    sections: [
      {
        heading: 'configured',
        items: [
          { name: 'auth_function_id', desc: 'runs once per upgrade; returns the session boundaries.' },
          { name: 'middleware_function_id', desc: 'wraps every allowed, non-engine:: call.' },
          { name: 'on_*_registration', desc: 'map or deny function / trigger registrations.' },
        ],
      },
    ],
    note: 'ordinary engine functions, registered from any sdk against the internal listener.',
  },
  engine: {
    id: 'engine',
    kindLabel: 'trusted · upstream',
    role: 'the trusted iii engine, running its internal listener with no rbac block. the proxy is the only public door in front of it.',
    sections: [
      {
        heading: 'unchanged',
        items: [
          { name: 'the wire protocol', desc: 'pure worker code against the existing Message protocol.' },
          { name: 'engine::* shapes', desc: 'the stable request/response shapes the proxy relies on.' },
        ],
      },
    ],
    bullets: ['may be local, remote, or managed; the proxy only needs an engine url.'],
    install: 'engine_url: ws://127.0.0.1:49134',
  },
  config: {
    id: 'configuration',
    kindLabel: 'trusted · worker',
    role: 'the configuration worker. holds the rbac-proxy config under its own id; no config.yaml is committed.',
    sections: [
      {
        heading: 'feeds',
        items: [
          { name: 'host · port · engine_url', desc: 'structural: a change rebinds the listener.' },
          { name: 'rbac.* · expose_worker_internals', desc: 'tuning: swapped into the live snapshot per connection.' },
        ],
      },
    ],
    note: 'on change it fires a trigger; the proxy re-fetches and decides rebind vs snapshot-swap by boot signature.',
  },
}

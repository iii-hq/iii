/* architecture — the system map (A4). nodes + edges + per-node datasheets. */
import type { MapEdge, MapNode, MapNodeInfo } from '@lib/components/diagrams/SystemMap'

/* coordinate space: 1030 x 600 (the SystemMap default viewBox) */

export const MAP_NODES: MapNode[] = [
  {
    id: 'worker',
    x: 36,
    y: 96,
    w: 186,
    h: 68,
    title: 'third-party worker',
    sub: 'console:script assets',
    kind: 'external',
    tag: 'author',
  },
  {
    id: 'build',
    x: 36,
    y: 258,
    w: 186,
    h: 58,
    title: 'console-build',
    sub: 'esbuild + scoped css',
    kind: 'optional',
    tag: 'dev loop',
  },
  {
    id: 'contentfn',
    x: 36,
    y: 420,
    w: 186,
    h: 58,
    title: 'content function',
    sub: 'state::ui-content',
    kind: 'secondary',
    tag: 'author',
  },
  {
    id: 'registry',
    x: 306,
    y: 96,
    w: 196,
    h: 68,
    title: 'trigger registry',
    sub: 'forward · park · replay',
    kind: 'primary',
    tag: 'engine',
  },
  {
    id: 'handler',
    x: 576,
    y: 96,
    w: 196,
    h: 68,
    title: 'trigger handler',
    sub: 'path-keyed · subscriber set',
    kind: 'primary',
    tag: 'console :3113',
  },
  {
    id: 'http',
    x: 576,
    y: 420,
    w: 196,
    h: 58,
    title: 'axum routes',
    sub: '/ui/*path · /vendor/*path',
    kind: 'primary',
    tag: 'console :3113',
  },
  {
    id: 'loader',
    x: 846,
    y: 96,
    w: 150,
    h: 68,
    title: 'ui loader',
    sub: 'dispose → import',
    kind: 'primary',
    tag: 'per tab',
  },
  {
    id: 'slots',
    x: 846,
    y: 420,
    w: 150,
    h: 58,
    title: 'slot registries',
    sub: '4 slot kinds',
    kind: 'secondary',
    tag: 'per tab',
  },
]

export const MAP_EDGES: MapEdge[] = [
  {
    id: 'reg',
    from: 'worker',
    to: 'registry',
    d: 'M 222 130 L 306 130',
    label: 'register',
    lx: 264,
    ly: 118,
    dur: 1.8,
  },
  {
    id: 'rebuild',
    from: 'build',
    to: 'worker',
    d: 'M 129 258 L 129 164',
    label: 're-register',
    lx: 139,
    ly: 216,
    anchor: 'start',
    dashed: true,
    dur: 1.6,
  },
  {
    id: 'fwd',
    from: 'registry',
    to: 'handler',
    d: 'M 502 130 L 576 130',
    label: 'forward',
    lx: 539,
    ly: 118,
    dur: 1.6,
  },
  {
    id: 'fetch',
    from: 'handler',
    to: 'contentfn',
    d: 'M 576 152 C 420 210, 320 400, 222 442',
    label: 'iii.trigger({path}) → {content}',
    lx: 356,
    ly: 288,
    dur: 2.4,
  },
  {
    id: 'push',
    from: 'handler',
    to: 'loader',
    d: 'M 772 130 L 846 130',
    label: 'push',
    lx: 809,
    ly: 118,
    dur: 1.6,
  },
  {
    id: 'subscribe',
    from: 'loader',
    to: 'registry',
    d: 'M 858 164 C 760 330, 600 330, 504 158',
    label: 'registerTrigger console:assets',
    lx: 690,
    ly: 332,
    dashed: true,
    dur: 2.6,
  },
  {
    id: 'get',
    from: 'loader',
    to: 'http',
    d: 'M 846 152 C 800 230, 792 350, 774 420',
    label: 'GET /ui/…?v=hash',
    lx: 806,
    ly: 296,
    anchor: 'start',
    dur: 2,
  },
  {
    id: 'setup',
    from: 'loader',
    to: 'slots',
    d: 'M 921 164 L 921 420',
    label: 'setup(host)',
    lx: 929,
    ly: 296,
    anchor: 'start',
    dur: 1.8,
  },
]

export const MAP_INFO: Record<string, MapNodeInfo> = {
  worker: {
    id: 'third-party worker',
    kindLabel: 'author-side',
    role: 'any worker on the bus. it ships console ui by registering one trigger per asset — type script or style, config {path} — over the sdk message path.',
    sections: [
      {
        heading: 'registers',
        items: [
          { name: 'script triggers', desc: 'esm assets, .js — pages, composer controls, renderers.' },
          { name: 'style triggers', desc: 'css assets, .css — scoped stylesheets.' },
        ],
      },
    ],
    note: 'sdk message path only: triggers gc on disconnect and replay on reconnect, so injected ui dies with its worker.',
  },
  build: {
    id: 'console-build',
    kindLabel: 'dev tooling · optional',
    role: '@iii-dev/console-build compiles ui/*.tsx with the five shared specifiers external and worker css scoped under [data-iii-ui]. the wire contract is bytes — any pipeline producing the same output is valid.',
    sections: [
      {
        heading: 'lints (build failures)',
        dotted: true,
        items: [
          { name: 'bundled react', desc: 'a forgotten external otherwise surfaces as "invalid hook call".' },
          { name: 'unscopable css', desc: '@font-face and selectors the scoper cannot contain.' },
        ],
      },
    ],
    install: 'npx @iii-dev/console-build --worker state --watch',
  },
  contentfn: {
    id: 'content function',
    kindLabel: 'author-side',
    role: 'one ordinary iii function per worker serves all its assets: {path} in, {content, content_type?} out. the console invokes it at registration and replay; browsers never wait on the bus.',
    sections: [
      {
        heading: 'contract',
        items: [
          { name: 'input', desc: '{ path } from the trigger config.' },
          { name: 'output', desc: '{ content, content_type? } — type defaults from the extension.' },
          { name: 'cap', desc: '8 MiB per asset; a slot component should be tens of KiB.' },
        ],
      },
    ],
  },
  registry: {
    id: 'trigger registry',
    kindLabel: 'engine · unchanged',
    role: 'existing machinery used as-is: forwards every console:* registration — worker assets and tab subscriptions alike — to the type owner, parks intents while the console is down, replays every live binding when the console re-registers the type.',
    sections: [
      {
        heading: 'lifecycle it provides',
        dotted: true,
        items: [
          { name: 'forward', desc: 'RegisterTrigger pushed to the owner live.' },
          { name: 'park', desc: 'RegisterTriggerOutcome::Deferred while the console is down.' },
          { name: 'replay', desc: 'every binding re-delivered on type re-registration.' },
          { name: 'gc', desc: 'message-path triggers removed on worker disconnect.' },
        ],
      },
    ],
    note: 'zero engine changes required. one optional nicety: an install-hint table entry for script/style.',
  },
  handler: {
    id: 'trigger handler',
    kindLabel: 'console worker · new',
    role: 'owns all three console:* types. validates paths, fetches content, hashes it, enforces "same path ⇒ override" with a two-key registry (by_path / by_id), and holds the subscriber set it pushes sync/set/delete events to.',
    sections: [
      {
        heading: 'serialization',
        items: [
          {
            name: 'one mpsc queue',
            desc: 'assets and subscriptions drain in ws-arrival order — a subscriber’s initial sync and later pushes are mutually ordered.',
          },
          { name: 'supersede', desc: 'prunes the old engine row so replay order never matters.' },
          { name: 'sync-on-subscribe', desc: 'a new subscriber immediately receives the full registry — no seed race.' },
        ],
      },
    ],
  },
  http: {
    id: 'axum routes',
    kindLabel: 'console worker · new',
    role: 'serves registered assets and the shared-dep shims from the port the spa already lives on. today’s router is exactly /, /assets/*path, /ws — the new paths are free.',
    sections: [
      {
        heading: 'routes',
        items: [
          { name: 'GET /ui/*path', desc: 'no-cache + ETag "<hash>" (304 on if-none-match).' },
          { name: 'GET /ui', desc: 'the manifest json, curl-friendly.' },
          { name: 'GET /vendor/*path', desc: 'the generated react shims.' },
        ],
      },
    ],
    note: 'never under /assets/* — that route is reserved for vite’s immutable content-hashed files.',
  },
  loader: {
    id: 'ui loader',
    kindLabel: 'console spa · new',
    role: 'per tab: register a console:assets trigger; the console answers with a sync push, then set/delete pushes follow. each event ⇒ dispose old cleanups lifo, import(?v=<hash>), run setup(host).',
    sections: [
      {
        heading: 'failure containment',
        items: [
          { name: 'import/setup failure', desc: 'console.error + one non-fatal toast naming the path.' },
          { name: 'render crash', desc: 'per-entry ErrorBoundary chip — never a white screen.' },
        ],
      },
    ],
    note: 'urls resolve against document.baseURI — subpath mounts keep working.',
  },
  slots: {
    id: 'slot registries',
    kindLabel: 'console spa · new',
    role: 'plain versioned maps read with useSyncExternalStore: composer.actions, functionCalls, pages, configForms. host components re-render exactly when a script (re)registers.',
    sections: [
      {
        heading: 'dispatch',
        items: [
          { name: 'functionCalls', desc: 'injected renderers first, then the 13 built-in families, then the json fallback.' },
          { name: 'pages', desc: '#/ext/<id> — outside the first-party View union, collision-free.' },
        ],
      },
    ],
  },
}

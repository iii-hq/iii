/* authoring — the author's chair: cli tracks + registration code (A3 + A13). */
import type { CliTrack } from '@lib/components/diagrams/CliPlayground'

export const CLI_TRACKS: CliTrack[] = [
  {
    id: 'golden',
    label: 'golden path',
    lines: [
      {
        cmd: 'cat ui/page.tsx',
        out: [
          'import { useState } from "react"          // the console\'s react',
          'export default function setup(host) {',
          '  host.pages.register({ id: "state-states", title: "states", … })',
          '}',
        ],
      },
      {
        cmd: 'npx @iii-dev/console-build --worker state',
        out: ['✓ dist/ui/page.js — esm, react external', '✓ dist/ui/styles.css — scoped under [data-iii-ui="state"]'],
      },
      {
        cmd: 'node src/index.js',
        fn: 'state::ui-content',
        out: ['✓ registered console:script state/page.js', '→ console fetched content · hash 9f2b6c01 · pushed to every tab'],
      },
      {
        cmd: 'open http://localhost:3113/#/ext/state-states',
        out: ['✓ page live in the nav — presence-gated by trigger gc'],
      },
    ],
  },
  {
    id: 'devloop',
    label: 'dev loop',
    lines: [
      {
        cmd: 'npx @iii-dev/console-build --worker state --watch',
        out: ['watching ui/ …'],
      },
      {
        cmd: ':w ui/page.tsx',
        out: ['✓ rebuilt dist/ui/page.js', '→ watchUi re-registers state/page.js, then unregisters the old handle'],
      },
      {
        out: ['→ console: supersede old trigger id · stream::set {path, kind, hash}'],
      },
      {
        out: ['✓ every open tab: dispose → import(?v=<new hash>) → setup(host)', '  no console rebuild. no tab refresh.'],
      },
    ],
  },
]

export const REGISTRATION_CODE = `// the content function — one per worker, serves all its assets
iii.registerFunction('state::ui-content', async ({ path }) => {
  const file = ASSETS[path]            // 'state/page.js' → 'dist/ui/page.js'
  if (!file) throw new Error(\`unknown ui asset: \${path}\`)
  return { content: await readFile(file, 'utf8') }
})

// one trigger per asset — registration IS deployment
const trigger = iii.registerTrigger({
  type: 'console:script',              // or 'console:style'
  function_id: 'state::ui-content',
  config: { path: 'state/page.js' },
})
// trigger.unregister() removes it; worker disconnect does it implicitly`

export const CONTENT_FN_ROWS = [
  { name: 'input', type: '{ path }', desc: 'the path from the trigger config.' },
  { name: 'output', type: '{ content, content_type? }', desc: 'content_type defaults from the extension.' },
  { name: 'errors', type: 'bus error', desc: 'fails the fetch; at live registration the registration is rejected.' },
  { name: 'cap', type: '8 MiB', desc: 'per asset — a slot component should be tens of KiB.' },
] as const

export const DEV_LOOP_NOTE =
  'registration stays in the worker process, deliberately: message-path triggers die with the connection that registered them. node workers embed watchUi(); rust workers run the cli and watch dist/ui with the notify crate.'

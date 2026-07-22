/* one react — the funnel data + import map + shim facts (A8 + A13). */
import type { FunnelPath } from '@lib/components/diagrams/Funnel'

export const REACT_PATHS: FunnelPath[] = [
  { id: 'react', label: 'react' },
  { id: 'react-dom', label: 'react-dom' },
  { id: 'react-dom-client', label: 'react-dom/client' },
  { id: 'jsx-runtime', label: 'react/jsx-runtime' },
  { id: 'console-api', label: '@iii/console' },
]

export const REACT_TARGET = {
  label: 'window.__III_CONSOLE__',
  sub: 'the spa’s bundled react 19.2.6 + the console api',
}

export const REACT_REJECT = {
  label: 'a second bundled react',
  desc: 'a forgotten --external ships one; its hooks resolve against a never-installed dispatcher: "invalid hook call". console-build fails the build instead.',
}

export const IMPORT_MAP = `<script type="importmap">
{ "imports": {
    "react":             "./vendor/react.js",
    "react-dom":         "./vendor/react-dom.js",
    "react-dom/client":  "./vendor/react-dom-client.js",
    "react/jsx-runtime": "./vendor/jsx-runtime.js",
    "@iii/console":      "./vendor/console-api.js"
} }
</script>`

export const SHIM_ROWS = [
  {
    name: 'generated, not hand-pinned',
    type: 'build time',
    desc: 'shim export lists come from the installed packages’ production export sets; a ci assertion keeps each shim ⊇ the real module.',
  },
  {
    name: 'why it matters',
    type: 'esm',
    desc: 'named imports are checked at resolution: one omitted export inside any bundled dependency is a whole-script SyntaxError at import() time.',
  },
  {
    name: 'static map, single',
    type: 'index.html',
    desc: 'placed before the vite module script and any modulepreload; no dependency on multiple/dynamic import-map support.',
  },
  {
    name: 'subpath caveat',
    type: 'esbuild',
    desc: '--external:react-dom also externalizes react-dom/server; only these five specifiers exist in the map — anything else fails at import time.',
  },
] as const

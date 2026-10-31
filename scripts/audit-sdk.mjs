#!/usr/bin/env node
// Supply-chain gate for the Node SDK packages.
// Fails on any advisory whose dependency path starts in sdk/packages/node/*;
// advisories reachable only from website/docs are reported, not gated
// (they are tracked separately).
import { execFileSync } from 'node:child_process'

// Dev-only paths whose advisory has no fix the dependent accepts. Each is
// reported, not gated, and names the advisory and the path suffix it covers.
const accepted = [
  // typedoc 0.28.20 needs minimatch 10 (it calls Minimatch#matchOne, which 11
  // makes private) and only builds the API reference from our own globs.
  { advisory: 'GHSA-g3hv-75wg-m249', path: '>typedoc>minimatch' },
]

let out
try {
  out = execFileSync('pnpm', ['audit', '--json'], { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 })
} catch (e) {
  // pnpm audit exits non-zero whenever it finds anything; the JSON is still on stdout.
  if (!e.stdout) throw e
  out = e.stdout
}

const { advisories = {} } = JSON.parse(out)
const sdk = new Set()
const others = {}
const known = new Set()
for (const a of Object.values(advisories)) {
  const id = a.github_advisory_id ?? a.url?.split('/').pop()
  for (const f of a.findings ?? []) {
    for (const p of f.paths ?? []) {
      const root = p.split('>')[0]
      if (accepted.some(x => x.advisory === id && p.endsWith(x.path))) {
        known.add(`${a.severity.padEnd(8)} ${a.module_name}@${f.version}  ${p}  ${a.url}`)
      } else if (root.startsWith('sdk__packages__node__')) {
        sdk.add(`${a.severity.padEnd(8)} ${a.module_name}@${f.version}  ${p}  ${a.url}`)
      } else {
        const key = `${root} ${a.severity}`
        others[key] = (others[key] ?? 0) + 1
      }
    }
  }
}

console.log('Advisory paths outside the SDK packages (report only):')
for (const [k, v] of Object.entries(others).sort()) console.log(`  ${k}: ${v}`)

if (known.size) {
  console.log('\nAccepted dev-only advisory paths (report only):')
  for (const line of [...known].sort()) console.log(`  ${line}`)
}

if (sdk.size) {
  console.error(`\n${sdk.size} advisory path(s) reach the SDK packages:`)
  for (const line of [...sdk].sort()) console.error(`  ${line}`)
  process.exit(1)
}
console.log('\nSDK packages: no advisories.')

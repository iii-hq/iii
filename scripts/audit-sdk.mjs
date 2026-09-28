#!/usr/bin/env node
// Supply-chain gate for the Node SDK packages.
// Fails on any advisory whose dependency path starts in sdk/packages/node/*;
// advisories reachable only from website/console/docs are reported, not gated
// (they are tracked separately).
import { execFileSync } from 'node:child_process'

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
for (const a of Object.values(advisories)) {
  for (const f of a.findings ?? []) {
    for (const p of f.paths ?? []) {
      const root = p.split('>')[0]
      if (root.startsWith('sdk__packages__node__')) {
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

if (sdk.size) {
  console.error(`\n${sdk.size} advisory path(s) reach the SDK packages:`)
  for (const line of [...sdk].sort()) console.error(`  ${line}`)
  process.exit(1)
}
console.log('\nSDK packages: no advisories.')

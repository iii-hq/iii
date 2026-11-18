#!/usr/bin/env node
// Supply-chain gate for the Node SDK packages.
// Fails on any advisory whose dependency path starts in sdk/packages/node/*;
// advisories reachable only from website/docs are reported, not gated
// (they are tracked separately).
import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { readFileSync } from 'node:fs'

// No upstream fix exists for GHSA-fc5q-39xc-vjpf. Report the locally patched
// versions only when the lockfile still records the exact patch integrity.
// Keep the advisory visible; do not suppress unpatched or new versions.
const locallyPatched = new Set()
const lock = readFileSync(new URL('../pnpm-lock.yaml', import.meta.url), 'utf8')
for (const version of ['2.3.2', '4.0.7']) {
  const patchPath = `patches/picomatch@${version}.patch`
  const hash = createHash('sha256')
    .update(readFileSync(new URL(`../${patchPath}`, import.meta.url)))
    .digest('hex')
  if (
    lock.includes(`  picomatch@${version}:\n    hash: ${hash}\n    path: ${patchPath}`) &&
    lock.includes(`  picomatch@${version}(patch_hash=${hash}): {}`) &&
    !lock.includes(`  picomatch@${version}: {}`)
  ) {
    locallyPatched.add(version)
  }
}

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
      const patched = id === 'GHSA-fc5q-39xc-vjpf' &&
        a.module_name === 'picomatch' && locallyPatched.has(f.version)
      if (patched) {
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
  console.log('\nLocally patched advisory paths (report only):')
  for (const line of [...known].sort()) console.log(`  ${line}`)
}

if (sdk.size) {
  console.error(`\n${sdk.size} advisory path(s) reach the SDK packages:`)
  for (const line of [...sdk].sort()) console.error(`  ${line}`)
  process.exit(1)
}
console.log('\nSDK packages: no unmitigated advisories.')

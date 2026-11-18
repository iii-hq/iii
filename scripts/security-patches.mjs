import { createHash } from 'node:crypto'
import { readFileSync } from 'node:fs'

// Reviewed independently of pnpm-lock.yaml. Updating these digests requires
// pnpm test:security, which CI runs after installing the patched dependencies.
// When retiring the patches for an upstream fix, remove this audit exception
// and update the regression to test the resolved upstream versions for bounded
// matching, without requiring our local literal fallback or retired versions.
export const reviewedPicomatch = {
  '2.3.2': {
    patch: '31f01a27a91e8cfc038cf7ea4f33ac29abb91e069973254f49fdd1c7366e594e',
    parser: '4c19c6e839dab9d176b1e9574d84d065c6b17e4e7d7776a48290e336803dd4a0',
  },
  '4.0.7': {
    patch: '72be261e61595e4ee40ad11dc1da11e3eec872cdf70e0bb70ad8e9e63f396f9a',
    parser: '8652d8acab8d3bc0fc436a28d826c4b19d6f9c272c94105024bb771f59af32c6',
  },
}

export function normalizeLines(value) {
  return String(value).replace(/\r\n/g, '\n')
}

export function digest(value) {
  return createHash('sha256').update(normalizeLines(value)).digest('hex')
}

export function validatePicomatchPatches({
  lock = readFileSync(new URL('../pnpm-lock.yaml', import.meta.url), 'utf8'),
  readPatch = name => readFileSync(new URL(`../${name}`, import.meta.url), 'utf8'),
} = {}) {
  const normalized = normalizeLines(lock)
  const verified = new Set()
  for (const [version, reviewed] of Object.entries(reviewedPicomatch)) {
    const patchPath = `patches/picomatch@${version}.patch`
    const hash = digest(readPatch(patchPath))
    if (
      hash !== reviewed.patch ||
      !normalized.includes(`  picomatch@${version}:\n    hash: ${hash}\n    path: ${patchPath}\n`) ||
      !normalized.includes(`  picomatch@${version}(patch_hash=${hash}): {}`) ||
      normalized.includes(`  picomatch@${version}: {}`)
    ) {
      throw new Error(`Unreviewed or missing Picomatch ${version} mitigation; review digests and run pnpm test:security`)
    }
    verified.add(version)
  }
  return verified
}

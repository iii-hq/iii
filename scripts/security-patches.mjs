import { createHash } from 'node:crypto'
import { readFileSync } from 'node:fs'
import { parse } from 'yaml'

// Reviewed independently of pnpm-lock.yaml. Updating these digests requires
// pnpm test:security, which CI runs after installing the patched dependencies.
// When retiring the patches for an upstream fix, remove this audit exception
// and update the regression to test the resolved upstream versions for bounded
// matching, without requiring our local literal fallback or retired versions.
export const reviewedPicomatch = {
  '2.3.2': {
    patch: '94e816fec0deab10ae461de4a157103a29093fc6a5147fb09fa93f486e2f238d',
    parser: 'e3c751e71fa14a494d88979b614687b3e4fcac386f4606e2d659b04b90cf80e0',
  },
  '4.0.7': {
    patch: '7773ce638ea70b87d0f2af0c08e1c151975d9a1cd81cdc95a21890af7166380f',
    parser: '34c2e08720c8a37d79f3b6f4e89f6825e46a908803949741137e70638ca0b5ed',
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
  const data = parse(normalizeLines(lock))
  const snapshots = data?.snapshots ?? {}
  const verified = new Set()
  for (const [version, reviewed] of Object.entries(reviewedPicomatch)) {
    const patchPath = `patches/picomatch@${version}.patch`
    const hash = digest(readPatch(patchPath))
    const record = data?.patchedDependencies?.[`picomatch@${version}`]
    if (
      hash !== reviewed.patch ||
      record?.hash !== hash || record?.path !== patchPath ||
      !Object.hasOwn(snapshots, `picomatch@${version}(patch_hash=${hash})`) ||
      Object.hasOwn(snapshots, `picomatch@${version}`)
    ) {
      throw new Error(`Unreviewed or missing Picomatch ${version} mitigation; review digests and run pnpm test:security`)
    }
    verified.add(version)
  }
  return verified
}

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
    patch: '17466b0fe3f3da769e274eff6b6fc11efae284318553662c68cbcabc0915817a',
    parser: 'f548a9384788cc0f7a3720624ea088519a46ea5325cdd2fb9b9fbace3809a843',
  },
  '4.0.7': {
    patch: '8c8e8881c5a79cc9f1116ffd2f5d521602ffdc9bf5f90a09230c44b182caa85a',
    parser: 'c48630d5b1ace5836c67c556d1d29741cabb03fcb52f18f91ff9b1e062fbe5dc',
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

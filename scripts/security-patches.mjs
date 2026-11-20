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
    patch: '816490c068ab2dbd1c9d0d1a809d6b1aa2ba687bba07ddd4f0622f901da0fccc',
    parser: '7bc1b9c1d7025547db6fedf572056034c65996e4b7a1081eb782308ebc48bfaf',
  },
  '4.0.7': {
    patch: '711ccf8ab59a8bb16857ffa5f4d519a65cad45e4e674bd6a4f74fab1b221da39',
    parser: 'e9d000b9f281a2783b06826293b500ad98cf3e14d5cb60abe25860559d91b129',
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

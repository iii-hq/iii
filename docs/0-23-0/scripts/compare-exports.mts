/**
 * compare-exports.mts — cross-SDK export parity matrix.
 *
 * Extracts the public export surface of each SDK (Node `iii-sdk`, Python `iii`,
 * Rust `iii_sdk`, Browser `iii-browser-sdk`), matches symbols across them by a
 * casing/separator-insensitive key (so `registerWorker` ≍ `register_worker`),
 * and writes a CSV that flags mismatches. Compares the EXPORTS themselves only —
 * it does not care whether a symbol documents params/examples.
 *
 * Browser is treated as secondary: rows are driven by the union of the three
 * primary SDKs (Node/Python/Rust); browser only fills its column when it matches
 * a primary row, and browser-only exports are listed in a separate trailing
 * block so they don't punch holes through the main matrix.
 *
 * Usage:
 *   pnpm tsx docs/next/scripts/compare-exports.mts <out.csv> [flags]
 *     <out.csv>             output path (absolute or relative to CWD)
 *     --use-existing        don't re-extract; use the JSON dumps already on disk
 *     --include-deprecated  include `@deprecated` / `#[deprecated]` exports
 *
 * Extraction (skipped with --use-existing) shells out to the same tools the
 * docs pipeline uses: TypeDoc (Node/Browser), griffe (Python), nightly rustdoc
 * (Rust). Run from the repo root.
 */
import { execSync } from 'node:child_process'
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, isAbsolute, resolve } from 'node:path'

const ROOT = process.cwd()

// ── args ──────────────────────────────────────────────────────────────────
const args = process.argv.slice(2)
const flags = new Set(args.filter(a => a.startsWith('--')))
const positional = args.filter(a => !a.startsWith('--'))
const USE_EXISTING = flags.has('--use-existing')
const INCLUDE_DEPRECATED = flags.has('--include-deprecated')
if (positional.length !== 1 || flags.has('--help')) {
  console.error('Usage: pnpm tsx docs/next/scripts/compare-exports.mts <out.csv> [--use-existing] [--include-deprecated]')
  process.exit(positional.length === 1 ? 0 : 1)
}
const OUT = isAbsolute(positional[0]) ? positional[0] : resolve(ROOT, positional[0])

// ── model ─────────────────────────────────────────────────────────────────
type Sdk = 'node' | 'python' | 'rust' | 'browser'
const PRIMARY: Sdk[] = ['node', 'python', 'rust']
interface Export { name: string; subpath: string; kind: string }

/** Casing/separator-insensitive match key: registerWorker ≍ register_worker. */
function key(name: string): string {
  return name.replace(/[_-]/g, '').toLowerCase()
}

/**
 * Normalize a subpath for cross-language comparison: strip the package prefix
 * and unify separators, so `iii-sdk/channel` ≍ `iii.channel` ≍ `iii_sdk::channel`
 * all collapse to `channel` (and a root export collapses to '').
 */
function normPath(subpath: string): string {
  return subpath
    .replace(/^(iii-browser-sdk|iii-sdk|iii_sdk|iii)\b/, '')
    .replace(/^[/.:]+/, '')
    .replace(/[/:]+/g, '.')
    .toLowerCase()
}

// ── TypeDoc (Node + Browser) ────────────────────────────────────────────────
const TD_KIND: Record<number, string> = {
  8: 'enum', 32: 'const', 64: 'function', 128: 'type', 256: 'type', 2097152: 'type', 4194304: 'reference',
}
function tdDeprecated(c: any): boolean {
  const tags = c?.comment?.blockTags ?? c?.signatures?.[0]?.comment?.blockTags
  return !!tags?.some((t: any) => t.tag === '@deprecated')
}
function extractTypedoc(jsonPath: string, pkg: string): Export[] {
  const raw = JSON.parse(readFileSync(jsonPath, 'utf-8'))
  const out: Export[] = []
  for (const mod of raw.children ?? []) {
    const sub = String(mod.name).replace(/\/index$/, '')
    const subpath = sub === 'index' ? pkg : `${pkg}/${sub}`
    for (const c of mod.children ?? []) {
      if (!INCLUDE_DEPRECATED && tdDeprecated(c)) continue
      out.push({ name: c.name, subpath, kind: TD_KIND[c.kind] ?? 'other' })
    }
  }
  return out
}

// ── griffe (Python) ──────────────────────────────────────────────────────────
// griffe dumps the whole package; the public import paths are the root barrel
// plus this fixed set of submodules (the rest are backing/impl modules).
const PY_PUBLIC_SUBMODULES = new Set([
  'channel', 'errors', 'trigger', 'runtime', 'engine', 'protocol', 'internal', 'utils', 'stream', 'state', 'helpers',
])
function griffeResolve(obj: any, index: Map<string, any>, depth = 0): any {
  if (!obj || depth > 10) return obj
  if (obj.kind === 'alias' && obj.target_path) return griffeResolve(index.get(obj.target_path), index, depth + 1)
  return obj
}
function griffeKind(o: any): string {
  if (!o) return 'other'
  if (o.kind === 'class') return 'type'
  if (o.kind === 'function') return 'function'
  if (o.kind === 'attribute') return 'const'
  return o.kind ?? 'other'
}
/** A module's `__all__` as a name set, or null if it declares none. When
 * present it's authoritative — it excludes names a module merely imports for
 * use (e.g. `iii.helpers` imports `IIIClient` for a signature but doesn't export it). */
function griffeAll(moduleObj: any): Set<string> | null {
  const elements = moduleObj?.members?.__all__?.value?.elements
  if (!Array.isArray(elements)) return null
  return new Set(elements.map((el: any) => (typeof el === 'string' ? el.replace(/^['"]|['"]$/g, '') : (el?.value ?? el?.name ?? ''))))
}
function extractGriffe(jsonPath: string): Export[] {
  const raw = JSON.parse(readFileSync(jsonPath, 'utf-8'))
  const root = raw.iii ?? raw
  const index = new Map<string, any>()
  const walk = (o: any, path: string) => {
    index.set(path, o)
    for (const [n, m] of Object.entries(o.members ?? {})) walk(m, `${path}.${n}`)
  }
  walk(root, 'iii')

  const out: Export[] = []
  const collect = (moduleObj: any, subpath: string) => {
    const allowed = griffeAll(moduleObj) // honor __all__ when declared
    for (const [name, member] of Object.entries<any>(moduleObj.members ?? {})) {
      if (name.startsWith('_')) continue
      if (member.kind === 'module') continue
      if (allowed && !allowed.has(name)) continue // not part of the module's public export list
      const r = griffeResolve(member, index)
      if (!r) continue
      // skip re-imports from outside the package (typing.Any, pydantic, …)
      if (r.path && !r.path.startsWith('iii.') && r.path !== 'iii') continue
      if (!INCLUDE_DEPRECATED && r.deprecated) continue
      out.push({ name, subpath, kind: griffeKind(r) })
    }
  }
  collect(root, 'iii')
  for (const [name, member] of Object.entries<any>(root.members ?? {})) {
    if (member.kind === 'module' && PY_PUBLIC_SUBMODULES.has(name)) collect(member, `iii.${name}`)
  }
  return out
}

// ── rustdoc (Rust) ────────────────────────────────────────────────────────────
const RD_EXPORT_KINDS = new Set(['struct', 'enum', 'trait', 'function', 'type_alias', 'constant', 'static', 'macro'])
const RD_KIND: Record<string, string> = {
  struct: 'type', enum: 'type', trait: 'trait', type_alias: 'type', function: 'function', constant: 'const', static: 'const', macro: 'macro',
}
// rustdoc only records an item's *definition* path (e.g. `iii_sdk::channels`),
// never the public Stage-1 re-export path (`iii_sdk::channel`). So walk the
// public module tree from the crate root, following `pub use`, and prefer the
// clean submodule path over the impl module it's defined in.
const RD_IMPL_MODULES = new Set(['channels', 'triggers', 'structs', 'types', 'iii', 'builtin_triggers', 'error', 'stream_provider', 'helpers'])
function extractRustdoc(jsonPath: string): Export[] {
  const data = JSON.parse(readFileSync(jsonPath, 'utf-8'))
  const index: Record<string, any> = data.index ?? {}
  const reach = new Map<number, Set<string>>() // itemId -> public module paths it's reachable at
  const names = new Map<number, string>()
  const kinds = new Map<number, string>()
  const visited = new Set<number>()
  const add = (id: number, modPath: string, name: string, kind: string) => {
    if (!reach.has(id)) reach.set(id, new Set())
    reach.get(id)!.add(modPath)
    names.set(id, name)
    kinds.set(id, kind)
  }
  const walk = (modId: number, modPath: string) => {
    if (visited.has(modId)) return
    visited.add(modId)
    for (const cid of index[modId]?.inner?.module?.items ?? []) {
      const c = index[cid]
      if (!c) continue
      const kind = Object.keys(c.inner)[0]
      if (kind === 'module') {
        if (c.name) walk(cid, `${modPath}::${c.name}`)
      } else if (kind === 'use') {
        if (!INCLUDE_DEPRECATED && c.deprecation) continue
        const u = c.inner.use
        if (u?.id == null || !u.name || u.is_glob) continue
        const t = index[u.id]
        const tk = t && Object.keys(t.inner)[0]
        if (t && RD_EXPORT_KINDS.has(tk)) add(u.id, modPath, u.name, tk)
      } else if (RD_EXPORT_KINDS.has(kind)) {
        if (!INCLUDE_DEPRECATED && c.deprecation) continue
        if (c.name) add(cid, modPath, c.name, kind)
      }
    }
  }
  walk(data.root, 'iii_sdk')

  // Emit EVERY public path (so cross-SDK alignment can intersect path sets),
  // but drop impl-module paths in favor of the clean submodule re-export; keep
  // an impl path only when there's no cleaner public path (rust-only types).
  const out: Export[] = []
  for (const [id, paths] of reach) {
    const clean = [...paths].filter(p => {
      const sub = p.replace(/^iii_sdk(::)?/, '')
      return !sub || !RD_IMPL_MODULES.has(sub.split('::')[0])
    })
    for (const p of clean.length ? clean : [...paths]) {
      out.push({ name: names.get(id)!, subpath: p, kind: RD_KIND[kinds.get(id)!] ?? 'other' })
    }
  }
  return out
}

// ── extraction (toolchain shell-outs) ──────────────────────────────────────
interface Target {
  sdk: Sdk
  json: string
  extract: () => void
  parse: (json: string) => Export[]
}
const TARGETS: Target[] = [
  {
    sdk: 'node', json: resolve(ROOT, 'sdk/packages/node/iii/api-docs.json'),
    extract: () => execSync('pnpm --filter iii-sdk docs:json', { cwd: ROOT, stdio: 'pipe' }),
    parse: j => extractTypedoc(j, 'iii-sdk'),
  },
  {
    sdk: 'python', json: resolve(ROOT, 'sdk/packages/python/iii/api-docs.json'),
    extract: () => execSync('uv run --with griffe griffe dump iii -d google > api-docs.json', { cwd: resolve(ROOT, 'sdk/packages/python/iii'), stdio: 'pipe', shell: '/bin/bash' }),
    parse: extractGriffe,
  },
  {
    sdk: 'rust', json: resolve(ROOT, 'target/doc/iii_sdk.json'),
    extract: () => execSync('cargo +nightly rustdoc -p iii-sdk --all-features -- -Z unstable-options --output-format json', { cwd: ROOT, stdio: 'pipe' }),
    parse: extractRustdoc,
  },
  {
    sdk: 'browser', json: resolve(ROOT, 'sdk/packages/node/iii-browser/api-docs.json'),
    extract: () => execSync('pnpm --filter iii-browser-sdk docs:json', { cwd: ROOT, stdio: 'pipe' }),
    parse: j => extractTypedoc(j, 'iii-browser-sdk'),
  },
]

const surfaces = new Map<Sdk, Export[]>()
for (const t of TARGETS) {
  if (!USE_EXISTING) {
    try {
      console.log(`[extract] ${t.sdk}…`)
      t.extract()
    } catch (e) {
      console.warn(`  [warn] extraction failed for ${t.sdk}; falling back to existing JSON. (${(e as Error).message.split('\n')[0]})`)
    }
  }
  if (!existsSync(t.json)) {
    console.warn(`  [warn] no JSON for ${t.sdk} at ${t.json} — column will be empty.`)
    surfaces.set(t.sdk, [])
    continue
  }
  surfaces.set(t.sdk, t.parse(t.json))
}

// ── build matrix ────────────────────────────────────────────────────────────
// A symbol may be exported from more than one path within a single SDK (e.g.
// re-exported at the root AND from its submodule). Aggregate ALL paths, so
// alignment is judged by whether SDKs *share* a path — not by an arbitrary pick.
interface Agg { name: string; subpaths: string[]; norms: Set<string>; kind: string }
function aggregate(exports: Export[]): Map<string, Agg> {
  const m = new Map<string, Agg>()
  for (const e of exports) {
    const k = key(e.name)
    let a = m.get(k)
    if (!a) { a = { name: e.name, subpaths: [], norms: new Set(), kind: e.kind }; m.set(k, a) }
    if (!a.subpaths.includes(e.subpath)) a.subpaths.push(e.subpath)
    a.norms.add(normPath(e.subpath))
    if ((a.kind === 'reference' || a.kind === 'other') && e.kind && e.kind !== 'reference') a.kind = e.kind
  }
  return m
}
const byKeyBySdk = new Map<Sdk, Map<string, Agg>>()
for (const [sdk, exps] of surfaces) byKeyBySdk.set(sdk, aggregate(exps))
const get = (sdk: Sdk, k: string) => byKeyBySdk.get(sdk)?.get(k)

// primary-union keys drive the rows; browser is a secondary column.
const primaryKeys = new Set<string>()
for (const sdk of PRIMARY) for (const k of byKeyBySdk.get(sdk)?.keys() ?? []) primaryKeys.add(k)

// Display: prefer the shared path (so aligned rows read cleanly), else the
// SDK's cleanest own path (submodule over the bare root).
const subForNorm = (a: Agg, norm: string) => a.subpaths.find(s => normPath(s) === norm)
const bestSub = (a: Agg) => [...a.subpaths].sort((x, y) => (normPath(x) === '' ? 1 : 0) - (normPath(y) === '' ? 1 : 0))[0] ?? ''
const cleanestNorm = (norms: string[]) => [...norms].sort((x, y) => (x === '' ? 1 : 0) - (y === '' ? 1 : 0) || x.localeCompare(y))[0]

interface Row {
  canonical: string; kind: string
  node: string; nodePath: string; python: string; pythonPath: string
  rust: string; rustPath: string; browser: string; browserPath: string
  status: string
}
const rows: Row[] = []
for (const k of primaryKeys) {
  const e = (sdk: Sdk) => get(sdk, k)
  const present = PRIMARY.filter(s => e(s))
  const missing = PRIMARY.filter(s => !e(s))
  const kinds = (['node', 'python', 'rust', 'browser'] as Sdk[]).map(s => e(s)?.kind).filter(Boolean) as string[]
  const anyKind = kinds.find(x => x !== 'reference' && x !== 'other') ?? kinds[0] ?? ''
  // Path-aligned iff the present primary SDKs share at least one normalized path.
  let shared: Set<string> | null = null
  for (const s of present) { const ns = e(s)!.norms; shared = shared === null ? new Set(ns) : new Set([...shared].filter(x => ns.has(x))) }
  let status: string
  let displayNorm: string | null = null
  if (missing.length) status = `missing: ${missing.join(', ')}`
  else if (shared && shared.size) { status = 'all'; displayNorm = cleanestNorm([...shared]) }
  else status = 'path mismatch'
  const showPath = (sdk: Sdk) => {
    const a = e(sdk)
    if (!a) return ''
    return (displayNorm != null ? subForNorm(a, displayNorm) : undefined) ?? bestSub(a)
  }
  rows.push({
    canonical: k, kind: anyKind,
    node: e('node')?.name ?? '', nodePath: showPath('node'),
    python: e('python')?.name ?? '', pythonPath: showPath('python'),
    rust: e('rust')?.name ?? '', rustPath: showPath('rust'),
    browser: e('browser')?.name ?? '', browserPath: showPath('browser'),
    status,
  })
}
// mismatches (missing or path) first, then full matches; alphabetical within each.
rows.sort((a, b) => {
  const am = a.status !== 'all', bm = b.status !== 'all'
  if (am !== bm) return am ? -1 : 1
  return a.canonical.localeCompare(b.canonical)
})

// browser-only exports: a compact trailing block (keeps gaps out of the matrix).
const browserOnly = [...(byKeyBySdk.get('browser')?.entries() ?? [])]
  .filter(([k]) => !primaryKeys.has(k))
  .map(([, a]) => a)
  .sort((a, b) => a.name.localeCompare(b.name))

// ── write CSV ────────────────────────────────────────────────────────────────
const cell = (v: string) => (/[",\n]/.test(v) ? `"${v.replace(/"/g, '""')}"` : v)
const HEADER = ['canonical', 'kind', 'node', 'node_path', 'python', 'python_path', 'rust', 'rust_path', 'browser', 'browser_path', 'status']
const lines: string[] = []
lines.push(HEADER.join(','))
for (const r of rows) {
  lines.push([r.canonical, r.kind, r.node, r.nodePath, r.python, r.pythonPath, r.rust, r.rustPath, r.browser, r.browserPath, r.status].map(cell).join(','))
}
if (browserOnly.length) {
  lines.push('')
  lines.push(cell('# browser-only exports (no Node/Python/Rust equivalent)'))
  lines.push(HEADER.join(','))
  for (const a of browserOnly) {
    lines.push([key(a.name), a.kind, '', '', '', '', '', '', a.name, bestSub(a), 'browser-only'].map(cell).join(','))
  }
}
mkdirSync(dirname(OUT), { recursive: true })
writeFileSync(OUT, lines.join('\n') + '\n', 'utf-8')

// ── summary ──────────────────────────────────────────────────────────────────
const matched = rows.filter(r => r.status === 'all').length
const mism = rows.length - matched
const counts = (['node', 'python', 'rust', 'browser'] as Sdk[]).map(s => `${s}=${byKeyBySdk.get(s)?.size ?? 0}`).join('  ')
console.log(`\n[compare-exports] surfaces: ${counts}`)
console.log(`[compare-exports] primary rows: ${rows.length} (${matched} in all 3, ${mism} mismatched), browser-only: ${browserOnly.length}`)
console.log(`[compare-exports] wrote ${OUT}`)

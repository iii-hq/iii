import { readFileSync } from 'node:fs'
import { EXPAND_PARAMS_RE, INTERNAL_RE, isInternalDoc, parseExpandMarker } from '../types.mjs'
import type { FunctionDoc, ModuleDoc, ParamDoc, SdkDoc, TypeDoc, TypeGroup } from '../types.mjs'

interface RustDocIndex {
  root: number
  format_version: number
  paths: Record<string, { crate_id: number; path: string[]; kind: string }>
  index: Record<string, RustDocItem>
}

interface RustDocItem {
  id: number
  crate_id: number
  name: string | null
  docs?: string
  inner: Record<string, any>
  visibility?: string
  /** Present (non-null) when the item carries `#[deprecated]` — e.g. old 0.19
   * path re-exports kept for back-compat. */
  deprecation?: { since?: string; note?: string } | null
}

const MAIN_STRUCT = 'IIIClient'

const METHOD_EXCLUDE = new Set([
  'new',
  'with_metadata',
  'address',
  'set_metadata',
  'set_headers',
  'set_otel_config',
  'connect',
])

const SIMPLIFY_PATHS: Record<string, string> = {
  'std::collections::HashMap': 'HashMap',
  'serde_json::Value': 'Value',
  'std::string::String': 'String',
}

/** `::core::pin::Pin`, `alloc::boxed::Box`, ... -> last path segment. */
function simplifyPathName(name: string): string {
  const mapped = SIMPLIFY_PATHS[name]
  if (mapped) return mapped
  if (/^(::)?(core|std|alloc)::/.test(name)) return name.split('::').pop() ?? name
  return name
}

export function rustTypeToString(type: any): string {
  if (type == null) return '()'
  if (typeof type === 'string') return type
  if (type.primitive != null) return type.primitive
  if (type.resolved_path != null) {
    const rp = type.resolved_path
    let name = rp.path ?? rp.name ?? 'unknown'
    name = name.replace(/^crate::[\w:]+::/, '')
    name = simplifyPathName(name)
    if (rp.args?.angle_bracketed?.args?.length) {
      const args = rp.args.angle_bracketed.args
        .map((a: any) => (a.type ? rustTypeToString(a.type) : a.lifetime ?? '?'))
        .join(', ')
      return `${name}<${args}>`
    }
    return name
  }
  if (type.borrowed_ref != null) {
    const br = type.borrowed_ref
    const inner = rustTypeToString(br.type)
    const mutStr = br.is_mutable || br.mutable ? 'mut ' : ''
    const lt = br.lifetime ? `${br.lifetime} ` : ''
    return `&${lt}${mutStr}${inner}`
  }
  if (type.generic != null) return type.generic
  if (type.tuple != null) {
    if (type.tuple.length === 0) return '()'
    return `(${type.tuple.map(rustTypeToString).join(', ')})`
  }
  if (type.slice != null) return `[${rustTypeToString(type.slice)}]`
  if (type.array != null) return `[${rustTypeToString(type.array.type)}; ${type.array.len}]`
  if (type.raw_pointer != null) {
    const rp = type.raw_pointer
    return `*${rp.is_mutable ? 'mut' : 'const'} ${rustTypeToString(rp.type)}`
  }
  if (type.impl_trait != null) {
    const bounds = type.impl_trait
      .map((b: any) => {
        if (b.trait_bound) {
          const t = b.trait_bound.trait
          let name = t.path ?? t.name ?? '?'
          name = simplifyPathName(name.replace(/^crate::[\w:]+::/, ''))
          if (t.args?.parenthesized) {
            const { inputs, output } = t.args.parenthesized
            const params = (inputs ?? []).map(rustTypeToString).join(', ')
            const ret = output ? ` -> ${rustTypeToString(output)}` : ''
            return `${name}(${params})${ret}`
          }
          if (t.args?.angle_bracketed?.args?.length) {
            const args = t.args.angle_bracketed.args.map((a: any) => (a.type ? rustTypeToString(a.type) : '?')).join(', ')
            return `${name}<${args}>`
          }
          return name
        }
        return '?'
      })
      .join(' + ')
    return `impl ${bounds}`
  }
  if (type.qualified_path != null) return type.qualified_path.name ?? 'unknown'
  if (type.dyn_trait != null) {
    const traits = type.dyn_trait.traits
      ?.map((b: any) => {
        const t = b.trait
        let name = t?.path ?? t?.name ?? '?'
        name = simplifyPathName(name.replace(/^crate::[\w:]+::/, ''))
        if (t?.args?.parenthesized) {
          const { inputs, output } = t.args.parenthesized
          const params = (inputs ?? []).map(rustTypeToString).join(', ')
          const ret = output ? ` -> ${rustTypeToString(output)}` : ''
          return `${name}(${params})${ret}`
        }
        if (t?.args?.angle_bracketed?.args?.length) {
          const args = t.args.angle_bracketed.args
            .map((a: any) => (a.type ? rustTypeToString(a.type) : a.lifetime ?? '?'))
            .join(', ')
          return `${name}<${args}>`
        }
        return name
      })
      .join(' + ')
    return `dyn ${traits ?? '?'}`
  }
  if (type.fn_pointer != null) {
    const fp = type.fn_pointer
    const params = fp.sig?.inputs?.map(([n, t]: [string, any]) => `${n}: ${rustTypeToString(t)}`).join(', ') ?? ''
    const ret = fp.sig?.output ? ` -> ${rustTypeToString(fp.sig.output)}` : ''
    return `fn(${params})${ret}`
  }
  return 'unknown'
}

/**
 * The summary prose of a doc-comment: everything before the first `# Section`
 * heading. Fence-aware, since a hidden doctest line (`# use ...` inside a
 * fence) is not a heading. `# Examples` and friends are rendered separately.
 * A fenced block inside the prose (e.g. the `for_function` example on
 * `IIITrigger`) is kept on request, with hidden doctest lines stripped;
 * otherwise the prose is cut at the first fence.
 */
function extractDocs(item: RustDocItem, opts?: { keepCodeBlocks?: boolean }): string {
  if (!item.docs) return ''
  const out: string[] = []
  let inFence = false
  for (const line of item.docs.split('\n')) {
    if (line.startsWith('```')) {
      if (!inFence && !opts?.keepCodeBlocks) break
      inFence = !inFence
      out.push(line)
      continue
    }
    if (inFence) {
      // Hidden doctest lines never render on docs.rs either.
      if (!/^\s*# /.test(line)) out.push(line)
      continue
    }
    if (/^# /.test(line)) break
    out.push(line)
  }
  return out
    .join('\n')
    .replace(EXPAND_PARAMS_RE, '')
    .replace(INTERNAL_RE, '')
    // Strip rustdoc intra-doc link markup: [`Foo`](path) / [`Foo`][] / [`Foo`] -> `Foo`
    .replace(/\[(`[^`]+`)\]\([^)]*\)/g, '$1')
    .replace(/\[(`[^`]+`)\]\[[^\]]*\]/g, '$1')
    .replace(/\[(`[^`]+`)\]/g, '$1')
    .trim()
}

function extractExamples(item: RustDocItem): string[] {
  if (!item.docs) return []
  const examples: string[] = []
  for (const match of item.docs.matchAll(/# Examples?\n+```[\w,]*\n([\s\S]*?)```/g)) {
    const cleaned = match[1]
      .split('\n')
      .filter(line => !line.startsWith('# '))
      .join('\n')
      .trim()
    examples.push(cleaned)
  }
  return examples
}

function extractArgDescriptions(docs: string | undefined): Record<string, string> {
  if (!docs) return {}
  const argSection = docs.match(/# Arguments\n([\s\S]*?)(?=\n# |\n\n[^*\s]|$)/)
  if (!argSection) return {}
  const result: Record<string, string> = {}
  let currentName = ''
  let currentDesc = ''
  for (const line of argSection[1].split('\n')) {
    const match = line.match(/^\s*\*\s*`(\w+)`\s*[-–—:]\s*(.*)/)
    if (match) {
      if (currentName) result[currentName] = currentDesc.trim()
      currentName = match[1]
      currentDesc = match[2]
    } else if (currentName && line.match(/^\s+\S/)) {
      currentDesc += ' ' + line.trim()
    }
  }
  if (currentName) result[currentName] = currentDesc.trim()
  return result
}

function extractReturnDescription(docs: string | undefined): string {
  if (!docs) return ''
  const returnSection = docs.match(/# (?:Returns|Return)\n([\s\S]*?)(?=\n# |$)/)
  if (!returnSection) return ''
  return returnSection[1]
    .split('\n')
    .map(l => l.replace(/^\s*\*?\s*/, ''))
    .join(' ')
    .replace(/`([^`]+)`/g, '$1')
    .trim()
}

/** First paragraph, unwrapped to one line, for table-cell descriptions. */
function firstParagraph(text: string): string {
  return (text.split(/\n\s*\n/)[0] ?? '').replace(/\s*\n\s*/g, ' ').trim()
}

function getItemKind(item: RustDocItem): string {
  return Object.keys(item.inner)[0] ?? 'unknown'
}

function getStructFields(item: RustDocItem): number[] {
  const s = item.inner.struct
  if (!s) return []
  return s.kind?.plain?.fields ?? s.fields ?? []
}
function getStructImpls(item: RustDocItem): number[] {
  return item.inner.struct?.impls ?? []
}
function getEnumVariants(item: RustDocItem): number[] {
  return item.inner.enum?.variants ?? []
}
function getEnumImpls(item: RustDocItem): number[] {
  return item.inner.enum?.impls ?? []
}
function isInherentImpl(item: RustDocItem): boolean {
  const impl = item.inner.impl
  return impl?.trait === null || impl?.trait === undefined
}
function getImplItems(item: RustDocItem): number[] {
  return item.inner.impl?.items ?? []
}
function getSig(item: RustDocItem): { inputs: [string, any][]; output: any } | null {
  return (item.inner.function ?? item.inner.method)?.sig ?? null
}
function isAsync(item: RustDocItem): boolean {
  return (item.inner.function ?? item.inner.method)?.header?.is_async ?? false
}
function isOptionType(type: any): boolean {
  const rp = type?.resolved_path
  if (rp) {
    const name = rp.path ?? rp.name ?? ''
    return name === 'Option' || name.endsWith('::Option')
  }
  return false
}

function buildMethodDoc(item: RustDocItem): FunctionDoc | null {
  const sig = getSig(item)
  if (!sig) return null
  const inputs = (sig.inputs ?? []).filter(
    ([name, type]: [string, any]) => name !== 'self' && !(type?.borrowed_ref?.type?.generic === 'Self'),
  )
  const argDescs = extractArgDescriptions(item.docs)
  const params: ParamDoc[] = inputs.map(([name, type]: [string, any]) => ({
    name,
    type: rustTypeToString(type),
    description: argDescs[name] ?? '',
    required: !isOptionType(type),
  }))
  const outputType = rustTypeToString(sig.output)
  const paramSig = inputs.map(([name, type]: [string, any]) => `${name}: ${rustTypeToString(type)}`).join(', ')
  const asyncPrefix = isAsync(item) ? 'async ' : ''
  const returnSuffix = outputType && outputType !== '()' ? ` -> ${outputType}` : ''
  return {
    name: item.name ?? 'unknown',
    signature: `${asyncPrefix}${item.name}(${paramSig})${returnSuffix}`,
    description: extractDocs(item),
    params,
    returns: { type: outputType, description: extractReturnDescription(item.docs) },
    examples: extractExamples(item),
    ...parseExpandMarker(item.docs),
  }
}

function extractMethodsFromStruct(structId: number, index: Record<string, RustDocItem>, exclude?: Set<string>): FunctionDoc[] {
  const structItem = index[structId]
  if (!structItem) return []
  const methods: FunctionDoc[] = []
  const kind = getItemKind(structItem)
  const implIds = kind === 'enum' ? getEnumImpls(structItem) : getStructImpls(structItem)
  for (const implId of implIds) {
    const implItem = index[implId]
    if (!implItem || !isInherentImpl(implItem)) continue
    for (const methodId of getImplItems(implItem)) {
      const method = index[methodId]
      if (!method || method.visibility !== 'public') continue
      if (method.deprecation) continue
      if (exclude && exclude.has(method.name ?? '')) continue
      if (getItemKind(method) !== 'function') continue
      const doc = buildMethodDoc(method)
      if (doc) methods.push(doc)
    }
  }
  return methods
}

/** Public inherent-impl methods of a struct/enum, as callable-typed rows so
 * handles like `FunctionRef::unregister` and `TriggerTypeRef::register_trigger`
 * are documented with their parameters. */
function extractMethodRows(item: RustDocItem, index: Record<string, RustDocItem>): ParamDoc[] {
  const rows: ParamDoc[] = []
  const kind = getItemKind(item)
  const implIds = kind === 'enum' ? getEnumImpls(item) : getStructImpls(item)
  for (const implId of implIds) {
    const implItem = index[implId]
    if (!implItem || !isInherentImpl(implItem)) continue
    for (const methodId of getImplItems(implItem)) {
      const method = index[methodId]
      if (!method || method.visibility !== 'public' || method.deprecation) continue
      if (getItemKind(method) !== 'function') continue
      if (isInternalDoc(method.docs)) continue
      const sig = getSig(method)
      if (!sig) continue
      const inputs = (sig.inputs ?? []).filter(
        ([name, type]: [string, any]) => name !== 'self' && !(type?.borrowed_ref?.type?.generic === 'Self'),
      )
      const paramSig = inputs.map(([name, type]: [string, any]) => `${name}: ${rustTypeToString(type)}`).join(', ')
      const outputType = rustTypeToString(sig.output)
      const returnSuffix = outputType && outputType !== '()' ? ` -> ${outputType}` : ''
      rows.push({
        name: method.name ?? '',
        type: `${isAsync(method) ? 'async ' : ''}fn(${paramSig})${returnSuffix}`,
        description: firstParagraph(extractDocs(method)),
        required: true,
      })
    }
  }
  return rows.sort((a, b) => a.name.localeCompare(b.name))
}

/**
 * Whether a method row is a fluent builder setter/constructor (returns `Self`
 * or the owning type). On a type that also has data fields these duplicate or
 * shadow the fields (e.g. a `stream_name(self) -> Self` setter next to the
 * `stream_name` field), so they are dropped there; on a pure builder with no
 * data fields (e.g. `RegisterFunction`) they are the real API and kept.
 */
function isBuilderRow(rowType: string, typeName: string): boolean {
  return new RegExp(`->\\s*(Self|${typeName})$`).test(rowType)
}

function withMethodRows(item: RustDocItem, index: Record<string, RustDocItem>, dataFields: ParamDoc[]): ParamDoc[] {
  const methodRows = extractMethodRows(item, index)
  const kept = dataFields.length
    ? methodRows.filter(m => !isBuilderRow(m.type, item.name ?? ''))
    : methodRows
  return [...dataFields, ...kept]
}

function extractStructType(item: RustDocItem, index: Record<string, RustDocItem>): TypeDoc {
  const dataFields: ParamDoc[] = []
  for (const fid of getStructFields(item)) {
    const field = index[fid]
    if (!field || field.visibility !== 'public') continue
    dataFields.push({
      name: field.name ?? '',
      type: rustTypeToString(field.inner.struct_field),
      description: extractDocs(field),
      required: !isOptionType(field.inner.struct_field),
    })
  }
  return {
    name: item.name ?? '',
    description: extractDocs(item, { keepCodeBlocks: true }),
    fields: withMethodRows(item, index, dataFields),
  }
}

function extractEnumType(item: RustDocItem, index: Record<string, RustDocItem>): TypeDoc {
  const fields: ParamDoc[] = []
  for (const vid of getEnumVariants(item)) {
    const variant = index[vid]
    if (!variant) continue
    const vKind = variant.inner.variant?.kind
    let typeStr = 'unit'
    if (vKind && typeof vKind === 'object') {
      if (vKind.struct) {
        const fieldStrs = (vKind.struct.fields ?? []).map((fid: number) => {
          const f = index[fid]
          return f ? `${f.name}: ${rustTypeToString(f.inner.struct_field)}` : '?'
        })
        typeStr = `{ ${fieldStrs.join(', ')} }`
      } else if (vKind.tuple) {
        typeStr = `(${vKind.tuple.map((tid: number) => {
          const t = index[tid]
          return t ? rustTypeToString(t.inner.struct_field) : '?'
        }).join(', ')})`
      }
    }
    fields.push({ name: variant.name ?? '', type: typeStr, description: extractDocs(variant), required: true })
  }
  return {
    name: item.name ?? '',
    description: extractDocs(item, { keepCodeBlocks: true }),
    fields: withMethodRows(item, index, fields),
  }
}

/** A trait documents as one row per method; implementing the required ones is
 * the contract (e.g. `TriggerHandler` for `register_trigger_type`). */
function extractTraitType(item: RustDocItem, index: Record<string, RustDocItem>): TypeDoc {
  const fields: ParamDoc[] = []
  for (const mid of item.inner.trait?.items ?? []) {
    const method = index[mid]
    if (!method || getItemKind(method) !== 'function') continue
    if (isInternalDoc(method.docs)) continue
    const sig = getSig(method)
    if (!sig) continue
    const inputs = (sig.inputs ?? []).filter(([name]: [string, any]) => name !== 'self')
    const paramSig = inputs.map(([name, type]: [string, any]) => `${name}: ${rustTypeToString(type)}`).join(', ')
    const outputType = rustTypeToString(sig.output)
    const returnSuffix = outputType && outputType !== '()' ? ` -> ${outputType}` : ''
    fields.push({
      name: method.name ?? '',
      type: `${isAsync(method) ? 'async ' : ''}fn(${paramSig})${returnSuffix}`,
      description: firstParagraph(extractDocs(method)),
      required: !(method.inner.function ?? method.inner.method)?.has_body,
    })
  }
  return { name: item.name ?? '', description: extractDocs(item, { keepCodeBlocks: true }), fields }
}

function itemToType(item: RustDocItem, index: Record<string, RustDocItem>): TypeDoc | null {
  if (isInternalDoc(item.docs)) return null
  const kind = getItemKind(item)
  if (kind === 'struct') return extractStructType(item, index)
  if (kind === 'enum') return extractEnumType(item, index)
  if (kind === 'trait') return extractTraitType(item, index)
  if (kind === 'type_alias') {
    const aliased = item.inner.type_alias?.type
    return {
      name: item.name ?? '',
      description: extractDocs(item),
      fields: [],
      codeBlock: `type ${item.name} = ${aliased ? rustTypeToString(aliased) : '...'}`,
    }
  }
  return null
}

const TYPE_KINDS = new Set(['struct', 'enum', 'type_alias', 'trait'])

/** All public, locally-defined, publicly-reachable types in the crate. */
function collectCrateTypes(
  data: RustDocIndex,
  exclude: Set<string>,
): { item: RustDocItem; module: string; rootLevel: boolean }[] {
  const out: { item: RustDocItem; module: string; rootLevel: boolean }[] = []
  const seen = new Set<string>()
  for (const [id, item] of Object.entries(data.index)) {
    if (item.crate_id !== 0) continue
    if (item.visibility !== 'public') continue
    if (item.deprecation) continue
    if (!item.name || item.name.startsWith('_')) continue
    if (exclude.has(item.name)) continue
    if (!TYPE_KINDS.has(getItemKind(item))) continue
    // Only items with a canonical public path are part of the reachable API.
    const path = data.paths[id]?.path
    if (!path || path.length < 2) continue
    if (seen.has(item.name)) continue
    seen.add(item.name)
    out.push({ item, module: path[1] ?? '', rootLevel: path.length <= 2 })
  }
  return out
}

/**
 * The crate's curated public facade modules (`pub mod channel { pub use ... }`,
 * etc.) are the blessed import paths; the flat crate-root re-exports of the same
 * types are `#[deprecated]`. rustdoc attributes each type to its *definition*
 * module (`channels`, `error`, `triggers`, ...), so we read the facade modules'
 * re-export (`use`) items and remap each type to the facade that exposes it.
 * This aligns Rust's channel/trigger/runtime/errors subpaths with the Node and
 * Python SDKs. Derived from the rustdoc index, not hardcoded.
 */
const FACADE_MODULES = new Set(['channel', 'trigger', 'runtime', 'errors'])
function collectFacadeMap(data: RustDocIndex): Map<string, string> {
  const map = new Map<string, string>()
  for (const item of Object.values(data.index)) {
    if (item.crate_id !== 0 || !item.name || !FACADE_MODULES.has(item.name)) continue
    const mod = item.inner?.module
    if (!mod) continue
    for (const cid of mod.items ?? []) {
      const child = data.index[cid]
      const name = child?.inner?.use?.name ?? child?.name
      if (name) map.set(name, `iii_sdk::${item.name}`)
    }
  }
  return map
}

function findItemByName(data: RustDocIndex, name: string, kind: string): RustDocItem | undefined {
  for (const item of Object.values(data.index)) {
    if (item.crate_id !== 0 || item.name !== name) continue
    if (getItemKind(item) === kind) return item
  }
  return undefined
}

/**
 * Sibling project crates whose types the SDK re-exports at its root
 * (`pub use iii_helpers::queue::EnqueueResult`). rustdoc leaves only a `use`
 * stub in the SDK crate, so the definition is read from the helpers crate JSON
 * and documented on the SDK page, matching the Node and Python SDK pages.
 */
const EXTERNAL_CRATES = ['iii_helpers']

function collectReExportedTypes(
  data: RustDocIndex,
  helpersJsonPath: string | undefined,
  exclude: Set<string>,
): TypeDoc[] {
  if (!helpersJsonPath) return []
  let helpers: RustDocIndex
  try {
    helpers = JSON.parse(readFileSync(helpersJsonPath, 'utf-8'))
  } catch {
    return []
  }
  // Names this crate re-exports from an external project crate.
  const names = new Set<string>()
  for (const item of Object.values(data.index)) {
    if (item.crate_id !== 0 || item.deprecation) continue
    const use = item.inner?.use
    if (!use?.name || !use?.source) continue
    if (EXTERNAL_CRATES.some(c => use.source.startsWith(`${c}::`))) names.add(use.name)
  }
  const out: TypeDoc[] = []
  for (const name of names) {
    if (exclude.has(name)) continue
    const def = Object.values(helpers.index).find(
      it => it.crate_id === 0 && it.name === name && TYPE_KINDS.has(getItemKind(it)),
    )
    if (!def) continue
    const t = itemToType(def, helpers.index)
    if (t) out.push(t)
  }
  return out
}

// ---------------------------------------------------------------------------
// Core SDK page (iii-sdk)
// ---------------------------------------------------------------------------

export function parseRustdoc(jsonPath: string, helpersJsonPath?: string): SdkDoc {
  let data: RustDocIndex
  try {
    data = JSON.parse(readFileSync(jsonPath, 'utf-8'))
  } catch {
    console.warn(`[parse-rustdoc] Could not read ${jsonPath}`)
    return emptyDoc('Rust SDK', 'API reference for the iii SDK for Rust.', 'cargo add iii-sdk', 'use iii_sdk::{register_worker, InitOptions};')
  }

  const clientStruct = findItemByName(data, MAIN_STRUCT, 'struct')
  const methods = clientStruct ? extractMethodsFromStruct(clientStruct.id, data.index, METHOD_EXCLUDE) : []
  methods.sort((a, b) => a.name.localeCompare(b.name))

  const registerWorker = findItemByName(data, 'register_worker', 'function')
  const entryPoint = registerWorker ? buildMethodDoc(registerWorker) : null

  const PKG = 'iii_sdk'
  const facade = collectFacadeMap(data)
  const types: TypeDoc[] = []
  const bySub = new Map<string, TypeDoc[]>()
  for (const { item, module, rootLevel } of collectCrateTypes(data, new Set([MAIN_STRUCT]))) {
    const t = itemToType(item, data.index)
    if (!t) continue
    types.push(t)
    // Facade module wins; the crate's core `iii` module and crate-root items
    // are re-exported (non-deprecated) at the root, so label them `iii_sdk`.
    const subpath =
      facade.get(item.name ?? '') ?? (rootLevel || module === 'iii' ? PKG : `${PKG}::${module}`)
    if (!bySub.has(subpath)) bySub.set(subpath, [])
    bySub.get(subpath)!.push(t)
  }
  // Types re-exported from sibling crates (e.g. EnqueueResult) — root re-exports.
  for (const t of collectReExportedTypes(data, helpersJsonPath, new Set([MAIN_STRUCT]))) {
    if (types.some(x => x.name === t.name)) continue
    types.push(t)
    if (!bySub.has(PKG)) bySub.set(PKG, [])
    bySub.get(PKG)!.push(t)
  }
  types.sort((a, b) => a.name.localeCompare(b.name))
  const typeGroups: TypeGroup[] = [...bySub.entries()].map(([subpath, ts]) => ({
    subpath,
    types: ts.sort((a, b) => a.name.localeCompare(b.name)),
  }))
  typeGroups.sort((a, b) => (a.subpath === PKG ? -1 : b.subpath === PKG ? 1 : a.subpath.localeCompare(b.subpath)))

  return {
    metadata: {
      language: 'rust',
      languageLabel: 'Rust',
      title: 'Rust SDK',
      description: 'API reference for the iii SDK for Rust.',
      installCommand: 'cargo add iii-sdk',
      importExample: 'use iii_sdk::{register_worker, InitOptions};',
      packageName: 'iii_sdk',
    },
    initialization: {
      entryPoint: entryPoint ?? {
        name: 'register_worker',
        signature: 'register_worker(address: &str, options: InitOptions) -> IIIClient',
        description: 'Create and return a connected SDK instance.',
        params: [],
        returns: { type: 'IIIClient', description: '' },
        examples: [],
      },
    },
    methods,
    types,
    typeGroups,
  }
}

// ---------------------------------------------------------------------------
// Helpers library page (iii-helpers)
// ---------------------------------------------------------------------------

const HELPERS_MODULE_DESCRIPTIONS: Record<string, string> = {
  http: 'HTTP request/response types, auth config, and the `http` helper.',
  queue: 'Queue enqueue result types.',
  stream: 'Stream trigger configs, change events, IO inputs, and update operations.',
  worker_connection_manager: 'RBAC auth and registration callback types.',
  observability: 'Logger, OpenTelemetry config, and span helpers.',
}

export function parseHelpersRustdoc(jsonPath: string): SdkDoc {
  let data: RustDocIndex
  try {
    data = JSON.parse(readFileSync(jsonPath, 'utf-8'))
  } catch {
    console.warn(`[parse-rustdoc] Could not read ${jsonPath}`)
    return { ...emptyDoc('Helpers (Rust)', 'API reference for the iii-helpers crate (Rust).', 'cargo add iii-helpers', 'use iii_helpers::http;'), isLibrary: true, modules: [] }
  }

  const byModule = new Map<string, TypeDoc[]>()
  for (const { item, module } of collectCrateTypes(data, new Set())) {
    const t = itemToType(item, data.index)
    if (!t) continue
    if (!byModule.has(module)) byModule.set(module, [])
    byModule.get(module)!.push(t)
  }

  const modules: ModuleDoc[] = []
  for (const [name, types] of byModule) {
    if (!name) continue
    types.sort((a, b) => a.name.localeCompare(b.name))
    modules.push({
      name,
      importPath: `use iii_helpers::${name};`,
      description: HELPERS_MODULE_DESCRIPTIONS[name] ?? '',
      functions: [],
      types,
    })
  }
  modules.sort((a, b) => a.name.localeCompare(b.name))

  return {
    metadata: {
      language: 'rust',
      languageLabel: 'Rust',
      title: 'Helpers (Rust)',
      description: 'API reference for the iii-helpers crate (Rust).',
      installCommand: 'cargo add iii-helpers',
      importExample: 'use iii_helpers::http;',
      packageName: 'iii_helpers',
    },
    isLibrary: true,
    initialization: { entryPoint: { name: '', signature: '', description: '', params: [], returns: { type: '', description: '' }, examples: [] } },
    methods: [],
    types: [],
    modules,
  }
}

function emptyDoc(title: string, description: string, installCommand: string, importExample: string): SdkDoc {
  return {
    metadata: { language: 'rust', languageLabel: 'Rust', title, description, installCommand, importExample, packageName: 'iii_sdk' },
    initialization: {
      entryPoint: { name: 'register_worker', signature: '', description: '', params: [], returns: { type: '', description: '' }, examples: [] },
    },
    methods: [],
    types: [],
  }
}

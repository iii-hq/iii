import { readFileSync } from 'node:fs'
import type { FunctionDoc, LoggerDoc, ParamDoc, SdkDoc, TypeDoc } from '../types.mjs'

// ---------------------------------------------------------------------------
// Rustdoc JSON types (format_version ~56)
// ---------------------------------------------------------------------------

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
}

// ---------------------------------------------------------------------------
// Configuration: which items make up the public API
// ---------------------------------------------------------------------------

const MAIN_STRUCT = 'III'

const METHOD_EXCLUDE = new Set([
  'new',
  'with_metadata',
  'address',
  'set_metadata',
  'set_otel_config',
  'connect',
])

const EXPORTED_TYPES = new Set([
  'InitOptions',
  'IIIError',
  'IIIConnectionState',
  'TriggerRequest',
  'TriggerAction',
  'HttpInvocationConfig',
  'HttpAuthConfig',
  'HttpMethod',
  'Channel',
  'ChannelReader',
  'ChannelWriter',
  'ChannelDirection',
  'StreamChannelRef',
  'FunctionInfo',
  'FunctionRef',
  'TriggerInfo',
  'WorkerInfo',
  'WorkerMetadata',
  'Trigger',
  'RegisterFunctionMessage',
  'RegisterServiceMessage',
  'RegisterTriggerMessage',
  'RegisterTriggerTypeMessage',
  'Streams',
  'UpdateBuilder',
  'UpdateOp',
  'UpdateResult',
  'EnqueueResult',
  'OtelConfig',
  'ReconnectionConfig',
])

// ---------------------------------------------------------------------------
// Type resolution: convert rustdoc JSON type objects to readable strings
// ---------------------------------------------------------------------------

const SIMPLIFY_PATHS: Record<string, string> = {
  'std::collections::HashMap': 'HashMap',
  'serde_json::Value': 'Value',
  'std::string::String': 'String',
}

export function rustTypeToString(type: any): string {
  if (type == null) return '()'
  if (typeof type === 'string') return type

  if (type.primitive != null) return type.primitive

  if (type.resolved_path != null) {
    const rp = type.resolved_path
    let name = rp.path ?? rp.name ?? 'unknown'
    name = name.replace(/^crate::[\w:]+::/, '')
    name = SIMPLIFY_PATHS[name] ?? name
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
    const mutStr = rp.is_mutable ? 'mut' : 'const'
    return `*${mutStr} ${rustTypeToString(rp.type)}`
  }

  if (type.impl_trait != null) {
    const bounds = type.impl_trait
      .map((b: any) => {
        if (b.trait_bound) {
          const t = b.trait_bound.trait
          let name = t.path ?? t.name ?? '?'
          name = name.replace(/^crate::[\w:]+::/, '')
          if (t.args?.angle_bracketed?.args?.length) {
            const args = t.args.angle_bracketed.args
              .map((a: any) => (a.type ? rustTypeToString(a.type) : '?'))
              .join(', ')
            return `${name}<${args}>`
          }
          return name
        }
        return '?'
      })
      .join(' + ')
    return `impl ${bounds}`
  }

  if (type.qualified_path != null) {
    const qp = type.qualified_path
    return qp.name ?? 'unknown'
  }

  if (type.dyn_trait != null) {
    const traits = type.dyn_trait.traits
      ?.map((b: any) => {
        const t = b.trait
        let name = t?.path ?? t?.name ?? '?'
        name = name.replace(/^crate::[\w:]+::/, '')
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

// ---------------------------------------------------------------------------
// Doc comment parsing
// ---------------------------------------------------------------------------

export function extractDocs(item: RustDocItem): string {
  if (!item.docs) return ''
  const text = item.docs
  const sectionIdx = text.search(/\n# /)
  const codeBlockIdx = text.search(/\n```/)
  let endIdx = text.length
  if (sectionIdx >= 0) endIdx = Math.min(endIdx, sectionIdx)
  if (codeBlockIdx >= 0) endIdx = Math.min(endIdx, codeBlockIdx)
  return text.slice(0, endIdx).trim()
}

export function extractExamples(item: RustDocItem): string[] {
  if (!item.docs) return []
  const exampleMatches = item.docs.matchAll(/# Examples?\n+```[\w,]*\n([\s\S]*?)```/g)
  const examples: string[] = []
  for (const match of exampleMatches) {
    const code = match[1].trim()
    const cleaned = code
      .split('\n')
      .filter((line) => !line.startsWith('# '))
      .join('\n')
      .trim()
    examples.push(cleaned)
  }
  return examples
}

export function extractArgDescriptions(docs: string | undefined): Record<string, string> {
  if (!docs) return {}
  const argSection = docs.match(/# Arguments\n([\s\S]*?)(?=\n# |\n\n[^*\s]|$)/)
  if (!argSection) return {}
  const result: Record<string, string> = {}
  const lines = argSection[1].split('\n')
  let currentName = ''
  let currentDesc = ''

  for (const line of lines) {
    const match = line.match(/^\s*\*\s*`(\w+)`\s*[-–—]\s*(.*)/)
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

export function extractReturnDescription(docs: string | undefined): string {
  if (!docs) return ''
  const returnSection = docs.match(/# (?:Returns|Return)\n([\s\S]*?)(?=\n# |$)/)
  if (returnSection) {
    return returnSection[1]
      .split('\n')
      .map((l) => l.replace(/^\s*\*?\s*/, ''))
      .join(' ')
      .replace(/`([^`]+)`/g, '$1')
      .trim()
  }
  return ''
}

// ---------------------------------------------------------------------------
// Helpers for navigating rustdoc index
// ---------------------------------------------------------------------------

function getItemKind(item: RustDocItem): string {
  return Object.keys(item.inner)[0] ?? 'unknown'
}

function getStructFields(item: RustDocItem): number[] {
  const s = item.inner.struct
  if (!s) return []
  return s.kind?.plain?.fields ?? s.fields ?? []
}

function getStructImpls(item: RustDocItem): number[] {
  const s = item.inner.struct
  return s?.impls ?? []
}

function getEnumVariants(item: RustDocItem): number[] {
  const e = item.inner.enum
  return e?.variants ?? []
}

function getEnumImpls(item: RustDocItem): number[] {
  const e = item.inner.enum
  return e?.impls ?? []
}

function isInherentImpl(item: RustDocItem): boolean {
  const impl = item.inner.impl
  return impl?.trait === null || impl?.trait === undefined
}

function getImplItems(item: RustDocItem): number[] {
  return item.inner.impl?.items ?? []
}

function getSig(item: RustDocItem): { inputs: [string, any][]; output: any } | null {
  const fn_ = item.inner.function ?? item.inner.method
  return fn_?.sig ?? null
}

function isAsync(item: RustDocItem): boolean {
  const fn_ = item.inner.function ?? item.inner.method
  return fn_?.header?.is_async ?? false
}

// ---------------------------------------------------------------------------
// Method/function extraction
// ---------------------------------------------------------------------------

function buildMethodDoc(
  item: RustDocItem,
  index: Record<string, RustDocItem>,
): FunctionDoc | null {
  const sig = getSig(item)
  if (!sig) return null

  const inputs = (sig.inputs ?? []).filter(
    ([name, type]: [string, any]) =>
      name !== 'self' && !(type?.borrowed_ref?.type?.generic === 'Self'),
  )

  const argDescs = extractArgDescriptions(item.docs)

  const params: ParamDoc[] = inputs.map(([name, type]: [string, any]) => ({
    name,
    type: rustTypeToString(type),
    description: argDescs[name] ?? '',
    required: !isOptionType(type),
  }))

  const outputType = rustTypeToString(sig.output)
  const returnDesc = extractReturnDescription(item.docs)

  const paramSig = inputs.map(([name, type]: [string, any]) => `${name}: ${rustTypeToString(type)}`).join(', ')
  const asyncPrefix = isAsync(item) ? 'async ' : ''
  const returnSuffix = outputType && outputType !== '()' ? ` -> ${outputType}` : ''
  const signature = `${asyncPrefix}${item.name}(${paramSig})${returnSuffix}`

  return {
    name: item.name ?? 'unknown',
    signature,
    description: extractDocs(item),
    params,
    returns: { type: outputType, description: returnDesc },
    examples: extractExamples(item),
  }
}

function isOptionType(type: any): boolean {
  if (!type) return false
  const rp = type.resolved_path
  if (rp) {
    const name = rp.path ?? rp.name ?? ''
    return name === 'Option' || name.endsWith('::Option')
  }
  return false
}

function extractMethodsFromStruct(
  structId: number,
  index: Record<string, RustDocItem>,
  exclude?: Set<string>,
): FunctionDoc[] {
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
      if (exclude && exclude.has(method.name ?? '')) continue
      if (getItemKind(method) !== 'function') continue

      const doc = buildMethodDoc(method, index)
      if (doc) methods.push(doc)
    }
  }

  return methods
}

// ---------------------------------------------------------------------------
// Type extraction
// ---------------------------------------------------------------------------

function extractMethodsAsFields(
  item: RustDocItem,
  index: Record<string, RustDocItem>,
  exclude?: Set<string>,
): ParamDoc[] {
  const kind = getItemKind(item)
  const implIds = kind === 'enum' ? getEnumImpls(item) : getStructImpls(item)
  const fields: ParamDoc[] = []

  for (const implId of implIds) {
    const implItem = index[implId]
    if (!implItem || !isInherentImpl(implItem)) continue

    for (const methodId of getImplItems(implItem)) {
      const method = index[methodId]
      if (!method || method.visibility !== 'public') continue
      if (exclude && exclude.has(method.name ?? '')) continue
      if (getItemKind(method) !== 'function') continue

      const sig = getSig(method)
      if (!sig) continue
      const inputs = (sig.inputs ?? []).filter(
        ([name, type]: [string, any]) =>
          name !== 'self' && !(type?.borrowed_ref?.type?.generic === 'Self'),
      )
      const paramSig = inputs.map(([n, t]: [string, any]) => `${n}: ${rustTypeToString(t)}`).join(', ')
      const asyncPrefix = isAsync(method) ? 'async ' : ''
      const outputType = rustTypeToString(sig.output)
      const retPart = outputType && outputType !== '()' ? ` -> ${outputType}` : ''
      const typeStr = `${asyncPrefix}fn(${paramSig})${retPart}`

      fields.push({
        name: method.name ?? '',
        type: typeStr,
        description: extractDocs(method),
        required: true,
      })
    }
  }

  return fields
}

function extractStructType(
  item: RustDocItem,
  index: Record<string, RustDocItem>,
): TypeDoc {
  const fieldIds = getStructFields(item)
  const fields: ParamDoc[] = []

  for (const fid of fieldIds) {
    const field = index[fid]
    if (!field || field.visibility !== 'public') continue
    fields.push({
      name: field.name ?? '',
      type: rustTypeToString(field.inner.struct_field),
      description: extractDocs(field),
      required: !isOptionType(field.inner.struct_field),
    })
  }

  const methodFields = extractMethodsAsFields(item, index, new Set(['new']))
  fields.push(...methodFields)

  return {
    name: item.name ?? '',
    description: extractDocs(item),
    fields,
  }
}

function extractEnumType(
  item: RustDocItem,
  index: Record<string, RustDocItem>,
): TypeDoc {
  const variantIds = getEnumVariants(item)
  const fields: ParamDoc[] = []

  for (const vid of variantIds) {
    const variant = index[vid]
    if (!variant) continue
    const vKind = variant.inner.variant?.kind
    let typeStr = 'unit'
    if (vKind && typeof vKind === 'object') {
      if (vKind.struct) {
        const vFields = vKind.struct.fields ?? []
        const fieldStrs = vFields.map((fid: number) => {
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
    fields.push({
      name: variant.name ?? '',
      type: typeStr,
      description: extractDocs(variant),
      required: true,
    })
  }

  const methodFields = extractMethodsAsFields(item, index, new Set(['new']))
  fields.push(...methodFields)

  return {
    name: item.name ?? '',
    description: extractDocs(item),
    fields,
  }
}

// ---------------------------------------------------------------------------
// Re-export resolution
// ---------------------------------------------------------------------------

interface ReExport {
  name: string
  targetId: number
}

function collectReExports(rootItem: RustDocItem, index: Record<string, RustDocItem>): ReExport[] {
  const mod = rootItem.inner.module
  if (!mod?.items) return []

  const result: ReExport[] = []
  for (const itemId of mod.items) {
    const item = index[itemId]
    if (!item) continue
    if (getItemKind(item) === 'use') {
      const use = item.inner.use
      if (use?.id != null && use.name) {
        result.push({ name: use.name, targetId: use.id })
      }
    }
  }
  return result
}

// ---------------------------------------------------------------------------
// Main parser
// ---------------------------------------------------------------------------

export function parseRustdocData(data: RustDocIndex): SdkDoc {
  const index = data.index ?? {}
  const rootItem = index[data.root]
  if (!rootItem) return createEmptyRustSdkDoc()

  const reExports = collectReExports(rootItem, index)
  const reExportMap = new Map<string, number>()
  for (const re of reExports) {
    reExportMap.set(re.name, re.targetId)
  }

  // Find the III struct (main client)
  const iiiId = reExportMap.get(MAIN_STRUCT)
  const methods: FunctionDoc[] = iiiId ? extractMethodsFromStruct(iiiId, index, METHOD_EXCLUDE) : []

  // Find register_worker free function
  let registerWorkerItem: RustDocItem | undefined
  const mod = rootItem.inner.module

  // Also collect direct items defined in the root module (e.g. InitOptions, register_worker)
  if (mod?.items) {
    for (const itemId of mod.items) {
      const item = index[itemId]
      if (!item || !item.name) continue
      const kind = getItemKind(item)
      if (kind !== 'use' && kind !== 'module' && !reExportMap.has(item.name)) {
        reExportMap.set(item.name, itemId)
      }
    }
  }
  if (mod?.items) {
    for (const itemId of mod.items) {
      const item = index[itemId]
      if (item?.name === 'register_worker' && getItemKind(item) === 'function') {
        registerWorkerItem = item
        break
      }
    }
  }

  const entryPoint = registerWorkerItem
    ? buildMethodDoc(registerWorkerItem, index)
    : null

  // Extract Logger
  const loggerId = reExportMap.get('Logger')
  let loggerSection: LoggerDoc | undefined
  if (loggerId != null) {
    const loggerItem = index[loggerId]
    if (loggerItem && getItemKind(loggerItem) === 'struct') {
      const loggerMethods = extractMethodsFromStruct(loggerId, index, new Set(['new']))
      const description = extractDocs(loggerItem)
      if (description || loggerMethods.length > 0) {
        loggerSection = { description, methods: loggerMethods }
      }
    }
  }

  // Extract types
  const types: TypeDoc[] = []
  for (const typeName of EXPORTED_TYPES) {
    const typeId = reExportMap.get(typeName)
    if (typeId == null) continue
    const typeItem = index[typeId]
    if (!typeItem) continue

    const kind = getItemKind(typeItem)
    if (kind === 'struct') {
      types.push(extractStructType(typeItem, index))
    } else if (kind === 'enum') {
      types.push(extractEnumType(typeItem, index))
    } else if (kind === 'type_alias') {
      types.push({
        name: typeItem.name ?? '',
        description: extractDocs(typeItem),
        fields: [],
        codeBlock: `type ${typeItem.name} = ...`,
      })
    }
  }

  return {
    metadata: {
      language: 'rust',
      languageLabel: 'Rust',
      title: 'Rust SDK',
      description: 'API reference for the iii SDK for Rust.',
      installCommand: 'cargo add iii-sdk',
      importExample: 'use iii_sdk::{register_worker, InitOptions};',
    },
    initialization: {
      entryPoint: entryPoint ?? {
        name: 'register_worker',
        signature: 'register_worker(address: &str, options: InitOptions) -> III',
        description: 'Create and return a connected SDK instance.',
        params: [
          { name: 'address', type: '&str', description: 'WebSocket URL of the III engine.', required: true },
          { name: 'options', type: 'InitOptions', description: 'Configuration for worker metadata and OTel.', required: true },
        ],
        returns: { type: 'Result<III, IIIError>', description: 'Connected SDK instance.' },
        examples: [],
      },
    },
    methods,
    types,
    loggerSection,
  }
}

export function parseRustdoc(jsonPath: string): SdkDoc {
  let data: RustDocIndex
  try {
    data = JSON.parse(readFileSync(jsonPath, 'utf-8'))
  } catch {
    console.warn(`[parse-rustdoc] Could not read ${jsonPath}, using empty data`)
    return createEmptyRustSdkDoc()
  }

  return parseRustdocData(data)
}

function createEmptyRustSdkDoc(): SdkDoc {
  return {
    metadata: {
      language: 'rust',
      languageLabel: 'Rust',
      title: 'Rust SDK',
      description: 'API reference for the iii SDK for Rust.',
      installCommand: 'cargo add iii-sdk',
      importExample: 'use iii_sdk::{register_worker, InitOptions};',
    },
    initialization: {
      entryPoint: {
        name: 'register_worker',
        signature: 'register_worker(address: &str, options: InitOptions) -> III',
        description: 'Create and return a connected SDK instance.',
        params: [],
        returns: { type: 'Result<III, IIIError>', description: '' },
        examples: [],
      },
    },
    methods: [],
    types: [],
  }
}

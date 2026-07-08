import { readFileSync } from 'node:fs'
import type { FunctionDoc, ModuleDoc, ParamDoc, SdkDoc, TypeDoc as TypeDocType, TypeGroup, SubpathExport } from '../types.mjs'

interface TypeDocReflection {
  id: number
  name: string
  kind: number
  kindString?: string
  comment?: { summary?: { kind: string; text: string }[]; blockTags?: { tag: string; content: { kind: string; text: string }[] }[] }
  signatures?: TypeDocReflection[]
  parameters?: TypeDocReflection[]
  type?: any
  flags?: { isOptional?: boolean }
  children?: TypeDocReflection[]
  groups?: { title: string; children: number[] }[]
}

const KIND_ENUM = 8
const KIND_FUNCTION = 64
const KIND_CLASS = 128
const KIND_INTERFACE = 256
const KIND_PROPERTY = 1024
const KIND_METHOD = 2048
const KIND_TYPE_ALIAS = 2097152

function extractText(summary?: { kind: string; text: string }[]): string {
  if (!summary) return ''
  return summary.map(s => s.text).join('').trim()
}

/** A symbol re-exported only for back-compat (e.g. old 0.19 paths) carries a
 * `@deprecated` block tag. Drop those so the generated surface reflects the
 * current API, not legacy aliases. */
function isDeprecated(comment?: TypeDocReflection['comment']): boolean {
  return !!comment?.blockTags?.some(t => t.tag === '@deprecated')
}

function extractExamples(comment?: TypeDocReflection['comment']): string[] {
  if (!comment?.blockTags) return []
  return comment.blockTags
    .filter(t => t.tag === '@example')
    .map(t => {
      const text = t.content.map(c => c.text).join('')
      const match = text.match(/```\w*\n([\s\S]*?)```/)
      return match ? match[1].trim() : text.trim()
    })
    .filter(Boolean)
}

function typeToString(type: any): string {
  if (!type) return 'unknown'
  if (type.type === 'intrinsic') return type.name
  if (type.type === 'literal') return JSON.stringify(type.value)
  if (type.type === 'reference') {
    const args = type.typeArguments?.map(typeToString).join(', ')
    return args ? `${type.name}<${args}>` : type.name
  }
  if (type.type === 'union') return type.types.map(typeToString).join(' | ')
  if (type.type === 'intersection') return type.types.map(typeToString).join(' & ')
  if (type.type === 'reflection') {
    const decl = type.declaration
    if (decl?.signatures) {
      const sig = decl.signatures[0]
      const params = sig.parameters?.map((p: any) => `${p.name}: ${typeToString(p.type)}`).join(', ') ?? ''
      return `(${params}) => ${typeToString(sig.type)}`
    }
    if (decl?.children?.length) {
      const fields = decl.children
        .map((c: any) => {
          const opt = c.flags?.isOptional ? '?' : ''
          const t = c.type ? typeToString(c.type) : c.signatures ? typeToString({ type: 'reflection', declaration: c }) : 'unknown'
          return `${c.name}${opt}: ${t}`
        })
        .join('; ')
      return `{ ${fields} }`
    }
    return 'object'
  }
  if (type.type === 'array') return `${typeToString(type.elementType)}[]`
  if (type.type === 'tuple') return `[${(type.elements ?? []).map(typeToString).join(', ')}]`
  if (type.type === 'named-tuple-member') return `${type.name}: ${typeToString(type.element)}`
  if (type.type === 'optional') return `${typeToString(type.elementType)}?`
  if (type.type === 'rest') return `...${typeToString(type.elementType)}`
  if (type.type === 'typeOperator') return `${type.operator} ${typeToString(type.target)}`
  if (type.type === 'query') return `typeof ${typeToString(type.queryType)}`
  if (type.type === 'predicate') return type.targetType ? `${type.name} is ${typeToString(type.targetType)}` : `${type.name}`
  if (type.type === 'indexedAccess') return `${typeToString(type.objectType)}[${typeToString(type.indexType)}]`
  if (type.type === 'templateLiteral') return 'string'
  if (type.type === 'mapped') return 'object'
  return type.name || 'unknown'
}

function extractParams(sig: TypeDocReflection): ParamDoc[] {
  return (sig.parameters ?? []).map(p => ({
    name: p.name,
    type: typeToString(p.type),
    description: extractText(p.comment?.summary),
    required: !(p.type?.type === 'union' && p.type.types?.some((t: any) => t.type === 'intrinsic' && t.name === 'undefined')),
  }))
}

function reflectionToFunction(ref: TypeDocReflection): FunctionDoc | null {
  const sig = ref.signatures?.[0] ?? ref
  const comment = sig.comment ?? ref.comment
  const params = sig.parameters ?? []
  const returnType = typeToString(sig.type ?? ref.type)

  return {
    name: ref.name,
    signature: `(${params.map(p => `${p.name}: ${typeToString(p.type)}`).join(', ')}) => ${returnType}`,
    description: extractText(comment?.summary),
    params: extractParams(sig),
    returns: { type: returnType, description: '' },
    examples: extractExamples(comment),
  }
}

function extractFieldsFromChildren(children: any[]): ParamDoc[] {
  return children
    .filter((f: any) => f.kind === KIND_PROPERTY || f.kind === KIND_METHOD)
    .map((f: any) => ({
      name: f.name,
      type: typeToString(f.type ?? f.signatures?.[0]?.type),
      description: extractText(f.comment?.summary ?? f.signatures?.[0]?.comment?.summary),
      required: !(f.flags?.isOptional),
    }))
}

function extractTypesFrom(children: TypeDocReflection[], skipNames: Set<string>): TypeDocType[] {
  const types: TypeDocType[] = []

  for (const child of children) {
    if (skipNames.has(child.name)) continue
    if (isDeprecated(child.comment)) continue

    if (child.kind === KIND_INTERFACE) {
      types.push({
        name: child.name,
        description: extractText(child.comment?.summary),
        fields: extractFieldsFromChildren(child.children ?? []),
      })
    } else if (child.kind === KIND_TYPE_ALIAS) {
      const directChildren = child.children ?? child.type?.declaration?.children
      if (directChildren?.length) {
        types.push({
          name: child.name,
          description: extractText(child.comment?.summary),
          fields: extractFieldsFromChildren(directChildren),
        })
      } else if (child.type?.type) {
        types.push({
          name: child.name,
          description: extractText(child.comment?.summary),
          fields: [],
          codeBlock: `type ${child.name} = ${typeToString(child.type)}`,
        })
      }
    } else if (child.kind === KIND_ENUM) {
      const fields: ParamDoc[] = (child.children ?? []).map((f: any) => ({
        name: f.name,
        type: f.type ? typeToString(f.type) : 'string',
        description: extractText(f.comment?.summary),
        required: true,
      }))
      types.push({
        name: child.name,
        description: extractText(child.comment?.summary),
        fields,
      })
    } else if (child.kind === KIND_CLASS) {
      const props = (child.children ?? []).filter((f: any) => f.kind === KIND_PROPERTY)
      types.push({
        name: child.name,
        description: extractText(child.comment?.summary),
        fields: extractFieldsFromChildren(props),
      })
    }
  }

  return types
}

function dedupeTypes(types: TypeDocType[]): TypeDocType[] {
  const seen = new Set<string>()
  const out: TypeDocType[] = []
  for (const t of types) {
    if (seen.has(t.name)) continue
    seen.add(t.name)
    out.push(t)
  }
  return out.sort((a, b) => a.name.localeCompare(b.name))
}

/** Strip a trailing `/index` so `http/index` → `http`. */
function submoduleName(moduleName: string): string {
  return moduleName.replace(/\/index$/, '')
}

/**
 * Attribute each type to the subpath that exports it. TypeDoc attributes the
 * class *declaration* to whichever entry resolves it (often `index`), so we go
 * by export membership instead: process submodules first (alpha) and `index`
 * last, so a type re-exported by `index` for convenience is credited to the
 * submodule that actually owns it (e.g. `ChannelReader` → `iii-sdk/channel`).
 */
function buildTypeGroups(
  modules: TypeDocReflection[],
  typesByName: Map<string, TypeDocType>,
  packageName: string,
): TypeGroup[] {
  const assigned = new Set<string>()
  const bySub = new Map<string, TypeDocType[]>()
  const subs = [...new Set(modules.map(m => submoduleName(m.name)))].filter(s => s !== 'index').sort()
  for (const sub of [...subs, 'index']) {
    const m = modules.find(mm => submoduleName(mm.name) === sub)
    if (!m) continue
    const subpath = sub === 'index' ? packageName : `${packageName}/${sub}`
    for (const child of m.children ?? []) {
      if (isDeprecated(child.comment)) continue
      const t = typesByName.get(child.name)
      if (!t || assigned.has(child.name)) continue
      assigned.add(child.name)
      if (!bySub.has(subpath)) bySub.set(subpath, [])
      bySub.get(subpath)!.push(t)
    }
  }
  const groups: TypeGroup[] = [...bySub.entries()].map(([subpath, types]) => ({
    subpath,
    types: types.sort((a, b) => a.name.localeCompare(b.name)),
  }))
  // Root package first, then submodules alphabetically.
  groups.sort((a, b) =>
    a.subpath === packageName ? -1 : b.subpath === packageName ? 1 : a.subpath.localeCompare(b.subpath),
  )
  return groups
}

type Metadata = {
  language: 'node'
  languageLabel: string
  title: string
  description: string
  installCommand: string
  importExample: string
  packageName: string
}

// ---------------------------------------------------------------------------
// Core SDK (iii-sdk / iii-browser-sdk): a client + worker entry point
// ---------------------------------------------------------------------------

export function parseNodeTypedoc(jsonPath: string): SdkDoc {
  return parseTypedoc(jsonPath, {
    language: 'node',
    languageLabel: 'TypeScript',
    title: 'Node.js SDK',
    description: 'API reference for the iii SDK for Node.js / TypeScript.',
    installCommand: 'npm install iii-sdk',
    importExample: "import { registerWorker } from 'iii-sdk'",
    packageName: 'iii-sdk',
  })
}

export function parseBrowserTypedoc(jsonPath: string): SdkDoc {
  return parseTypedoc(jsonPath, {
    language: 'node',
    languageLabel: 'TypeScript',
    title: 'Browser SDK',
    description: 'API reference for the iii SDK for Browser / TypeScript.',
    installCommand: 'npm install iii-browser-sdk',
    importExample: "import { registerWorker } from 'iii-browser-sdk'",
    packageName: 'iii-browser-sdk',
  })
}

export function parseTypedoc(jsonPath: string, metadata: Metadata): SdkDoc {
  const raw = JSON.parse(readFileSync(jsonPath, 'utf-8'))

  const modules: TypeDocReflection[] = raw.children ?? []
  const indexModule = modules.find((m: any) => submoduleName(m.name) === 'index')
  const allChildren = indexModule?.children ?? []

  const registerWorker = allChildren.find(c => c.name === 'registerWorker')
  // The client handle is `IIIClient` after the cross-language rename; the
  // browser SDK still defines `ISdk`, so fall back to it.
  const client =
    allChildren.find(c => c.name === 'IIIClient' && (c.kind === KIND_CLASS || c.kind === KIND_INTERFACE)) ??
    allChildren.find(c => c.name === 'ISdk')

  const methods: FunctionDoc[] = []
  if (client?.children) {
    for (const child of client.children) {
      if (child.kind !== KIND_METHOD) continue
      if (isDeprecated(child.comment) || isDeprecated(child.signatures?.[0]?.comment)) continue
      const fn = reflectionToFunction(child)
      if (fn) methods.push(fn)
    }
  }

  const skipTypes = new Set(['IIIClient', 'ISdk'])
  const types = dedupeTypes(modules.flatMap(m => extractTypesFrom(m.children ?? [], skipTypes)))
  const typeGroups = buildTypeGroups(modules, new Map(types.map(t => [t.name, t])), metadata.packageName)

  const entryFn = registerWorker ? reflectionToFunction(registerWorker) : null

  // Names owned by a dedicated submodule. The root barrel re-exports many of
  // them for convenience, but (like buildTypeGroups) credit each to its
  // submodule so the `iii-sdk` row stops advertising the moved 0.19 root paths.
  const submoduleOwned = new Set<string>()
  for (const m of modules) {
    if (submoduleName(m.name) === 'index') continue
    for (const c of m.children ?? []) if (!isDeprecated(c.comment)) submoduleOwned.add(c.name)
  }
  const subpathExports: SubpathExport[] = []
  for (const m of modules) {
    const sub = submoduleName(m.name)
    let names = (m.children ?? []).filter(c => !isDeprecated(c.comment)).map(c => c.name)
    if (sub === 'index') names = names.filter(n => !submoduleOwned.has(n))
    names = [...new Set(names)].sort() // dedup + stable order
    if (names.length === 0) continue
    subpathExports.push({
      path: sub === 'index' ? metadata.packageName : `${metadata.packageName}/${sub}`,
      description: sub === 'index' ? 'Core SDK exports' : `${sub} submodule`,
      exports: names,
    })
  }

  return {
    metadata,
    initialization: {
      entryPoint: entryFn ?? {
        name: 'registerWorker',
        signature: '(address: string, options?: InitOptions) => IIIClient',
        description: '',
        params: [],
        returns: { type: 'IIIClient', description: '' },
        examples: [],
      },
    },
    methods,
    types,
    typeGroups,
    subpathExports,
  }
}

// ---------------------------------------------------------------------------
// Helpers library (@iii-dev/helpers): per-submodule functions + types
// ---------------------------------------------------------------------------

const HELPERS_MODULE_DESCRIPTIONS: Record<string, string> = {
  http: 'HTTP request/response types, auth config, and the `http` helper.',
  queue: 'Queue enqueue result types.',
  stream: 'Stream trigger configs, change events, IO inputs, and update operations.',
  'worker-connection-manager': 'RBAC auth and registration callback types.',
  observability: 'Logger, OpenTelemetry config, and span helpers.',
}

export function parseHelpersTypedoc(jsonPath: string): SdkDoc {
  const raw = JSON.parse(readFileSync(jsonPath, 'utf-8'))
  const rawModules: TypeDocReflection[] = raw.children ?? []

  const modules: ModuleDoc[] = []
  for (const m of rawModules) {
    const sub = submoduleName(m.name)
    const children = m.children ?? []
    const functions: FunctionDoc[] = []
    for (const c of children) {
      if (c.kind === KIND_FUNCTION) {
        const fn = reflectionToFunction(c)
        if (fn) functions.push(fn)
      }
    }
    const types = dedupeTypes(extractTypesFrom(children, new Set()))
    if (functions.length === 0 && types.length === 0) continue
    modules.push({
      name: sub,
      importPath: `import { ... } from '@iii-dev/helpers/${sub}'`,
      description: HELPERS_MODULE_DESCRIPTIONS[sub] ?? '',
      functions: functions.sort((a, b) => a.name.localeCompare(b.name)),
      types,
    })
  }

  modules.sort((a, b) => a.name.localeCompare(b.name))

  return {
    metadata: {
      language: 'node',
      languageLabel: 'TypeScript',
      title: 'Helpers (Node.js)',
      description: 'API reference for the @iii-dev/helpers package (Node.js / TypeScript).',
      installCommand: 'npm install @iii-dev/helpers',
      importExample: "import { http } from '@iii-dev/helpers/http'",
      packageName: '@iii-dev/helpers',
    },
    isLibrary: true,
    initialization: { entryPoint: { name: '', signature: '', description: '', params: [], returns: { type: '', description: '' }, examples: [] } },
    methods: [],
    types: [],
    modules,
  }
}

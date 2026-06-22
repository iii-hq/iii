import { readFileSync } from 'node:fs'
import type { FunctionDoc, ModuleDoc, ParamDoc, SdkDoc, TypeDoc, TypeGroup } from '../types.mjs'

interface GriffeObject {
  name: string
  kind: string
  path?: string
  target_path?: string
  docstring?: { value?: string; parsed?: { kind: string; value?: any }[] }
  members?: Record<string, GriffeObject>
  parameters?: { name: string; annotation?: any; default?: string | null }[]
  returns?: { annotation?: any }
  labels?: string[]
  annotation?: any
  value?: string | null
}

// ---------------------------------------------------------------------------
// Annotation / docstring helpers
// ---------------------------------------------------------------------------

export function annotationToString(ann: any): string {
  if (!ann) return ''
  if (typeof ann === 'string') return ann
  switch (ann.cls) {
    case 'ExprName':
      return ann.name ?? ''
    case 'ExprBinOp': {
      const left = annotationToString(ann.left)
      const right = annotationToString(ann.right)
      const op = ann.operator ?? '|'
      if (!left && !right) return ''
      if (!left) return right
      if (!right) return left
      return `${left} ${op} ${right}`
    }
    case 'ExprSubscript': {
      const base = annotationToString(ann.left)
      const slice = annotationToString(ann.slice)
      if (!base) return ''
      return slice ? `${base}[${slice}]` : base
    }
    case 'ExprTuple':
      return (ann.elements ?? []).map(annotationToString).filter(Boolean).join(', ')
    case 'ExprAttribute':
      return ann.member ?? ann.name ?? ''
    default:
      return ann.source ?? ''
  }
}

function extractDocstring(obj: GriffeObject): string {
  if (!obj.docstring?.parsed) {
    return obj.docstring?.value
      ?.split(/\n\n(?:Args|Attributes|Returns|Raises|Examples?|Note|Yields|See Also):/)[0]
      ?.trim() ?? ''
  }
  const textParts = obj.docstring.parsed.filter(p => p.kind === 'text')
  return textParts.map(p => (typeof p.value === 'string' ? p.value : '')).join('\n').trim()
}

function extractParams(obj: GriffeObject): ParamDoc[] {
  const docParams: Record<string, string> = {}
  if (obj.docstring?.parsed) {
    for (const section of obj.docstring.parsed) {
      if (section.kind === 'parameters' && Array.isArray(section.value)) {
        for (const param of section.value as any[]) {
          if (param.name && param.description) docParams[param.name] = param.description
        }
      }
    }
  }
  return (obj.parameters ?? [])
    .filter(p => p.name !== 'self' && p.name !== 'cls')
    .map(p => ({
      name: p.name,
      type: annotationToString(p.annotation) || 'Any',
      description: docParams[p.name] ?? '',
      required: p.default === undefined || p.default === null,
    }))
}

function extractExamples(obj: GriffeObject): string[] {
  if (!obj.docstring?.value) return []
  const exampleMatch = obj.docstring.value.match(/Examples?:\n([\s\S]*?)(?:\n\n(?:[A-Z]\w*:)|\n\n\S|$)/)
  if (!exampleMatch) return []
  const code = exampleMatch[1]
    .split('\n')
    .map(l => l.replace(/^\s{4,8}/, ''))
    .map(l => l.replace(/^>>> /, ''))
    .map(l => l.replace(/^\.\.\. /, ''))
    .filter(l => l.trim())
    .join('\n')
  return code ? [code] : []
}

function isAsync(obj: GriffeObject): boolean {
  return (obj.labels ?? []).includes('async')
}

function buildSignature(obj: GriffeObject): string {
  const asyncPrefix = isAsync(obj) ? 'async ' : ''
  const params = (obj.parameters ?? [])
    .filter(p => p.name !== 'self' && p.name !== 'cls')
    .map(p => {
      const annStr = annotationToString(p.annotation)
      const ann = annStr ? `: ${annStr}` : ''
      const def = p.default !== undefined && p.default !== null ? ` = ${p.default}` : ''
      return `${p.name}${ann}${def}`
    })
    .join(', ')
  const retStr = annotationToString(obj.returns?.annotation)
  const ret = retStr ? ` -> ${retStr}` : ''
  return `${asyncPrefix}(${params})${ret}`
}

function griffeToFunction(obj: GriffeObject): FunctionDoc {
  return {
    name: obj.name,
    signature: buildSignature(obj),
    description: extractDocstring(obj),
    params: extractParams(obj),
    returns: { type: annotationToString(obj.returns?.annotation) || 'None', description: '' },
    examples: extractExamples(obj),
  }
}

function extractAttributeDescriptions(obj: GriffeObject): Record<string, string> {
  const docstring = obj.docstring?.value ?? ''
  const attrMatch = docstring.match(/Attributes:\n([\s\S]*?)(?:\n\n\S|\n\n$|$)/)
  if (!attrMatch) return {}
  const result: Record<string, string> = {}
  let currentAttr = ''
  let currentDesc = ''
  for (const line of attrMatch[1].split('\n')) {
    const attrLine = line.match(/^\s{4,8}(\w+):\s*(.*)/)
    if (attrLine) {
      if (currentAttr) result[currentAttr] = currentDesc.trim()
      currentAttr = attrLine[1]
      currentDesc = attrLine[2]
    } else if (currentAttr && line.match(/^\s{8,}/)) {
      currentDesc += ' ' + line.trim()
    }
  }
  if (currentAttr) result[currentAttr] = currentDesc.trim()
  return result
}

function griffeToType(obj: GriffeObject): TypeDoc {
  // A module-level type alias (`X = Callable[...]`) shows up as an attribute.
  if (obj.kind === 'attribute') {
    const ann = annotationToString(obj.annotation)
    return {
      name: obj.name,
      description: extractDocstring(obj),
      fields: [],
      codeBlock: ann ? `${obj.name} = ${ann}` : undefined,
    }
  }

  const fields: ParamDoc[] = []
  const attrDescs = extractAttributeDescriptions(obj)
  if (obj.members) {
    for (const [name, member] of Object.entries(obj.members)) {
      if (name.startsWith('_')) continue
      if (member.kind === 'attribute') {
        fields.push({
          name: member.name,
          type: annotationToString(member.annotation) || 'Any',
          description: extractDocstring(member) || attrDescs[member.name] || '',
          required: member.value === undefined || member.value === null,
        })
      }
    }
  }
  if (fields.length === 0 && obj.parameters) fields.push(...extractParams(obj))
  return { name: obj.name, description: extractDocstring(obj), fields }
}

// ---------------------------------------------------------------------------
// Path index + alias resolution
// ---------------------------------------------------------------------------

function buildIndex(root: GriffeObject, rootPath: string): Map<string, GriffeObject> {
  const index = new Map<string, GriffeObject>()
  const walk = (obj: GriffeObject, path: string) => {
    index.set(path, obj)
    if (obj.members) {
      for (const [name, member] of Object.entries(obj.members)) {
        walk(member, `${path}.${name}`)
      }
    }
  }
  walk(root, rootPath)
  return index
}

/** Follow an alias chain to its real definition (class/function/attribute). */
function resolve(obj: GriffeObject | undefined, index: Map<string, GriffeObject>, depth = 0): GriffeObject | undefined {
  if (!obj || depth > 10) return obj
  if (obj.kind === 'alias' && obj.target_path) {
    return resolve(index.get(obj.target_path), index, depth + 1)
  }
  return obj
}

const PUBLIC_SUBMODULES = ['channel', 'errors', 'trigger', 'runtime', 'engine', 'protocol', 'internal', 'utils', 'stream', 'state']

/** Collect resolved public members of a module, keyed by exported name. */
function collectResolved(
  moduleObj: GriffeObject | undefined,
  index: Map<string, GriffeObject>,
  pkg: string,
): Map<string, GriffeObject> {
  const out = new Map<string, GriffeObject>()
  if (!moduleObj?.members) return out
  for (const [name, member] of Object.entries(moduleObj.members)) {
    if (name.startsWith('_')) continue
    const resolved = resolve(member, index)
    if (!resolved) continue
    // Keep only definitions that live in this package (skip typing.* etc.).
    if (resolved.path && !resolved.path.startsWith(`${pkg}.`) && resolved.path !== pkg) continue
    out.set(name, resolved)
  }
  return out
}

// ---------------------------------------------------------------------------
// Core SDK page (iii)
// ---------------------------------------------------------------------------

export function parseGriffe(jsonPath: string): SdkDoc {
  const raw = JSON.parse(readFileSync(jsonPath, 'utf-8'))
  const root: GriffeObject = raw['iii'] ?? raw
  const index = buildIndex(root, 'iii')
  const rootMembers = root.members ?? {}

  // Client + entry point (resolved through the root re-export aliases).
  const client = resolve(rootMembers['IIIClient'], index)
  const methods: FunctionDoc[] = []
  if (client?.members) {
    for (const [name, member] of Object.entries(client.members)) {
      if (name.startsWith('_')) continue
      if (member.kind === 'function') methods.push(griffeToFunction(member))
    }
  }
  methods.sort((a, b) => a.name.localeCompare(b.name))

  const registerWorker = resolve(rootMembers['register_worker'], index)

  // Public type surface = root re-exports + public submodule members.
  const publicDefs = new Map<string, GriffeObject>()
  for (const [name, obj] of collectResolved(root, index, 'iii')) publicDefs.set(name, obj)
  for (const sub of PUBLIC_SUBMODULES) {
    for (const [name, obj] of collectResolved(rootMembers[sub], index, 'iii')) {
      if (!publicDefs.has(name)) publicDefs.set(name, obj)
    }
  }

  const types: TypeDoc[] = []
  for (const [name, obj] of publicDefs) {
    if (name === 'IIIClient') continue
    if (obj.kind === 'class') types.push(griffeToType(obj))
    else if (obj.kind === 'attribute') {
      const t = griffeToType(obj)
      if (t.codeBlock) types.push(t)
    }
  }
  types.sort((a, b) => a.name.localeCompare(b.name))

  // Attribute each type to its owning submodule (submodules first, root last)
  // so re-exports from the package root don't all collapse into `iii`.
  const home = new Map<string, string>()
  const noteHome = (name: string, obj: GriffeObject, subpath: string) => {
    if (!home.has(name) && (obj.kind === 'class' || obj.kind === 'attribute')) home.set(name, subpath)
  }
  for (const sub of PUBLIC_SUBMODULES) {
    for (const [name, obj] of collectResolved(rootMembers[sub], index, 'iii')) noteHome(name, obj, `iii.${sub}`)
  }
  for (const [name, obj] of collectResolved(root, index, 'iii')) noteHome(name, obj, 'iii')

  const bySub = new Map<string, TypeDoc[]>()
  for (const t of types) {
    const subpath = home.get(t.name) ?? 'iii'
    if (!bySub.has(subpath)) bySub.set(subpath, [])
    bySub.get(subpath)!.push(t)
  }
  const typeGroups: TypeGroup[] = [...bySub.entries()].map(([subpath, ts]) => ({
    subpath,
    types: ts.sort((a, b) => a.name.localeCompare(b.name)),
  }))
  typeGroups.sort((a, b) => (a.subpath === 'iii' ? -1 : b.subpath === 'iii' ? 1 : a.subpath.localeCompare(b.subpath)))

  const entryFn = registerWorker
    ? griffeToFunction(registerWorker)
    : {
        name: 'register_worker',
        signature: '(address: str, options: InitOptions | None = None) -> IIIClient',
        description: 'Create an IIIClient and auto-start its connection task.',
        params: [],
        returns: { type: 'IIIClient', description: '' },
        examples: [],
      }

  return {
    metadata: {
      language: 'python',
      languageLabel: 'Python',
      title: 'Python SDK',
      description: 'API reference for the iii SDK for Python.',
      installCommand: 'pip install iii-sdk',
      importExample: 'from iii import register_worker, InitOptions',
      packageName: 'iii',
    },
    initialization: { entryPoint: entryFn },
    methods,
    types,
    typeGroups,
  }
}

// ---------------------------------------------------------------------------
// Helpers library page (iii_helpers)
// ---------------------------------------------------------------------------

const HELPERS_MODULE_DESCRIPTIONS: Record<string, string> = {
  http: 'HTTP request/response types, auth config, and the `http` helper.',
  queue: 'Queue enqueue result types.',
  stream: 'Stream trigger configs, change events, IO inputs, and update operations.',
  worker_connection_manager: 'RBAC auth and registration callback types.',
  observability: 'Logger, OpenTelemetry config, and span helpers.',
}

export function parseHelpersGriffe(jsonPath: string): SdkDoc {
  const raw = JSON.parse(readFileSync(jsonPath, 'utf-8'))
  const root: GriffeObject = raw['iii_helpers'] ?? raw
  const index = buildIndex(root, 'iii_helpers')
  const rootMembers = root.members ?? {}

  const modules: ModuleDoc[] = []
  for (const [subName, subObj] of Object.entries(rootMembers)) {
    if (subObj.kind !== 'module' || subName.startsWith('_')) continue
    const resolved = collectResolved(subObj, index, 'iii_helpers')
    const functions: FunctionDoc[] = []
    const types: TypeDoc[] = []
    for (const [name, obj] of resolved) {
      if (obj.kind === 'function') functions.push(griffeToFunction(obj))
      else if (obj.kind === 'class') types.push(griffeToType(obj))
      else if (obj.kind === 'attribute') {
        const t = griffeToType(obj)
        if (t.codeBlock) types.push(t)
      }
    }
    if (functions.length === 0 && types.length === 0) continue
    functions.sort((a, b) => a.name.localeCompare(b.name))
    types.sort((a, b) => a.name.localeCompare(b.name))
    modules.push({
      name: subName,
      importPath: `from iii_helpers.${subName} import ...`,
      description: HELPERS_MODULE_DESCRIPTIONS[subName] ?? '',
      functions,
      types,
    })
  }
  modules.sort((a, b) => a.name.localeCompare(b.name))

  return {
    metadata: {
      language: 'python',
      languageLabel: 'Python',
      title: 'Helpers (Python)',
      description: 'API reference for the iii-helpers package (Python).',
      installCommand: 'pip install iii-helpers',
      importExample: 'from iii_helpers.http import http',
      packageName: 'iii_helpers',
    },
    isLibrary: true,
    initialization: { entryPoint: { name: '', signature: '', description: '', params: [], returns: { type: '', description: '' }, examples: [] } },
    methods: [],
    types: [],
    modules,
  }
}

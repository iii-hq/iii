import { readFileSync } from 'node:fs'
import { EXPAND_PARAMS_RE, parseExpandMarker } from '../types.mjs'
import type { FunctionDoc, ModuleDoc, ParamDoc, SdkDoc, TypeDoc, TypeGroup } from '../types.mjs'

interface GriffeObject {
  name: string
  kind: string
  path?: string
  target_path?: string
  docstring?: { value?: string; parsed?: { kind: string; value?: any }[] }
  members?: Record<string, GriffeObject>
  parameters?: { name: string; annotation?: any; default?: string | null; kind?: string }[]
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
      // Don't filter empty renderings: `Callable[[], None]` carries an empty
      // ExprList that must survive as `[]`.
      return (ann.elements ?? []).map(annotationToString).join(', ')
    case 'ExprList':
      return `[${(ann.elements ?? []).map(annotationToString).join(', ')}]`
    case 'ExprAttribute':
      return ann.member ?? ann.name ?? ''
    default:
      return ann.source ?? ''
  }
}

function extractDocstring(obj: GriffeObject): string {
  if (!obj.docstring?.parsed) {
    return (
      obj.docstring?.value
        ?.split(/\n\n(?:Args|Attributes|Returns|Raises|Examples?|Note|Yields|See Also):/)[0]
        ?.replace(EXPAND_PARAMS_RE, '')
        .trim() ?? ''
    )
  }
  const textParts = obj.docstring.parsed.filter(p => p.kind === 'text')
  return textParts
    .map(p => (typeof p.value === 'string' ? p.value : ''))
    .join('\n')
    .replace(EXPAND_PARAMS_RE, '')
    .trim()
}

/**
 * Parse a google-style docstring section (e.g. `Args:`, `Attributes:`) from the
 * raw docstring text into a `name -> description` map. Griffe's `dump` output
 * frequently omits the pre-parsed sections and gives us only the raw `value`,
 * so we recover entries by indentation: the section header sets a base indent,
 * entry lines (`name: text` or `name (type): text`) sit one level deeper, and
 * more-indented lines continue the previous entry. A dedent back to the header
 * level (or another section header) ends the section.
 */
function parseDocSection(docstring: string, section: string): Record<string, string> {
  const lines = docstring.split('\n')
  let headerIndent = -1
  let i = 0
  for (; i < lines.length; i++) {
    const header = lines[i].match(/^(\s*)(\w[\w ]*):\s*$/)
    if (header && header[2] === section) {
      headerIndent = header[1].length
      i++
      break
    }
  }
  if (headerIndent < 0) return {}

  const result: Record<string, string> = {}
  let currentName = ''
  let currentDesc = ''
  let entryIndent = -1
  for (; i < lines.length; i++) {
    const line = lines[i]
    if (line.trim() === '') continue
    const indent = line.match(/^(\s*)/)![1].length
    if (indent <= headerIndent) break // dedent ends the section
    const entry = line.match(/^\s*(\w+)\s*(?:\([^)]*\))?:\s*(.*)$/)
    if (entryIndent < 0) entryIndent = indent
    if (entry && indent === entryIndent) {
      if (currentName) result[currentName] = currentDesc.trim()
      currentName = entry[1]
      currentDesc = entry[2]
    } else if (currentName) {
      currentDesc += ' ' + line.trim()
    }
  }
  if (currentName) result[currentName] = currentDesc.trim()
  return result
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
  // Griffe's `dump` output often omits parsed sections (only the raw docstring
  // `value` is present), so fall back to parsing the google-style parameter
  // block directly, the same recovery used for attribute descriptions.
  if (Object.keys(docParams).length === 0 && obj.docstring?.value) {
    for (const name of ['Args', 'Arguments', 'Parameters']) {
      const parsed = parseDocSection(obj.docstring.value, name)
      if (Object.keys(parsed).length) {
        Object.assign(docParams, parsed)
        break
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

/** griffe emits the return annotation directly as `returns`; tolerate the
 * wrapped `{ annotation }` shape too. */
function returnAnnotation(obj: GriffeObject): any {
  const r: any = obj.returns
  if (!r) return undefined
  return r.annotation ?? r
}

function buildSignature(obj: GriffeObject): string {
  const asyncPrefix = isAsync(obj) ? 'async ' : ''
  const parts: string[] = []
  let starEmitted = false
  for (const p of obj.parameters ?? []) {
    if (p.name === 'self' || p.name === 'cls') continue
    if (p.kind === 'keyword-only' && !starEmitted) {
      parts.push('*')
      starEmitted = true
    }
    const annStr = annotationToString(p.annotation)
    const ann = annStr ? `: ${annStr}` : ''
    const def = p.default !== undefined && p.default !== null ? ` = ${p.default}` : ''
    parts.push(`${p.name}${ann}${def}`)
  }
  const retStr = annotationToString(returnAnnotation(obj))
  const ret = retStr ? ` -> ${retStr}` : ''
  return `${asyncPrefix}(${parts.join(', ')})${ret}`
}

function griffeToFunction(obj: GriffeObject): FunctionDoc {
  return {
    name: obj.name,
    signature: buildSignature(obj),
    description: extractDocstring(obj),
    params: extractParams(obj),
    returns: { type: annotationToString(returnAnnotation(obj)) || 'None', description: '' },
    examples: extractExamples(obj),
    ...parseExpandMarker(obj.docstring?.value),
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

/**
 * Whether a class attribute is required. A plain annotation with no value is
 * required; any default makes it optional; a pydantic `Field(...)` call is
 * required unless it carries a default (positional first arg, `default=`, or
 * `default_factory=`).
 */
function attributeRequired(member: GriffeObject): boolean {
  const value: any = member.value
  if (value === undefined || value === null) return true
  if (value?.cls === 'ExprCall' && annotationToString(value.function) === 'Field') {
    const args: any[] = value.arguments ?? []
    return !args.some(
      a => a.cls !== 'ExprKeyword' || a.name === 'default' || a.name === 'default_factory',
    )
  }
  return false
}

/** Pydantic class configuration, never a data field. */
function isModelConfig(member: GriffeObject): boolean {
  return member.name === 'model_config'
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
        if (isModelConfig(member)) continue
        fields.push({
          name: member.name,
          type: annotationToString(member.annotation) || 'Any',
          description: extractDocstring(member) || attrDescs[member.name] || '',
          required: attributeRequired(member),
        })
      } else if (member.kind === 'function') {
        // Methods document as callable-typed rows so their parameters survive
        // (e.g. `Trigger.unregister`, `TriggerTypeRef.register_trigger`).
        fields.push({
          name: member.name,
          type: buildSignature(member),
          description: extractDocstring(member).split('\n')[0] ?? '',
          required: true,
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

/** True if `path` belongs to one of our own packages (not typing.*, pydantic.*). */
function isLocalPath(path: string | undefined, pkgs: string[]): boolean {
  if (!path) return true
  return pkgs.some(pkg => path === pkg || path.startsWith(`${pkg}.`))
}

/**
 * Collect resolved public members of a module, keyed by exported name.
 *
 * `pkgs` lists the packages whose definitions count as part of the public
 * surface. The core SDK page passes both `iii` and `iii_helpers` so that types
 * the SDK re-exports from the helpers package (e.g. `EnqueueResult`) resolve and
 * are documented, matching the Node SDK page. Symbols from third-party packages
 * (typing, pydantic, ...) are still dropped.
 */
function collectResolved(
  moduleObj: GriffeObject | undefined,
  index: Map<string, GriffeObject>,
  pkgs: string[],
): Map<string, GriffeObject> {
  const out = new Map<string, GriffeObject>()
  if (!moduleObj?.members) return out
  for (const [name, member] of Object.entries(moduleObj.members)) {
    if (name.startsWith('_')) continue
    const resolved = resolve(member, index)
    if (!resolved) continue
    if (!isLocalPath(resolved.path, pkgs)) continue
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
  // The dump also carries the `iii_helpers` package; index it so aliases the SDK
  // re-exports from helpers (e.g. `EnqueueResult`) resolve to their definitions.
  const helpersRoot: GriffeObject | undefined = raw['iii_helpers']
  if (helpersRoot) {
    for (const [path, obj] of buildIndex(helpersRoot, 'iii_helpers')) index.set(path, obj)
  }
  const rootMembers = root.members ?? {}

  // Client + entry point. Methods come from the implementation class that
  // `register_worker` returns (`III`), never the `IIIClient` Protocol: the
  // Protocol elides keyword-only parameters and return annotations, so pages
  // generated from it under-document the real surface.
  const registerWorkerAlias = resolve(rootMembers['register_worker'], index)
  const clientClassName = annotationToString(returnAnnotation(registerWorkerAlias ?? {} as GriffeObject)) || 'III'
  let client: GriffeObject | undefined
  for (const obj of index.values()) {
    if (obj.kind === 'class' && obj.name === clientClassName) {
      client = obj
      break
    }
  }
  client ??= resolve(rootMembers['IIIClient'], index)
  const methods: FunctionDoc[] = []
  if (client?.members) {
    for (const [name, member] of Object.entries(client.members)) {
      if (name.startsWith('_')) continue
      if (member.kind === 'function') methods.push(griffeToFunction(member))
    }
  }
  methods.sort((a, b) => a.name.localeCompare(b.name))

  const registerWorker = registerWorkerAlias

  // Public type surface = root re-exports + public submodule members. Both our
  // own package and the re-exported helpers types count.
  const SDK_PKGS = ['iii', 'iii_helpers']
  const publicDefs = new Map<string, GriffeObject>()
  for (const [name, obj] of collectResolved(root, index, SDK_PKGS)) publicDefs.set(name, obj)
  for (const sub of PUBLIC_SUBMODULES) {
    for (const [name, obj] of collectResolved(rootMembers[sub], index, SDK_PKGS)) {
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
    for (const [name, obj] of collectResolved(rootMembers[sub], index, SDK_PKGS)) noteHome(name, obj, `iii.${sub}`)
  }
  for (const [name, obj] of collectResolved(root, index, SDK_PKGS)) noteHome(name, obj, 'iii')

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
    const resolved = collectResolved(subObj, index, ['iii_helpers'])
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

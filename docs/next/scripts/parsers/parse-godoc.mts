import { readFileSync } from 'node:fs'
import { EXPAND_PARAMS_RE, INTERNAL_RE, isInternalDoc, parseExpandMarker } from '../types.mjs'
import type {
  FunctionDoc,
  ModuleDoc,
  PackageValues,
  ParamDoc,
  SdkDoc,
  TypeDoc,
  TypeGroup,
  ValueGroupDoc,
} from '../types.mjs'

/**
 * Parser for the go/doc JSON that `docs/next/scripts/godoc-json` dumps for the
 * Go SDK module (`sdk/packages/go/iii/api-docs.json`). One dump feeds two pages:
 *
 * - `sdk-go.mdx` (`parseGodoc`): the root package `iii` and every other public
 *   package that is not a helper (today `iii/filter`), shaped like the other SDK
 *   pages: `RegisterWorker` is the entry point, the `Client` methods are the
 *   Methods, every other exported function is under Functions, then the
 *   package-level constants and variables and the types, grouped per package.
 * - `helpers-go.mdx` (`parseHelpersGodoc`): the packages under `iii/helpers/`
 *   and `iii/observability`, one section per package, like the other helpers
 *   pages.
 *
 * Packages are discovered from the dump, never listed here, so a package added
 * to the module shows up on the next run without a change to this file.
 */

// ---------------------------------------------------------------------------
// godoc-json model (docs/next/scripts/godoc-json/main.go)
// ---------------------------------------------------------------------------

interface GoSpan {
  kind: 'text' | 'doclink' | 'link'
  text: string
  url?: string
}
interface GoBlock {
  kind: 'paragraph' | 'heading' | 'code' | 'list'
  spans?: GoSpan[]
  code?: string
  ordered?: boolean
  items?: GoBlock[][]
}
interface GoDoc {
  raw: string
  blocks: GoBlock[]
  /** The `Deprecated: ` paragraph, prefix removed. */
  deprecated?: GoBlock
}
interface GoParam {
  name: string
  type: string
  variadic?: boolean
}
interface GoExample {
  suffix?: string
  doc?: string
  code: string
}
interface GoFunc {
  name: string
  recv?: string
  signature: string
  funcType: string
  params: GoParam[]
  results: GoParam[]
  doc: GoDoc
  examples: GoExample[]
}
interface GoValueSpec {
  name: string
  type?: string
  value?: string
  doc: GoDoc
}
interface GoValue {
  doc: GoDoc
  specs: GoValueSpec[]
}
interface GoField {
  name: string
  type: string
  embedded?: boolean
  jsonTag?: string
  doc: GoDoc
}
interface GoType {
  name: string
  kind: string
  decl?: string
  doc: GoDoc
  fields: GoField[]
  interfaceMethods: GoField[]
  incomplete?: boolean
  consts: GoValue[]
  vars: GoValue[]
  funcs: GoFunc[]
  methods: GoFunc[]
  examples: GoExample[]
}
interface GoPackage {
  importPath: string
  relPath: string
  name: string
  /** The name the package's doc or examples import it under, when not `name`. */
  importName?: string
  synopsis: string
  doc: GoDoc
  consts: GoValue[]
  vars: GoValue[]
  funcs: GoFunc[]
  types: GoType[]
  examples: GoExample[]
}
interface GoModule {
  module: string
  goVersion: string
  packages: GoPackage[]
}

// ---------------------------------------------------------------------------
// Page layout
// ---------------------------------------------------------------------------

const MAIN_TYPE = 'Client'
const ENTRY_POINT = 'RegisterWorker'
const INSTALL_COMMAND = 'go get github.com/iii-hq/iii/sdk/packages/go/iii@latest'
const IMPORT_EXAMPLE = 'import iii "github.com/iii-hq/iii/sdk/packages/go/iii"'

/** Package roots (relative to the module) documented on the helpers page. */
const HELPER_ROOTS = ['helpers', 'observability']

const HELPERS_EMPTY_NOTE =
  'No Go helper package exists yet. The packages under `iii/helpers/` and `iii/observability` are listed here once they ship.'

/** Whether a package (by its path relative to the module) belongs on the helpers page. */
export function isGoHelpersPackage(relPath: string): boolean {
  return HELPER_ROOTS.some(root => relPath === root || relPath.startsWith(`${root}/`))
}

// ---------------------------------------------------------------------------
// Prose
// ---------------------------------------------------------------------------

const MARKERS_RE = new RegExp(`${EXPAND_PARAMS_RE.source}|${INTERNAL_RE.source}`, 'g')

/**
 * A bracketed name go/doc did not resolve as a doc link (a field such as
 * `[TriggerRequest.Timeout]`): the author meant a link, so it renders as a code
 * span like a resolved one.
 * Not after an identifier or `]`, so `map[string]any` and `Handler[Req]` stay.
 */
const UNRESOLVED_LINK_RE = /(?<![\w\]])\[((?:[a-z]\w*\.)?[A-Z]\w*(?:\.[A-Z]\w*)?)\](?![(\[])/g

/**
 * Go doc comments are plain text, not Markdown: what MDX would read as markup
 * is escaped (`<`, `>` and `*`, e.g. `*InvocationError` or `Bearer <token>`).
 * Backtick-quoted words, a common Go doc idiom, stay code spans. Braces are
 * left to the renderer, which escapes them everywhere.
 */
function escapeText(text: string): string {
  return text
    .replace(MARKERS_RE, '')
    .replace(UNRESOLVED_LINK_RE, '`$1`')
    .split(/(`[^`]*`)/)
    .map((part, i) =>
      i % 2 === 1 ? part : part.replace(/[<>*]/g, c => (c === '<' ? '&lt;' : c === '>' ? '&gt;' : '\\*')),
    )
    .join('')
}

/** Inline text. A `[Name]` doc link renders as a code span, as rustdoc intra-doc links do. */
function renderSpans(spans: GoSpan[] = []): string {
  return spans
    .map(span => {
      if (span.kind === 'doclink') return `\`${span.text}\``
      if (span.kind === 'link') return `[${span.text === span.url ? span.url : escapeText(span.text)}](${span.url})`
      return escapeText(span.text)
    })
    .join('')
    .trim()
}

/**
 * How code blocks render: `fence` as a fenced block (descriptions), `inline` as a
 * code span when the block is one line and dropped otherwise (table cells),
 * `drop` not at all (method prose, whose code blocks become examples).
 */
type CodeMode = 'fence' | 'inline' | 'drop'

function fence(code: string): string {
  return '```go\n' + code + '\n```'
}

function renderBlock(block: GoBlock, code: CodeMode): string {
  switch (block.kind) {
    case 'paragraph':
      return renderSpans(block.spans)
    case 'heading': {
      const text = renderSpans(block.spans)
      return text ? `**${text}**` : ''
    }
    case 'code': {
      const text = block.code ?? ''
      if (!text || code === 'drop') return ''
      if (code === 'fence') return fence(text)
      return text.includes('\n') || text.includes('`') ? '' : `\`${text}\``
    }
    case 'list':
      return (block.items ?? [])
        .map((item, i) => {
          const text = item.map(b => renderBlock(b, code === 'fence' ? 'inline' : code)).filter(Boolean).join(' ')
          return `${block.ordered ? `${i + 1}.` : '-'} ${text}`
        })
        .join('\n')
  }
  return ''
}

function renderBlocks(blocks: GoBlock[], code: CodeMode = 'fence'): string {
  return blocks
    .map(b => renderBlock(b, code))
    .filter(Boolean)
    .join('\n\n')
}

/** The first paragraph, for a one-line table cell (as the Rust parser does). */
function summary(doc: GoDoc): string {
  const first = doc.blocks.find(b => b.kind === 'paragraph')
  return first ? renderSpans(first.spans) : ''
}

function deprecationNote(doc: GoDoc): string {
  return doc.deprecated ? renderSpans(doc.deprecated.spans) : ''
}

/**
 * Prefixes a table cell with the item's deprecation, as the Rust parser does
 * for deprecated method rows (a cell has no room for a callout).
 */
function withDeprecation(text: string, doc: GoDoc): string {
  if (!doc.deprecated) return text
  const note = deprecationNote(doc)
  const sentence = note ? ` ${note.charAt(0).toUpperCase()}${note.slice(1)}`.replace(/[^.]$/, '$&.') : ''
  return `**Deprecated.**${sentence}${text ? ` ${text}` : ''}`
}

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

function byName<T extends { name: string }>(a: T, b: T): number {
  return a.name < b.name ? -1 : a.name > b.name ? 1 : 0
}

function resultType(results: GoParam[]): string {
  if (results.length === 0) return ''
  if (results.length === 1 && !results[0].name) return results[0].type
  return `(${results.map(r => (r.name ? `${r.name} ${r.type}` : r.type)).join(', ')})`
}

/** Inline text as plain words: doc links by name, links by their text. */
function plainText(spans: GoSpan[] = []): string {
  return spans.map(span => span.text).join('').replace(MARKERS_RE, '').trim()
}

/** Text as `//` comment lines of at most 80 columns, for the top of a code example. */
function commentLines(text: string): string {
  const lines: string[] = []
  let line = ''
  for (const word of text.split(/\s+/).filter(Boolean)) {
    if (line && line.length + 1 + word.length > 77) {
      lines.push(`// ${line}`)
      line = word
    } else {
      line = line ? `${line} ${word}` : word
    }
  }
  if (line) lines.push(`// ${line}`)
  return lines.join('\n')
}

/**
 * A function's doc comment split into prose and examples. Its code blocks move
 * to the Example tab; a paragraph that ends with ':' right before one is that
 * code's lead-in ("To turn telemetry off:"), so it moves along as a comment on
 * the example's first lines instead of dangling at the end of the prose.
 */
function splitFuncDoc(blocks: GoBlock[]): { prose: GoBlock[]; examples: string[] } {
  const prose: GoBlock[] = []
  const examples: string[] = []
  for (let i = 0; i < blocks.length; i++) {
    const block = blocks[i]
    const next = blocks[i + 1]
    if (block.kind === 'paragraph' && next?.kind === 'code' && next.code && plainText(block.spans).endsWith(':')) {
      examples.push(`${commentLines(plainText(block.spans))}\n${next.code}`)
      i++
    } else if (block.kind === 'code') {
      if (block.code) examples.push(block.code)
    } else {
      prose.push(block)
    }
  }
  return { prose, examples }
}

/**
 * A function or method. Code blocks in its doc comment (with their lead-in
 * sentence, see `splitFuncDoc`) and its testable examples (`ExampleX`
 * functions) become the Example tab; the prose stays the description. A
 * variadic parameter is optional.
 */
function buildFunc(fn: GoFunc, opts: { name?: string; extraExamples?: GoExample[] } = {}): FunctionDoc {
  const { prose, examples: docExamples } = splitFuncDoc(fn.doc.blocks)
  const examples = [...docExamples, ...[...fn.examples, ...(opts.extraExamples ?? [])].map(e => e.code)].filter(Boolean)
  const note = deprecationNote(fn.doc)
  return {
    name: opts.name ?? fn.name,
    signature: fn.signature,
    description: renderBlocks(prose, 'drop'),
    params: fn.params.map(p => ({
      name: p.name || '_',
      type: p.type,
      description: '',
      required: !p.variadic,
    })),
    returns: { type: resultType(fn.results), description: '' },
    examples,
    ...parseExpandMarker(fn.doc.raw),
    ...(fn.doc.deprecated ? { deprecated: { note } } : {}),
  }
}

/** Every function a package exports, constructors included (go/doc files those under their result type). */
function packageFuncs(pkg: GoPackage): GoFunc[] {
  return [...pkg.funcs, ...pkg.types.flatMap(t => t.funcs)].filter(fn => !isInternalDoc(fn.doc.raw)).sort(byName)
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/** A type doc that states the zero value of every field (`A zero field is unset`). */
const ZERO_VALUE_TYPE_RE = /\bzero (value|field)/i
/** A field doc that says what its zero value means. */
const ZERO_VALUE_FIELD_RE = /\b(nil|empty|zero|absent|unset|omitted|optional|defaults?|overrides?)\b/i
/** Field types whose zero value is nil. */
const NILABLE_TYPE_RE = /^(\*|\[\]|map\[|func\(|chan |<-chan |chan<- |interface\{|any$|json\.RawMessage$)/

/**
 * Go has no optional fields, so "required" is read from what the code says:
 * a field with a json tag is optional when the tag has `omitempty`/`omitzero`
 * or the type is a pointer (the wire contract). A field without one is
 * optional when its zero value is nil, when the type's doc says its zero
 * fields are unset, or when the field's own doc says what its zero value does.
 */
function fieldRequired(field: GoField, zeroValueType: boolean): boolean {
  if (field.jsonTag !== undefined && field.jsonTag !== '-') {
    const options = field.jsonTag.split(',').slice(1)
    return !(options.includes('omitempty') || options.includes('omitzero') || field.type.startsWith('*'))
  }
  if (zeroValueType || NILABLE_TYPE_RE.test(field.type)) return false
  return !ZERO_VALUE_FIELD_RE.test(field.doc.raw)
}

/** Constant or variable rows: one per name, the value in the type column. */
function valueRows(groups: GoValue[]): ParamDoc[] {
  const rows: ParamDoc[] = []
  for (const group of groups) {
    if (isInternalDoc(group.doc.raw)) continue
    const specs = group.specs.filter(s => !isInternalDoc(s.doc.raw))
    const single = specs.length === 1 && !specs[0].doc.raw.trim()
    for (const spec of specs) {
      const doc = single ? group.doc : spec.doc
      rows.push({
        name: spec.name,
        type: spec.value || spec.type || '-',
        description: withDeprecation(renderBlocks(doc.blocks, 'inline'), group.doc.deprecated ? group.doc : doc),
        required: true,
      })
    }
  }
  return rows
}

/** Function rows (constructors, methods, interface methods): signature and summary. */
function funcRow(name: string, type: string, doc: GoDoc): ParamDoc {
  return { name, type, description: withDeprecation(summary(doc), doc), required: true }
}

/**
 * A type. The table lists its exported fields (or interface methods), then the
 * constants and variables of the type, its constructors and its methods, the
 * way the Rust parser lists a struct's fields then its methods. A type that is
 * not a struct or an interface also shows its declaration.
 */
function buildType(t: GoType): TypeDoc {
  const zeroValueType = ZERO_VALUE_TYPE_RE.test(t.doc.raw)
  const fields: ParamDoc[] = []
  for (const field of t.fields) {
    if (isInternalDoc(field.doc.raw)) continue
    fields.push({
      name: field.name,
      type: field.type,
      description: withDeprecation(renderBlocks(field.doc.blocks, 'inline'), field.doc),
      required: fieldRequired(field, zeroValueType),
    })
  }
  for (const method of t.interfaceMethods) {
    if (isInternalDoc(method.doc.raw)) continue
    fields.push(funcRow(method.name, method.embedded ? 'embedded' : method.type, method.doc))
  }
  fields.push(...valueRows(t.consts), ...valueRows(t.vars))
  for (const fn of [...t.funcs, ...t.methods]) {
    if (isInternalDoc(fn.doc.raw)) continue
    fields.push(funcRow(fn.name, fn.funcType, fn.doc))
  }
  const description = [renderBlocks(t.doc.blocks, 'fence'), ...t.examples.map(e => fence(e.code))]
    .filter(Boolean)
    .join('\n\n')
  return {
    name: t.name,
    description,
    fields,
    ...(t.decl ? { codeBlock: t.decl } : {}),
  }
}

// ---------------------------------------------------------------------------
// Constants and variables
// ---------------------------------------------------------------------------

/**
 * Package-level `const` / `var` groups. A declaration of one value without a
 * doc of its own (e.g. `DefaultEngineURL`) puts its doc in its row, and
 * consecutive ones share a table.
 */
function valueGroups(groups: GoValue[]): ValueGroupDoc[] {
  const out: ValueGroupDoc[] = []
  let previousSingle = false
  for (const group of groups) {
    if (isInternalDoc(group.doc.raw)) continue
    const specs = group.specs.filter(s => !isInternalDoc(s.doc.raw))
    if (specs.length === 0) continue
    const single = specs.length === 1 && !specs[0].doc.raw.trim()
    if (single && previousSingle) {
      out[out.length - 1].values.push({
        name: specs[0].name,
        value: specs[0].value ?? '',
        description: withDeprecation(renderBlocks(group.doc.blocks, 'inline'), group.doc),
      })
      continue
    }
    previousSingle = single
    out.push({
      description: single ? '' : withDeprecation(renderBlocks(group.doc.blocks, 'fence'), group.doc),
      values: specs.map(spec => ({
        name: spec.name,
        value: spec.value ?? '',
        description: single
          ? withDeprecation(renderBlocks(group.doc.blocks, 'inline'), group.doc)
          : withDeprecation(renderBlocks(spec.doc.blocks, 'inline'), spec.doc),
      })),
    })
  }
  return out
}

// ---------------------------------------------------------------------------
// Pages
// ---------------------------------------------------------------------------

function readModule(jsonPath: string): GoModule | null {
  try {
    return JSON.parse(readFileSync(jsonPath, 'utf-8')) as GoModule
  } catch {
    console.warn(`[parse-godoc] Could not read ${jsonPath}`)
    return null
  }
}

/** `iii` for the root package, `iii/filter` for a subpackage. */
function subpathOf(mod: GoModule, pkg: GoPackage): string {
  const base = mod.module.split('/').pop() || pkg.name
  return pkg.relPath ? `${base}/${pkg.relPath}` : base
}

/** A package's synopsis, plus its testable examples when asked. */
function packageDescription(pkg: GoPackage, withExamples: boolean): string {
  const examples = withExamples ? pkg.examples.map(e => fence(e.code)) : []
  return [escapeText(pkg.synopsis), ...examples].filter(Boolean).join('\n\n')
}

const EMPTY_FUNCTION: FunctionDoc = {
  name: '',
  signature: '',
  description: '',
  params: [],
  returns: { type: '', description: '' },
  examples: [],
}

export function parseGodoc(jsonPath: string): SdkDoc {
  const metadata: SdkDoc['metadata'] = {
    language: 'go',
    languageLabel: 'Go',
    title: 'Go SDK',
    description: 'API reference for the iii SDK for Go.',
    installCommand: INSTALL_COMMAND,
    importExample: IMPORT_EXAMPLE,
    packageName: 'iii',
  }
  const fallbackEntry: FunctionDoc = {
    ...EMPTY_FUNCTION,
    name: ENTRY_POINT,
    signature: 'func RegisterWorker(url string, opts ...Option) *Client',
    returns: { type: '*Client', description: '' },
  }
  const mod = readModule(jsonPath)
  if (!mod) return { metadata, initialization: { entryPoint: fallbackEntry }, methods: [], types: [] }

  const packages = mod.packages.filter(p => !isGoHelpersPackage(p.relPath))
  const root = packages.find(p => p.relPath === '')
  const client = root?.types.find(t => t.name === MAIN_TYPE)
  const methods = (client?.methods ?? []).filter(m => !isInternalDoc(m.doc.raw)).map(m => buildFunc(m))
  // The package's own examples (`Example`, `Example_suffix`) are complete
  // workers: they illustrate the entry point.
  const entry = root ? packageFuncs(root).find(fn => fn.name === ENTRY_POINT) : undefined
  const entryPoint = entry ? buildFunc(entry, { extraExamples: root?.examples }) : fallbackEntry

  const functions: FunctionDoc[] = []
  const types: TypeDoc[] = []
  const paramOnlyTypes: TypeDoc[] = []
  const typeGroups: TypeGroup[] = []
  const packageValues: PackageValues[] = []
  const typeHome = new Map<string, string>()
  for (const pkg of packages) {
    const subpath = subpathOf(mod, pkg)
    // Functions of a subpackage are named as callers write them: `filter.Compile`.
    const qualifier = pkg.relPath ? `${pkg.name}.` : ''
    for (const fn of packageFuncs(pkg)) {
      if (!pkg.relPath && fn.name === ENTRY_POINT) continue
      functions.push(buildFunc(fn, { name: `${qualifier}${fn.name}` }))
    }

    const groupTypes: TypeDoc[] = []
    for (const t of pkg.types) {
      if ((!pkg.relPath && t.name === MAIN_TYPE) || isInternalDoc(t.doc.raw)) continue
      const doc = buildType(t)
      // Deprecated types leave the Types section but still expand under the
      // parameters of the deprecated methods that take them.
      if (t.doc.deprecated) {
        paramOnlyTypes.push(doc)
        continue
      }
      const home = typeHome.get(t.name)
      if (home) console.warn(`[parse-godoc] type ${t.name} is in both ${home} and ${subpath}: its anchor is ambiguous`)
      typeHome.set(t.name, subpath)
      types.push(doc)
      groupTypes.push(doc)
    }
    if (groupTypes.length > 0) {
      typeGroups.push({ subpath, description: packageDescription(pkg, pkg.relPath !== ''), types: groupTypes })
    }

    const constants = valueGroups(pkg.consts)
    const variables = valueGroups(pkg.vars)
    if (constants.length > 0 || variables.length > 0) packageValues.push({ subpath, constants, variables })
  }
  types.sort(byName)

  return {
    metadata,
    initialization: { entryPoint },
    methods,
    functions,
    types,
    paramOnlyTypes,
    typeGroups,
    packageValues,
  }
}

export function parseHelpersGodoc(jsonPath: string): SdkDoc {
  const metadata: SdkDoc['metadata'] = {
    language: 'go',
    languageLabel: 'Go',
    title: 'Helpers (Go)',
    description: 'API reference for the helper packages of the iii SDK for Go.',
    installCommand: INSTALL_COMMAND,
    importExample: IMPORT_EXAMPLE,
    packageName: 'iii',
  }
  const mod = readModule(jsonPath)
  const modules: ModuleDoc[] = (mod?.packages ?? [])
    .filter(pkg => isGoHelpersPackage(pkg.relPath))
    .map(pkg => {
      const constants = valueGroups(pkg.consts)
      const variables = valueGroups(pkg.vars)
      return {
        name: pkg.relPath,
        // The name the package's doc or examples import it under (e.g.
        // iiihttp for helpers/http), so the line compiles with the examples.
        importPath: `import ${pkg.importName ? `${pkg.importName} ` : ''}"${pkg.importPath}"`,
        description: packageDescription(pkg, true),
        functions: packageFuncs(pkg).map(fn => buildFunc(fn)),
        types: pkg.types.filter(t => !t.doc.deprecated && !isInternalDoc(t.doc.raw)).map(buildType),
        ...(constants.length > 0 ? { constants } : {}),
        ...(variables.length > 0 ? { variables } : {}),
      }
    })

  return {
    metadata,
    isLibrary: true,
    initialization: { entryPoint: EMPTY_FUNCTION },
    methods: [],
    types: [],
    modules,
    ...(modules.length === 0 ? { libraryEmptyNote: HELPERS_EMPTY_NOTE } : {}),
  }
}

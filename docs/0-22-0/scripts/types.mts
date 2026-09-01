/**
 * Doc-comment marker that flags a method for `expandParams` (see
 * `FunctionDoc.expandParams`). An HTML comment renders as nothing in every
 * language's native doc tooling; parsers detect it and strip it from prose.
 * An optional type list (`<!-- docs:expand-params: TriggerRequest -->`) names
 * additional types to expand when the signature type is a wrapper the reader
 * would not construct directly.
 */
export const EXPAND_PARAMS_MARKER = '<!-- docs:expand-params -->'
export const EXPAND_PARAMS_RE = /<!--\s*docs:expand-params(?::([^>]*?))?\s*-->/

/** Parse the marker out of raw doc text: flag plus optional explicit types. */
export function parseExpandMarker(text: string | undefined): { expandParams: boolean; expandTypes: string[] } {
  const m = text?.match(EXPAND_PARAMS_RE)
  if (!m) return { expandParams: false, expandTypes: [] }
  const expandTypes = (m[1] ?? '')
    .split(',')
    .map(s => s.trim())
    .filter(Boolean)
  return { expandParams: true, expandTypes }
}

/**
 * Doc-comment marker that hides an otherwise-public symbol (type, method, or
 * subpath export) from the generated reference. Like `docs:expand-params`, it
 * is an HTML comment that renders as nothing in native doc tooling; the parsers
 * detect it, drop the carrying item, and strip the marker from any prose they
 * do emit. Use for symbols that are exported for internal wiring but are not
 * meant for the public reference.
 */
export const INTERNAL_MARKER = '<!-- docs:internal -->'
export const INTERNAL_RE = /<!--\s*docs:internal\s*-->/

/** Whether raw doc text flags its symbol as internal (hidden from the reference). */
export function isInternalDoc(text: string | undefined): boolean {
  return INTERNAL_RE.test(text ?? '')
}

/** Remove the internal marker from prose that is still rendered. */
export function stripInternalMarker(text: string): string {
  return text.replace(INTERNAL_RE, '')
}

export interface ParamDoc {
  name: string
  type: string
  description: string
  required: boolean
  fields?: ParamDoc[]
}

export interface FunctionDoc {
  name: string
  signature: string
  description: string
  params: ParamDoc[]
  returns: { type: string; description: string }
  examples: string[]
  /**
   * Set by the `<!-- docs:expand-params -->` marker in the method's source
   * doc-comment: nested type Expandables under this method's parameters render
   * open by default. Declarative and per-language; never hardcode method names.
   */
  expandParams?: boolean
  /**
   * Extra documented type names to expand under this method's parameters, from
   * the marker's optional type list. Covers wrapper signatures (e.g. Rust's
   * `impl Into<TriggerRequestWithMetadata>`) where the type the reader
   * actually constructs never appears in the parameter type.
   */
  expandTypes?: string[]
}

export interface TypeDoc {
  name: string
  description: string
  fields: ParamDoc[]
  codeBlock?: string
}

export interface SubpathExport {
  path: string
  description: string
  exports: string[]
}

export interface LoggerDoc {
  description: string
  methods: FunctionDoc[]
}

/**
 * A group of types that share a subpath / submodule (e.g. `iii-sdk/state`,
 * `iii.channel`, `iii_sdk::channels`). When a core (non-library) page carries
 * `typeGroups`, its Types section is rendered grouped by subpath instead of as
 * one flat list, so the namespace structure stays visible.
 */
export interface TypeGroup {
  subpath: string
  description?: string
  types: TypeDoc[]
}

/**
 * A submodule of a "library" package (e.g. `@iii-dev/helpers/stream`) that has
 * no single client entry point, just a bag of functions and types.
 */
export interface ModuleDoc {
  name: string
  importPath: string
  description: string
  functions: FunctionDoc[]
  types: TypeDoc[]
}

export interface SdkDoc {
  metadata: {
    language: 'node' | 'python' | 'rust'
    languageLabel: string
    title: string
    description: string
    installCommand: string
    importExample: string
    /** Package name used to build subpath-export rows (e.g. `iii-sdk`). */
    packageName?: string
    /** SDK source dir the doc-comments come from; named in the do-not-edit banner. */
    docSourcePath?: string
  }
  /**
   * When true the page is rendered in "library" mode: no Initialization /
   * Methods sections, just the per-submodule groupings in `modules`.
   */
  isLibrary?: boolean
  initialization: {
    entryPoint: FunctionDoc
  }
  methods: FunctionDoc[]
  types: TypeDoc[]
  /**
   * Per-subpath grouping of the same types in `types`. When present and
   * non-empty, the Types section renders grouped by subpath; `types` stays
   * populated (flat) so cross-type links resolve across groups.
   */
  typeGroups?: TypeGroup[]
  subpathExports?: SubpathExport[]
  loggerSection?: LoggerDoc
  modules?: ModuleDoc[]
}

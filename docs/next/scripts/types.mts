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

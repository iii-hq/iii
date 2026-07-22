import type { FunctionDoc, ModuleDoc, SdkDoc, TypeDoc } from '../types.mjs'
import { renderParamsTable, renderReturnsTable, renderTypesIndex, codeBlock } from './components.mjs'

const LANG_MAP: Record<string, string> = {
  node: 'typescript',
  python: 'python',
  rust: 'rust',
}

// Canonical ordering for the core worker-handle methods in the generated docs.
// Matched case-insensitively and ignoring underscores so one list covers both
// camelCase (Node/Browser) and snake_case (Python/Rust). Methods not listed here
// keep their existing alphabetical order and sort after these.
const METHOD_ORDER = ['registertrigger', 'registerfunction', 'trigger', 'registertriggertype', 'unregistertriggertype']

function methodRank(name: string): number {
  const i = METHOD_ORDER.indexOf(name.toLowerCase().replace(/_/g, ''))
  return i === -1 ? METHOD_ORDER.length : i
}

function orderMethods(methods: FunctionDoc[]): FunctionDoc[] {
  return [...methods].sort((a, b) => methodRank(a.name) - methodRank(b.name) || a.name.localeCompare(b.name))
}

function escapeMdxText(value: string): string {
  return value.replace(/`[^`]*`|[{}]/g, (match) => {
    if (match.startsWith('`')) return match
    return match === '{' ? '\\{' : '\\}'
  })
}

function formatMethodSignature(method: FunctionDoc): string {
  const signature = method.signature.trim()
  if (!signature) {
    return method.name
  }
  return signature.startsWith('(') ? `${method.name}${signature}` : signature
}

/**
 * Method parameters render as ParamField entries. A parameter whose type
 * references a documented object type carries that type's field table in a
 * nested collapsed Expandable, so the parameter itself is what expands and the
 * reader sees what to supply without leaving the method. The Types section
 * stays the canonical definition; this is one level deep and generated, so
 * nothing is maintained twice.
 */
function renderParamFields(
  params: FunctionDoc['params'],
  knownTypes: Set<string> | undefined,
  typesByName: Map<string, TypeDoc> | undefined,
  expandParams?: boolean,
  expandTypes?: string[],
): string[] {
  const lines: string[] = []
  const seen = new Set<string>()
  for (const param of params) {
    lines.push(`<ParamField body="${param.name}" type="${attrSafe(param.type)}"${param.required ? ' required' : ''}>`)
    if (param.description) {
      lines.push(`  ${escapeMdxText(param.description)}`)
    }
    // Types referenced by the parameter's own type, plus any explicitly named
    // by the method's expand marker (wrapper signatures never mention the type
    // the reader constructs). Deduped across the method's parameters.
    const candidates = [...(typesByName?.keys() ?? [])].filter(name =>
      new RegExp(`\\b${name}\\b`).test(param.type),
    )
    candidates.push(...(expandTypes ?? []))
    for (const name of candidates) {
      if (seen.has(name)) continue
      seen.add(name)
      const t = typesByName?.get(name)
      if (!t?.fields.length) continue
      lines.push('')
      lines.push(`  <Expandable title="\`${name}\` fields"${expandParams ? ' defaultOpen' : ''}>`)
      for (const field of t.fields) {
        lines.push(`    <ParamField body="${attrSafe(field.name)}" type="${attrSafe(field.type)}"${field.required ? ' required' : ''}>`)
        if (field.description) {
          lines.push(`      ${escapeMdxText(field.description.replace(/\s*\n\s*/g, ' '))}`)
        }
        lines.push('    </ParamField>')
      }
      lines.push('  </Expandable>')
    }
    lines.push('</ParamField>')
    lines.push('')
  }
  return lines
}

/** Value of a JSX string attribute: a double quote would end the attribute. */
function attrSafe(value: string): string {
  return value.replace(/"/g, "'")
}

function renderMethod(method: FunctionDoc, lang: string, knownTypes?: Set<string>, typesByName?: Map<string, TypeDoc>): string {
  const codeLang = LANG_MAP[lang] ?? lang
  const lines: string[] = []

  lines.push(`### ${method.name}`)
  lines.push('')
  lines.push(escapeMdxText(method.description))
  lines.push('')
  lines.push('**Signature**')
  lines.push('')
  lines.push(codeBlock(codeLang, formatMethodSignature(method)))
  lines.push('')

  // Parameters, Returns, and Example(s) share a tabbed panel so a method's
  // reference detail stays in one compact block instead of stacking down the
  // page. A lone section renders on its own without a single-tab group.
  const sections: { title: string; body: string[] }[] = []

  if (method.params.length > 0) {
    sections.push({
      title: 'Parameters',
      body: renderParamFields(method.params, knownTypes, typesByName, method.expandParams, method.expandTypes),
    })
  }

  if (method.returns.description) {
    sections.push({ title: 'Returns', body: [renderReturnsTable(method.returns, knownTypes)] })
  }

  if (method.examples.length > 0) {
    const body: string[] = []
    for (const example of method.examples) {
      body.push(codeBlock(codeLang, example))
      body.push('')
    }
    sections.push({ title: method.examples.length > 1 ? 'Examples' : 'Example', body })
  }

  if (sections.length >= 2) {
    lines.push('<Tabs>')
    for (const section of sections) {
      lines.push(`  <Tab title="${section.title}">`)
      lines.push('')
      lines.push(...section.body)
      lines.push('')
      lines.push('  </Tab>')
    }
    lines.push('</Tabs>')
    lines.push('')
  } else if (sections.length === 1) {
    const section = sections[0]
    lines.push(`#### ${section.title}`)
    lines.push('')
    lines.push(...section.body)
    lines.push('')
  }

  return lines.join('\n')
}

function renderType(type: TypeDoc, codeLang: string, knownTypes: Set<string>, depth = 3): string {
  const lines: string[] = []
  lines.push(`${'#'.repeat(depth)} ${type.name}`)
  lines.push('')
  if (type.description) {
    lines.push(escapeMdxText(type.description))
    lines.push('')
  }
  if (type.codeBlock) {
    lines.push(codeBlock(codeLang, type.codeBlock))
    lines.push('')
  }
  if (type.fields.length > 0) {
    lines.push(renderParamsTable(type.fields, knownTypes))
    lines.push('')
  }
  return lines.join('\n')
}

function renderFrontmatter(doc: SdkDoc): string[] {
  const lines = ['---', `title: "${doc.metadata.title}"`]
  // For the per-language helpers (library) pages the title is "Helpers (Lang)";
  // shorten the sidebar entry to just the language so the Helpers nav group reads
  // "Node.js / Python / Rust" while the page heading keeps the full title.
  const langSuffix = doc.isLibrary ? doc.metadata.title.match(/\(([^)]+)\)\s*$/)?.[1] : undefined
  if (langSuffix) {
    lines.push(`sidebarTitle: "${langSuffix}"`)
  }
  const source = doc.metadata.docSourcePath ?? 'the SDK source'
  lines.push(
    `description: "${doc.metadata.description}"`,
    'owner: "engineering"',
    'type: "reference"',
    '---',
    '',
    '{/* AUTO-GENERATED FILE. Do not edit. Regenerate with docs/next/scripts/generate-api-docs.mts. */}',
    `{/* AI: any skill-check (vale/AI) text fixes belong in the source doc-comments under ${source} (prose) or docs/next/scripts/ (structure/formatting), then regenerate. Never edit this file directly. */}`,
    '',
  )
  return lines
}

function renderLibraryMdx(doc: SdkDoc): string {
  const lang = doc.metadata.language
  const codeLang = LANG_MAP[lang] ?? lang
  const modules = doc.modules ?? []
  // Cross-link any type referenced anywhere in the package.
  const knownTypes = new Set(modules.flatMap(m => m.types.map(t => t.name)))
  const lines: string[] = [...renderFrontmatter(doc)]

  lines.push('## Installation')
  lines.push('')
  lines.push(codeBlock('bash', doc.metadata.installCommand))
  lines.push('')

  lines.push(escapeMdxText(doc.metadata.description))
  lines.push('')

  for (const mod of modules) {
    lines.push(`## ${mod.name}`)
    lines.push('')
    if (mod.description) {
      lines.push(escapeMdxText(mod.description))
      lines.push('')
    }
    lines.push('**Import**')
    lines.push('')
    lines.push(codeBlock(codeLang, mod.importPath))
    lines.push('')

    if (mod.functions.length > 0) {
      lines.push('### Functions')
      lines.push('')
      mod.functions.forEach((fn, i) => {
        if (i > 0) {
          lines.push('---')
          lines.push('')
        }
        lines.push(renderMethod(fn, lang, knownTypes))
      })
    }

    if (mod.types.length > 0) {
      lines.push('### Types')
      lines.push('')
      lines.push(renderTypesIndex(mod.types))
      lines.push('')
      mod.types.forEach((type, i) => {
        if (i > 0) {
          lines.push('---')
          lines.push('')
        }
        lines.push(renderType(type, codeLang, knownTypes))
      })
    }
  }

  return lines.join('\n')
}

export function renderSdkMdx(doc: SdkDoc): string {
  if (doc.isLibrary) return renderLibraryMdx(doc)

  const lang = doc.metadata.language
  const codeLang = LANG_MAP[lang] ?? lang
  const knownTypes = new Set(doc.types.map(t => t.name))
  const typesByName = new Map(doc.types.map(t => [t.name, t]))
  const lines: string[] = [...renderFrontmatter(doc)]

  lines.push('## Installation')
  lines.push('')
  lines.push(codeBlock('bash', doc.metadata.installCommand))
  lines.push('')

  lines.push('## Initialization')
  lines.push('')
  // Document the worker entry point as fully as a method: heading, signature,
  // parameters, returns, and example, not just a blurb + snippet.
  lines.push(renderMethod(doc.initialization.entryPoint, lang, knownTypes, typesByName))

  if (doc.methods.length > 0) {
    lines.push('## Methods')
    lines.push('')
    // A rule between methods: each section is table-heavy, and without a
    // divider one method's Example bleeds visually into the next method.
    orderMethods(doc.methods).forEach((method, i) => {
      if (i > 0) {
        lines.push('---')
        lines.push('')
      }
      lines.push(renderMethod(method, lang, knownTypes, typesByName))
    })
  }

  if (doc.subpathExports && doc.subpathExports.length > 0) {
    lines.push('## Subpath Exports')
    lines.push('')
    lines.push(`The \`${doc.metadata.packageName ?? 'package'}\` package provides additional entry points:`)
    lines.push('')
    lines.push('| Import path | Contents |')
    lines.push('|---|---|')
    for (const exp of doc.subpathExports) {
      // List every export; a truncated row makes the coverage unverifiable and
      // can hide the entry point itself (registerWorker sorts after 10 names).
      lines.push(`| \`${exp.path}\` | \`${exp.exports.join('`, `')}\` |`)
    }
    lines.push('')
  }

  if (doc.loggerSection) {
    lines.push('## Logger')
    lines.push('')
    lines.push(escapeMdxText(doc.loggerSection.description))
    lines.push('')
    for (const method of doc.loggerSection.methods) {
      lines.push(renderMethod(method, lang, knownTypes))
    }
  }

  const typeGroups = (doc.typeGroups ?? []).filter(g => g.types.length > 0)
  if (typeGroups.length > 0) {
    // Grouped: one subsection per subpath so the namespace structure is visible.
    lines.push('## Types')
    lines.push('')
    for (const group of typeGroups) {
      lines.push(`### ${group.subpath}`)
      lines.push('')
      if (group.description) {
        lines.push(escapeMdxText(group.description))
        lines.push('')
      }
      lines.push(renderTypesIndex(group.types))
      lines.push('')
      group.types.forEach((type, i) => {
        if (i > 0) {
          lines.push('---')
          lines.push('')
        }
        lines.push(renderType(type, codeLang, knownTypes, 4))
      })
    }
  } else if (doc.types.length > 0) {
    lines.push('## Types')
    lines.push('')
    lines.push(renderTypesIndex(doc.types))
    lines.push('')
    doc.types.forEach((type, i) => {
      if (i > 0) {
        lines.push('---')
        lines.push('')
      }
      lines.push(renderType(type, codeLang, knownTypes))
    })
  }

  return lines.join('\n')
}

import type { FunctionDoc, ModuleDoc, SdkDoc, TypeDoc } from '../types.mjs'
import { renderParamsTable, renderReturnsTable, renderTypesIndex, codeBlock } from './components.mjs'

const LANG_MAP: Record<string, string> = {
  node: 'typescript',
  python: 'python',
  rust: 'rust',
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

function renderMethod(method: FunctionDoc, lang: string, knownTypes?: Set<string>): string {
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

  if (method.params.length > 0) {
    lines.push('#### Parameters')
    lines.push('')
    lines.push(renderParamsTable(method.params, knownTypes))
    lines.push('')
  }

  if (method.returns.description) {
    lines.push('#### Returns')
    lines.push('')
    lines.push(renderReturnsTable(method.returns, knownTypes))
    lines.push('')
  }

  if (method.examples.length > 0) {
    lines.push(method.examples.length > 1 ? '#### Examples' : '#### Example')
    lines.push('')
    for (const example of method.examples) {
      lines.push(codeBlock(codeLang, example))
      lines.push('')
    }
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
  } else if (type.fields.length > 0) {
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
  lines.push(
    `description: "${doc.metadata.description}"`,
    '---',
    '',
    '{/* AUTO-GENERATED FILE. Do not edit manually. Run docs/next/scripts/generate-api-docs.mts (see the Generate API Docs workflow). */}',
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
      for (const fn of mod.functions) {
        lines.push(renderMethod(fn, lang, knownTypes))
      }
    }

    if (mod.types.length > 0) {
      lines.push('### Types')
      lines.push('')
      lines.push(renderTypesIndex(mod.types))
      lines.push('')
      for (const type of mod.types) {
        lines.push(renderType(type, codeLang, knownTypes))
      }
    }
  }

  return lines.join('\n')
}

export function renderSdkMdx(doc: SdkDoc): string {
  if (doc.isLibrary) return renderLibraryMdx(doc)

  const lang = doc.metadata.language
  const codeLang = LANG_MAP[lang] ?? lang
  const knownTypes = new Set(doc.types.map(t => t.name))
  const lines: string[] = [...renderFrontmatter(doc)]

  lines.push('## Installation')
  lines.push('')
  lines.push(codeBlock('bash', doc.metadata.installCommand))
  lines.push('')

  lines.push('## Initialization')
  lines.push('')
  // Document the worker entry point as fully as a method: heading, signature,
  // parameters, returns, and example — not just a blurb + snippet.
  lines.push(renderMethod(doc.initialization.entryPoint, lang, knownTypes))

  if (doc.methods.length > 0) {
    lines.push('## Methods')
    lines.push('')
    for (const method of doc.methods) {
      lines.push(renderMethod(method, lang, knownTypes))
    }
  }

  if (doc.subpathExports && doc.subpathExports.length > 0) {
    lines.push('## Subpath Exports')
    lines.push('')
    lines.push(`The \`${doc.metadata.packageName ?? 'package'}\` package provides additional entry points:`)
    lines.push('')
    lines.push('| Import path | Contents |')
    lines.push('|---|---|')
    for (const exp of doc.subpathExports) {
      const exports = exp.exports.slice(0, 10).join('`, `')
      const suffix = exp.exports.length > 10 ? ', etc.' : ''
      lines.push(`| \`${exp.path}\` | \`${exports}\`${suffix} |`)
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
      for (const type of group.types) {
        lines.push(renderType(type, codeLang, knownTypes, 4))
      }
    }
  } else if (doc.types.length > 0) {
    lines.push('## Types')
    lines.push('')
    lines.push(renderTypesIndex(doc.types))
    lines.push('')
    for (const type of doc.types) {
      lines.push(renderType(type, codeLang, knownTypes))
    }
  }

  return lines.join('\n')
}

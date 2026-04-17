import type { FunctionDoc, SdkDoc } from '../types.mjs'
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

export function renderSdkMdx(doc: SdkDoc): string {
  const lang = doc.metadata.language
  const codeLang = LANG_MAP[lang] ?? lang
  const knownTypes = new Set(doc.types.map(t => t.name))
  const lines: string[] = []

  lines.push('---')
  lines.push(`title: "${doc.metadata.title}"`)
  lines.push(`description: "${doc.metadata.description}"`)
  lines.push('---')
  lines.push('')
  lines.push('{/* AUTO-GENERATED FILE. Do not edit manually. Run the generate-api-docs pipeline. */}')
  lines.push('')

  lines.push('## Installation')
  lines.push('')
  lines.push(codeBlock('bash', doc.metadata.installCommand))
  lines.push('')

  lines.push('## Initialization')
  lines.push('')
  const entryFn = doc.initialization.entryPoint
  lines.push(escapeMdxText(entryFn.description))
  lines.push('')
  if (entryFn.examples.length > 0) {
    lines.push(codeBlock(codeLang, entryFn.examples[0]))
    lines.push('')
  }

  lines.push('## Methods')
  lines.push('')

  for (const method of doc.methods) {
    lines.push(renderMethod(method, lang, knownTypes))
  }

  if (doc.subpathExports && doc.subpathExports.length > 0) {
    lines.push('## Subpath Exports')
    lines.push('')
    lines.push('The `iii-sdk` package provides additional entry points:')
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

  if (doc.types.length > 0) {
    lines.push('## Types')
    lines.push('')
    lines.push(renderTypesIndex(doc.types))
    lines.push('')
    for (const type of doc.types) {
      if (type.codeBlock) {
        lines.push(`### ${type.name}`)
        lines.push('')
        if (type.description) {
          lines.push(escapeMdxText(type.description))
          lines.push('')
        }
        lines.push(codeBlock(codeLang, type.codeBlock))
        lines.push('')
      } else {
        lines.push(`### ${type.name}`)
        lines.push('')
        if (type.description) {
          lines.push(escapeMdxText(type.description))
          lines.push('')
        }
        if (type.fields.length > 0) {
          lines.push(renderParamsTable(type.fields, knownTypes))
          lines.push('')
        }
      }
    }
  }

  return lines.join('\n')
}

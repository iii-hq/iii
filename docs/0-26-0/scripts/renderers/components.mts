import type { ParamDoc, TypeDoc, ValueDoc } from '../types.mjs'

function escapeTableCell(value: string): string {
  return value
    .replace(/\r?\n/g, '<br />')
    .replace(/`[^`]*`|[{}|]/g, (match) => {
      if (match.startsWith('`')) return match
      if (match === '{') return '\\{'
      if (match === '}') return '\\}'
      return '\\|'
    })
}

function escapeCodeInTableCell(value: string): string {
  return value.replace(/\|/g, '\\|')
}

export function slugify(name: string): string {
  return name.toLowerCase().replace(/[^\w]+/g, '-').replace(/^-+|-+$/g, '')
}

function escapeRegExp(str: string): string {
  return str.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
}

function escapeMdxFragment(value: string): string {
  return escapeTableCell(value)
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
}

function renderTypeCell(typeStr: string, knownTypes?: Set<string>): string {
  if (!knownTypes || knownTypes.size === 0) return `\`${escapeCodeInTableCell(typeStr)}\``
  if (knownTypes.has(typeStr)) return `[\`${escapeCodeInTableCell(typeStr)}\`](#${slugify(typeStr)})`

  const sorted = [...knownTypes].sort((a, b) => b.length - a.length)
  const pattern = sorted.map(n => `\\b${escapeRegExp(n)}\\b`).join('|')
  const regex = new RegExp(pattern, 'g')

  const matches: { start: number; end: number; name: string }[] = []
  let m: RegExpExecArray | null
  while ((m = regex.exec(typeStr)) !== null) {
    matches.push({ start: m.index, end: m.index + m[0].length, name: m[0] })
  }

  if (matches.length === 0) return `\`${escapeCodeInTableCell(typeStr)}\``

  const parts: string[] = []
  let lastEnd = 0

  for (const match of matches) {
    if (match.start > lastEnd) {
      parts.push(escapeMdxFragment(typeStr.slice(lastEnd, match.start)))
    }
    parts.push(`[\`${escapeCodeInTableCell(match.name)}\`](#${slugify(match.name)})`)
    lastEnd = match.end
  }

  if (lastEnd < typeStr.length) {
    parts.push(escapeMdxFragment(typeStr.slice(lastEnd)))
  }

  return parts.join('')
}

type ParamRow = {
  name: string
  type: string
  required: boolean
  description: string
}

function flattenParam(param: ParamDoc, parent = ''): ParamRow[] {
  const fullName = parent ? `${parent}.${param.name}` : param.name
  const current: ParamRow = {
    name: fullName,
    type: param.type,
    required: param.required,
    description: param.description || '-',
  }

  const nested = param.fields?.flatMap(field => flattenParam(field, fullName)) ?? []
  return [current, ...nested]
}

export function renderParamsTable(params: ParamDoc[], knownTypes?: Set<string>): string {
  const rows = params.flatMap(param => flattenParam(param))
  if (rows.length === 0) {
    return ''
  }

  const lines = [
    '| Name | Type | Required | Description |',
    '| --- | --- | --- | --- |',
  ]

  for (const row of rows) {
    lines.push(
      `| \`${escapeTableCell(row.name)}\` | ${renderTypeCell(row.type, knownTypes)} | ${row.required ? 'Yes' : 'No'} | ${escapeTableCell(row.description)} |`,
    )
  }

  return lines.join('\n')
}

export function renderReturnsTable(returns: { type: string; description: string }, knownTypes?: Set<string>): string {
  if (!returns.description && !returns.type) {
    return ''
  }

  return [
    '| Type | Description |',
    '| --- | --- |',
    `| ${renderTypeCell(returns.type || 'void', knownTypes)} | ${escapeTableCell(returns.description || '-')} |`,
  ].join('\n')
}

/**
 * A value written over several lines (a Go struct type with its literal, say) on one line, so its code span
 * stays inside one table cell: each line is trimmed, runs of blanks collapse to one space, and lines join
 * with "; ", except after an opening `{` and before a closing `}`.
 */
export function oneLineValue(value: string): string {
  const lines = value.split(/\r?\n/).map(line => line.trim().replace(/\s+/g, ' ')).filter(Boolean)
  return lines.reduce((out, line) => {
    if (!out) return line
    return out.endsWith('{') || line.startsWith('}') ? `${out} ${line}` : `${out}; ${line}`
  }, '')
}

/**
 * Table rows of a rendered page that MDX would not read as one row: a row with an unclosed code span, or a
 * line right after a row that continues it (a cell value that spilled onto the next line). Fenced code
 * blocks are skipped. Returns one message per problem, with its 1-based line number.
 */
export function findBrokenTableRows(mdx: string): string[] {
  const problems: string[] = []
  let fenced = false
  let previousIsRow = false
  mdx.split('\n').forEach((line, index) => {
    if (line.startsWith('```')) { fenced = !fenced; previousIsRow = false; return }
    if (fenced) return
    const isRow = line.startsWith('|')
    if (isRow && (line.replace(/\\`/g, '').match(/`/g)?.length ?? 0) % 2 === 1) {
      problems.push(`line ${index + 1}: table row has an unclosed code span`)
    } else if (previousIsRow && !isRow && line.trim() !== '') {
      problems.push(`line ${index + 1}: text right after a table row (a cell spilled onto the next line)`)
    }
    previousIsRow = isRow
  })
  return problems
}

/** A table of package-level constants or variables (Go): name, source value, doc. */
export function renderValuesTable(values: ValueDoc[]): string {
  if (values.length === 0) return ''
  const lines = ['| Name | Value | Description |', '| --- | --- | --- |']
  for (const v of values) {
    const value = v.value ? `\`${escapeCodeInTableCell(oneLineValue(v.value))}\`` : '-'
    lines.push(`| \`${escapeTableCell(v.name)}\` | ${value} | ${escapeTableCell(v.description || '-')} |`)
  }
  return lines.join('\n')
}

export function renderTypesIndex(types: TypeDoc[]): string {
  if (types.length === 0) return ''
  return types.map(t => `[\`${t.name}\`](#${slugify(t.name)})`).join(' · ')
}

export function codeBlock(lang: string, code: string): string {
  return '```' + lang + '\n' + code + '\n```'
}

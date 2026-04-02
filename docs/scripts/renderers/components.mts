import type { ParamDoc, TypeDoc } from '../types.mjs'

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

export function renderTypesIndex(types: TypeDoc[]): string {
  if (types.length === 0) return ''
  return types.map(t => `[\`${t.name}\`](#${slugify(t.name)})`).join(' · ')
}

export function codeBlock(lang: string, code: string): string {
  return '```' + lang + '\n' + code + '\n```'
}

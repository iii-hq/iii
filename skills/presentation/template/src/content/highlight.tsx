import { Fragment, type CSSProperties, type ReactNode } from 'react'

/**
 * A dependency-free, multi-color ("rainbow") syntax highlighter for the four
 * languages codegen emits. It classifies tokens into keyword / string / comment
 * / type / function / number / operator / plain and colors each from a
 * dedicated syntax palette (see HighlightStyles). The palette is theme-aware: a
 * darker saturated set on the light sheet, a brighter One-Dark-style set under
 * [data-theme="dark"]. This intentionally overrides the deck's rationed-accent
 * rule for code blocks.
 */
export type HlLang = 'typescript' | 'javascript' | 'rust' | 'python' | 'yaml'

const KEYWORDS: Record<HlLang, Set<string>> = {
  typescript: new Set([
    'export', 'import', 'from', 'interface', 'type', 'async', 'function',
    'return', 'const', 'let', 'await', 'new', 'class', 'extends', 'implements',
    'as', 'void', 'in', 'of', 'typeof', 'keyof', 'string', 'number', 'boolean',
  ]),
  javascript: new Set([
    'export', 'import', 'from', 'async', 'function', 'return', 'const', 'let',
    'await', 'new', 'class', 'extends', 'as', 'in', 'of', 'typeof',
  ]),
  rust: new Set([
    'use', 'pub', 'type', 'struct', 'fn', 'async', 'let', 'await', 'crate',
    'mut', 'match', 'impl', 'mod', 'as', 'return', 'self', 'Self', 'where',
    'dyn', 'move', 'enum', 'trait', 'const', 'static', 'ref',
  ]),
  python: new Set([
    'from', 'import', 'class', 'def', 'return', 'None', 'True', 'False', 'and',
    'or', 'not', 'in', 'is', 'if', 'else', 'elif', 'for', 'while', 'await',
    'async', 'with', 'as', 'pass', 'lambda', 'yield', 'try', 'except', 'raise',
    'str', 'int', 'float', 'bool',
  ]),
  yaml: new Set(['true', 'false', 'null', 'yes', 'no', 'on', 'off']),
}

type TokKind =
  | 'ws'
  | 'comment'
  | 'string'
  | 'number'
  | 'keyword'
  | 'type'
  | 'function'
  | 'punct'
  | 'plain'
interface Tok {
  kind: TokKind
  value: string
}

const isSpace = (c: string) => c === ' ' || c === '\t' || c === '\n' || c === '\r'
const isWordStart = (c: string) => /[A-Za-z_$]/.test(c)
const isWord = (c: string) => /[A-Za-z0-9_$]/.test(c)
const isDigit = (c: string) => c >= '0' && c <= '9'

function tokenize(code: string, lang: HlLang): Tok[] {
  const kw = KEYWORDS[lang]
  const hashComments = lang === 'python'
  const slashComments = lang !== 'python'
  const toks: Tok[] = []
  const n = code.length
  let i = 0

  while (i < n) {
    const c = code[i]

    if (isSpace(c)) {
      let j = i + 1
      while (j < n && isSpace(code[j])) j++
      toks.push({ kind: 'ws', value: code.slice(i, j) })
      i = j
      continue
    }

    if (hashComments && c === '#') {
      let j = i + 1
      while (j < n && code[j] !== '\n') j++
      toks.push({ kind: 'comment', value: code.slice(i, j) })
      i = j
      continue
    }

    if (slashComments && c === '/' && code[i + 1] === '/') {
      let j = i + 2
      while (j < n && code[j] !== '\n') j++
      toks.push({ kind: 'comment', value: code.slice(i, j) })
      i = j
      continue
    }

    if (slashComments && c === '/' && code[i + 1] === '*') {
      let j = i + 2
      while (j < n && !(code[j] === '*' && code[j + 1] === '/')) j++
      j = Math.min(n, j + 2)
      toks.push({ kind: 'comment', value: code.slice(i, j) })
      i = j
      continue
    }

    if (c === '"' || c === "'") {
      let j = i + 1
      while (j < n && code[j] !== c) {
        if (code[j] === '\\') j++
        j++
      }
      j = Math.min(n, j + 1)
      toks.push({ kind: 'string', value: code.slice(i, j) })
      i = j
      continue
    }

    if (isDigit(c)) {
      let j = i + 1
      while (j < n && (isWord(code[j]) || code[j] === '.')) j++
      toks.push({ kind: 'number', value: code.slice(i, j) })
      i = j
      continue
    }

    if (isWordStart(c)) {
      let j = i + 1
      while (j < n && isWord(code[j])) j++
      const word = code.slice(i, j)
      const next = code[j]
      let kind: TokKind
      if (kw.has(word)) kind = 'keyword'
      else if (next === '(' || next === '<') kind = 'function'
      else if (/^[A-Z]/.test(word)) kind = 'type'
      else kind = 'plain'
      toks.push({ kind, value: word })
      i = j
      continue
    }

    // a run of operator / punctuation chars, stopping before tokens a later
    // pass should own
    let j = i + 1
    while (
      j < n &&
      !isSpace(code[j]) &&
      !isWordStart(code[j]) &&
      !isDigit(code[j]) &&
      code[j] !== '"' &&
      code[j] !== "'" &&
      code[j] !== '/'
    ) {
      j++
    }
    toks.push({ kind: 'punct', value: code.slice(i, j) })
    i = j
  }

  return toks
}

function tokenizeYamlValue(text: string, toks: Tok[]) {
  const consts = KEYWORDS.yaml
  const n = text.length
  let i = 0
  while (i < n) {
    const c = text[i]
    if (isSpace(c)) {
      let j = i + 1
      while (j < n && isSpace(text[j])) j++
      toks.push({ kind: 'ws', value: text.slice(i, j) })
      i = j
      continue
    }
    if (c === '#') {
      toks.push({ kind: 'comment', value: text.slice(i) })
      i = n
      continue
    }
    if (c === '"' || c === "'") {
      let j = i + 1
      while (j < n && text[j] !== c) {
        if (text[j] === '\\') j++
        j++
      }
      j = Math.min(n, j + 1)
      toks.push({ kind: 'string', value: text.slice(i, j) })
      i = j
      continue
    }
    if (isDigit(c)) {
      let j = i + 1
      while (j < n && (isWord(text[j]) || text[j] === '.')) j++
      toks.push({ kind: 'number', value: text.slice(i, j) })
      i = j
      continue
    }
    if (isWordStart(c)) {
      let j = i + 1
      while (j < n && isWord(text[j])) j++
      const w = text.slice(i, j)
      toks.push({ kind: consts.has(w) ? 'keyword' : 'plain', value: w })
      i = j
      continue
    }
    let j = i + 1
    while (
      j < n &&
      !isSpace(text[j]) &&
      !isWordStart(text[j]) &&
      !isDigit(text[j]) &&
      text[j] !== '"' &&
      text[j] !== "'" &&
      text[j] !== '#'
    ) {
      j++
    }
    toks.push({ kind: 'punct', value: text.slice(i, j) })
    i = j
  }
}

function tokenizeYaml(code: string): Tok[] {
  const toks: Tok[] = []
  const lines = code.split('\n')
  lines.forEach((line, idx) => {
    if (idx > 0) toks.push({ kind: 'ws', value: '\n' })
    const indent = (line.match(/^\s*/) as RegExpMatchArray)[0]
    if (indent) toks.push({ kind: 'ws', value: indent })
    let rest = line.slice(indent.length)
    if (rest === '') return
    if (rest[0] === '#') {
      toks.push({ kind: 'comment', value: rest })
      return
    }
    if (rest[0] === '-' && (rest[1] === ' ' || rest.length === 1)) {
      toks.push({ kind: 'punct', value: '-' })
      rest = rest.slice(1)
    }
    const keyMatch = rest.match(/^([\w./@-]+)(:)(?=\s|$)/)
    if (keyMatch) {
      toks.push({ kind: 'keyword', value: keyMatch[1] })
      toks.push({ kind: 'punct', value: ':' })
      rest = rest.slice(keyMatch[0].length)
    }
    tokenizeYamlValue(rest, toks)
  })
  return toks
}

const COLOR: Partial<Record<TokKind, string>> = {
  comment: 'var(--cg-comment)',
  string: 'var(--cg-string)',
  number: 'var(--cg-number)',
  keyword: 'var(--cg-keyword)',
  type: 'var(--cg-type)',
  function: 'var(--cg-function)',
  punct: 'var(--cg-punct)',
}

/** The syntax palette. Render once per page. */
export function HighlightStyles() {
  const css = `
.cg-hl {
  --cg-plain: #0a0a0a;
  --cg-comment: #9a948c;
  --cg-keyword: #9333ea;
  --cg-string: #15803d;
  --cg-number: #b45309;
  --cg-type: #0e7490;
  --cg-function: #1d4ed8;
  --cg-punct: #6b6865;
  color: var(--cg-plain);
}
[data-theme="dark"] .cg-hl {
  --cg-plain: #e6e6e6;
  --cg-comment: #7f848e;
  --cg-keyword: #c678dd;
  --cg-string: #98c379;
  --cg-number: #d19a66;
  --cg-type: #e5c07b;
  --cg-function: #61afef;
  --cg-punct: #abb2bf;
}`
  return <style>{css}</style>
}

export function Highlight({ code, lang }: { code: string; lang: HlLang }): ReactNode {
  const toks = lang === 'yaml' ? tokenizeYaml(code) : tokenize(code, lang)
  return (
    <span className="cg-hl">
      {toks.map((tok, idx) => {
        const color = COLOR[tok.kind]
        if (!color) return <Fragment key={idx}>{tok.value}</Fragment>
        const style: CSSProperties = { color }
        if (tok.kind === 'comment') style.fontStyle = 'italic'
        return (
          <span key={idx} style={style}>
            {tok.value}
          </span>
        )
      })}
    </span>
  )
}

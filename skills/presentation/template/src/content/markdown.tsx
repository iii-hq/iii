import { marked } from 'marked'
import { Fragment, type MouseEvent, type ReactNode } from 'react'
import { CodeBlock } from '@/components/schematic/CodeBlock'
import { Highlight, type HlLang } from '@/content/highlight'
import { Mermaid } from '@/content/mermaid'
import { cn } from '@/lib/utils'

/**
 * Renders a markdown string as React, styled in the deck's drafting-sheet
 * system. Fenced code reuses the deck's syntax highlighter where the language
 * is supported (monochrome otherwise); ```mermaid fences render as live,
 * themed diagrams. Spec content keeps its original casing (the deck's lowercase
 * rule applies to chrome copy, not rendered docs).
 *
 * Input is locally-authored, trusted spec markdown (not user input).
 */

const LANG_MAP: Record<string, HlLang> = {
  ts: 'typescript',
  typescript: 'typescript',
  tsx: 'typescript',
  js: 'javascript',
  jsx: 'javascript',
  javascript: 'javascript',
  mjs: 'javascript',
  rs: 'rust',
  rust: 'rust',
  py: 'python',
  python: 'python',
  yml: 'yaml',
  yaml: 'yaml',
}

/** GitHub-style heading slug (matches the anchors the specs link to). */
export function slugify(text: string): string {
  return text
    .trim()
    .toLowerCase()
    .replace(/[^\w\s-]/g, '')
    .replace(/\s+/g, '-')
}

/** Rewrite a markdown href: sibling `*.md` links point into the spec viewer. */
function rewriteHref(href: string): string {
  if (!href) return '#'
  if (href.startsWith('#')) return href
  const m = href.match(/^\.?\/?([\w./-]+?)\.md(?:#.*)?$/)
  if (m) {
    const base = m[1].split('/').pop() ?? m[1]
    return `#/spec/${base.toLowerCase()}`
  }
  return href
}

// marked's Token union is broad; this renderer reads a known subset of fields.
type Tok = { type: string; [k: string]: unknown }

function renderInline(tokens: Tok[] = [], keyPrefix = 'i'): ReactNode[] {
  return tokens.map((t, i) => {
    const key = `${keyPrefix}-${i}`
    const sub = (t.tokens as Tok[] | undefined) ?? []
    switch (t.type) {
      case 'text':
        return sub.length ? (
          <Fragment key={key}>{renderInline(sub, key)}</Fragment>
        ) : (
          <Fragment key={key}>{t.text as string}</Fragment>
        )
      case 'strong':
        return (
          <strong key={key} className="font-semibold text-ink">
            {renderInline(sub, key)}
          </strong>
        )
      case 'em':
        return (
          <em key={key} className="italic">
            {renderInline(sub, key)}
          </em>
        )
      case 'del':
        return (
          <del key={key} className="text-ink-ghost line-through">
            {renderInline(sub, key)}
          </del>
        )
      case 'codespan':
        return (
          <code
            key={key}
            className="font-mono text-[0.88em] text-ink bg-panel px-1 py-0.5 border border-rule"
          >
            {t.text as string}
          </code>
        )
      case 'link': {
        const href = rewriteHref(t.href as string)
        const external = /^https?:/.test(href)
        // a same-page anchor (`#section`) must scroll in place, not change the
        // hash to a bare `#…` (which the router would read as "go home")
        const anchor = href.startsWith('#') && !href.startsWith('#/')
        return (
          <a
            key={key}
            href={href}
            className="text-accent hover:underline underline-offset-2"
            {...(external ? { target: '_blank', rel: 'noreferrer' } : {})}
            {...(anchor
              ? {
                  onClick: (e: MouseEvent) => {
                    e.preventDefault()
                    document
                      .getElementById(decodeURIComponent(href.slice(1)))
                      ?.scrollIntoView({ behavior: 'smooth' })
                  },
                }
              : {})}
          >
            {renderInline(sub, key)}
          </a>
        )
      }
      case 'br':
        return <br key={key} />
      case 'escape':
        return <Fragment key={key}>{t.text as string}</Fragment>
      default:
        return <Fragment key={key}>{(t.text as string) ?? ''}</Fragment>
    }
  })
}

function renderTokens(tokens: Tok[] = [], keyPrefix = 'b'): ReactNode[] {
  return tokens.map((t, i) => {
    const key = `${keyPrefix}-${i}`
    const sub = (t.tokens as Tok[] | undefined) ?? []
    switch (t.type) {
      case 'heading': {
        const id = slugify(t.text as string)
        const inner = renderInline(sub, key)
        const depth = t.depth as number
        if (depth <= 1)
          return (
            <h1
              key={key}
              id={id}
              className="font-mono font-semibold text-ink text-[24px] @3xl:text-[28px] leading-tight mt-0 mb-4 scroll-mt-20"
            >
              {inner}
            </h1>
          )
        if (depth === 2)
          return (
            <h2
              key={key}
              id={id}
              className="font-mono font-medium text-ink text-[18px] leading-snug mt-10 mb-3 pt-5 border-t border-rule scroll-mt-20"
            >
              {inner}
            </h2>
          )
        if (depth === 3)
          return (
            <h3
              key={key}
              id={id}
              className="font-mono font-semibold text-ink text-[14px] mt-6 mb-2 scroll-mt-20"
            >
              {inner}
            </h3>
          )
        return (
          <h4
            key={key}
            id={id}
            className="font-mono font-semibold text-ink-faint text-[11px] uppercase tracking-[0.08em] mt-5 mb-2 scroll-mt-20"
          >
            {inner}
          </h4>
        )
      }
      case 'paragraph':
        return (
          <p
            key={key}
            className="font-mono text-[13px] @3xl:text-[14px] leading-[1.7] text-ink-faint my-3"
          >
            {renderInline(sub, key)}
          </p>
        )
      case 'text':
        return sub.length ? (
          <Fragment key={key}>{renderInline(sub, key)}</Fragment>
        ) : (
          <Fragment key={key}>{t.text as string}</Fragment>
        )
      case 'code': {
        const code = (t.text as string) ?? ''
        const lang = ((t.lang as string) ?? '').trim().toLowerCase().split(/\s+/)[0]
        if (lang === 'mermaid') return <Mermaid key={key} chart={code} />
        const hl = LANG_MAP[lang]
        return (
          <div key={key} className="my-4">
            <CodeBlock title={lang || undefined}>
              {hl ? <Highlight code={code} lang={hl} /> : code}
            </CodeBlock>
          </div>
        )
      }
      case 'blockquote':
        return (
          <blockquote
            key={key}
            className="border-l-2 border-rule pl-4 my-4 [&>p]:text-ink-faint [&>p]:my-2"
          >
            {renderTokens(sub, key)}
          </blockquote>
        )
      case 'list': {
        const ordered = Boolean(t.ordered)
        const items = (t.items as Tok[]) ?? []
        const className = cn(
          'my-3 pl-5 font-mono text-[13px] @3xl:text-[14px] leading-[1.7] text-ink-faint space-y-1.5 marker:text-ink-ghost',
          ordered ? 'list-decimal' : 'list-disc',
        )
        const lis = items.map((it, j) => (
          <li key={`${key}-${j}`} className="pl-1">
            {renderTokens((it.tokens as Tok[]) ?? [], `${key}-${j}`)}
          </li>
        ))
        return ordered ? (
          <ol key={key} className={className}>
            {lis}
          </ol>
        ) : (
          <ul key={key} className={className}>
            {lis}
          </ul>
        )
      }
      case 'table': {
        const header = (t.header as Tok[]) ?? []
        const rows = (t.rows as Tok[][]) ?? []
        return (
          <div key={key} className="my-4 overflow-x-auto">
            <table className="w-full border-collapse text-[13px] min-w-[480px]">
              <thead>
                <tr>
                  {header.map((cell, c) => (
                    <th
                      key={c}
                      className="border border-rule bg-panel px-3 py-2 text-left font-mono text-[10px] uppercase tracking-[0.06em] text-ink-faint"
                    >
                      {renderInline((cell.tokens as Tok[]) ?? [], `${key}-h-${c}`)}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {rows.map((row, r) => (
                  <tr key={r}>
                    {row.map((cell, c) => (
                      <td
                        key={c}
                        className="border border-rule px-3 py-2 align-top font-mono text-ink-faint leading-[1.6]"
                      >
                        {renderInline((cell.tokens as Tok[]) ?? [], `${key}-${r}-${c}`)}
                      </td>
                    ))}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )
      }
      case 'hr':
        return <hr key={key} className="my-8 border-0 border-t border-rule" />
      case 'space':
        return null
      case 'html':
        return (
          <div
            key={key}
            className="contents"
            // biome-ignore lint/security/noDangerouslySetInnerHtml: trusted local spec markdown
            dangerouslySetInnerHTML={{ __html: t.text as string }}
          />
        )
      default:
        return null
    }
  })
}

export function Markdown({ source }: { source: string }) {
  const tokens = marked.lexer(source) as unknown as Tok[]
  return <div className="min-w-0">{renderTokens(tokens)}</div>
}

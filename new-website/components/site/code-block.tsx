import type { BundledLanguage } from 'shiki'

import { CopyButton } from '@/components/animate-ui/components/buttons/copy'
import { highlight, tokenize } from '@/lib/highlight'
import { cn } from '@/lib/utils'
import { CodeLines } from './code-lines'

type CodeBlockProps = {
  code: string
  lang: BundledLanguage
  /** File name or label shown in the header bar. */
  title?: string
  /** Small glyph shown before the title, for example a terminal icon. */
  icon?: React.ReactNode
  className?: string
  /** Hide the header bar (for code nested inside tabs that already label it). */
  bare?: boolean
  /** Reveal the code line by line the first time it scrolls into view. */
  typed?: boolean
}

/** Server-rendered, Shiki-highlighted code with a copy button. */
export async function CodeBlock({ code, lang, title, icon, className, bare, typed }: CodeBlockProps) {
  const source = code.trim()
  const bodyClass =
    'overflow-x-auto p-3.5 font-mono text-[12.5px] leading-[1.7] sm:p-4 sm:text-[13px] [&_pre]:outline-none [scrollbar-width:thin]'
  return (
    <div className={cn('group/code relative overflow-hidden rounded-xl border bg-card', className)}>
      {bare ? null : (
        <div className={cn('flex h-11 items-center justify-between border-b pr-1', icon ? 'pl-2' : 'pl-4')}>
          <span className="flex items-center gap-2.5 font-mono text-muted-foreground text-xs">
            {icon}
            {title ?? lang}
          </span>
          <CopyButton
            content={source}
            variant="ghost"
            size="default"
            aria-label="Copy code"
            className="text-muted-foreground hover:text-foreground"
          />
        </div>
      )}
      {typed ? (
        <div className={bodyClass}>
          <CodeLines lines={await tokenize(source, lang)} />
        </div>
      ) : (
        <div
          className={bodyClass}
          // biome-ignore lint/security/noDangerouslySetInnerHtml: Shiki output is generated at build time from static strings.
          dangerouslySetInnerHTML={{ __html: await highlight(source, lang) }}
        />
      )}
    </div>
  )
}

import type { ReactNode } from "react"
import { type CodeLang, tokenize } from "@/lib/shiki"
import { cn } from "@/lib/utils"
import { CopyCodeButton } from "./copy-code-button"
import { TokenLines } from "./token-lines"

type CodeBlockProps = {
  code: string
  lang: CodeLang
  /** shown in the title bar, e.g. "orchestrator.ts" */
  title?: ReactNode
  /** small trailing slot in the title bar (a language tag, a status) */
  meta?: ReactNode
  showLineNumbers?: boolean
  copy?: boolean
  className?: string
}

/**
 * A static code window in the site's style: title bar, Shiki-highlighted body
 * (Min Light / Vesper), optional copy button. Server component: highlighting
 * happens at render time and ships as plain spans.
 */
export async function CodeBlock({ code, lang, title, meta, showLineNumbers, copy = true, className }: CodeBlockProps) {
  const lines = await tokenize(code.replace(/\n$/, ""), lang)
  return (
    <figure className={cn("overflow-hidden rounded-[14px] bg-gray-2 shadow-panel", className)}>
      {(title || meta || copy) && (
        <figcaption className="flex h-11 items-center gap-3 border-b border-gray-5 px-4">
          {title && <span className="truncate text-[13px] font-medium text-gray-12">{title}</span>}
          <span className="ml-auto flex items-center gap-2 text-[12px] text-gray-10">
            {meta}
            {copy && <CopyCodeButton code={code} />}
          </span>
        </figcaption>
      )}
      <TokenLines lines={lines} showLineNumbers={showLineNumbers} className="p-4" />
    </figure>
  )
}

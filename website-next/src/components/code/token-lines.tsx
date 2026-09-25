import type { CSSProperties, ReactNode } from "react"
import { keyed } from "@/lib/keys"
import type { CodeLine, CodeToken } from "@/lib/shiki"
import { cn } from "@/lib/utils"

/** Inline style carrying both theme colors; `.code-tokens` in globals.css picks one. */
function tokenStyle(t: CodeToken): CSSProperties {
  const s: Record<string, string> = {}
  if (t.light) s["--shiki-light"] = t.light
  if (t.dark) s["--shiki-dark"] = t.dark
  if (t.fontStyle && t.fontStyle & 1) s.fontStyle = "italic"
  if (t.fontStyle && t.fontStyle & 2) s.fontWeight = "600"
  if (t.fontStyle && t.fontStyle & 4) s.textDecoration = "underline"
  return s as CSSProperties
}

export function Token({ token }: { token: CodeToken }) {
  return <span style={tokenStyle(token)}>{token.text}</span>
}

type TokenLinesProps = {
  lines: CodeLine[]
  /** decorate a line (highlight, dim, append a result comment) */
  renderLine?: (line: ReactNode, index: number) => ReactNode
  showLineNumbers?: boolean
  className?: string
}

/**
 * Pre-tokenized code as JSX. Works in client components (the tokens are plain
 * data), so sections can type it out, highlight the executing line, or reveal
 * lines over time without touching Shiki on the client.
 */
export function TokenLines({ lines, renderLine, showLineNumbers, className }: TokenLinesProps) {
  return (
    <pre
      className={cn("code-tokens m-0 overflow-x-auto font-mono text-[14px] leading-[1.8] tracking-[0.01em]", className)}
    >
      <code>
        {keyed(lines, (line) => line.map((t) => t.text).join("")).map(({ key, item: line }, i) => {
          const content = (
            <>
              {showLineNumbers && (
                <span aria-hidden="true" className="mr-4 inline-block w-6 text-right text-gray-8 select-none">
                  {i + 1}
                </span>
              )}
              {line.length ? keyed(line, (t) => t.text).map((t) => <Token key={t.key} token={t.item} />) : "\n"}
            </>
          )
          return (
            <span key={key} className="block">
              {renderLine ? renderLine(content, i) : content}
            </span>
          )
        })}
      </code>
    </pre>
  )
}

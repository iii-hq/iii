import "server-only"
import { createHighlighter, type ThemedToken } from "shiki"

// One highlighter for the whole server, with exactly the grammars the site
// shows. Dual themes: Min Light in light mode, Vesper in dark. Tokens carry
// both colors as CSS variables, and globals.css picks one per theme, so a
// theme flip recolors code with no re-render.
export const CODE_THEMES = { light: "min-light", dark: "vesper" } as const
export const CODE_LANGS = ["typescript", "tsx", "python", "rust", "bash", "json", "yaml", "toml"] as const
export type CodeLang = (typeof CODE_LANGS)[number]

let highlighter: ReturnType<typeof createHighlighter> | undefined
function getHighlighter() {
  highlighter ??= createHighlighter({ themes: Object.values(CODE_THEMES), langs: [...CODE_LANGS] })
  return highlighter
}

/** A token as a plain object, safe to pass into client components. */
export type CodeToken = { text: string; light?: string; dark?: string; fontStyle?: number }
export type CodeLine = CodeToken[]

function serialize(token: ThemedToken): CodeToken {
  const vars = (token.htmlStyle ?? {}) as Record<string, string>
  return {
    text: token.content,
    light: vars["--shiki-light"],
    dark: vars["--shiki-dark"],
    fontStyle: token.fontStyle || undefined,
  }
}

/** Tokenize code into lines for client-side rendering (typing, line highlights, reveals). */
export async function tokenize(code: string, lang: CodeLang): Promise<CodeLine[]> {
  const hl = await getHighlighter()
  const { tokens } = hl.codeToTokens(code, { lang, themes: CODE_THEMES, defaultColor: false })
  return tokens.map((line) => line.map(serialize))
}

/** Static highlighted HTML (a `<pre class="shiki">…`), for code that never changes. */
export async function highlight(code: string, lang: CodeLang): Promise<string> {
  const hl = await getHighlighter()
  return hl.codeToHtml(code, { lang, themes: CODE_THEMES, defaultColor: false })
}

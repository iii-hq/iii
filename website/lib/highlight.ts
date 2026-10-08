import 'server-only'

import { type BundledLanguage, codeToHtml, codeToTokens } from 'shiki'

/**
 * One theme pair for every code sample on the site: Vitesse Light on the light theme, Vesper on
 * the dark theme. Both are muted enough to sit inside the monochrome UI without competing with
 * the green accent used by the graphics. Swap the values here to retheme every code block at once.
 */
export const codeThemes = { light: 'vitesse-light', dark: 'vesper' } as const

export type CodeToken = { text: string; style?: Record<string, string> }
export type CodeLine = CodeToken[]

/** Full Shiki HTML (`<pre class="shiki">…`) for server-rendered code blocks. */
export async function highlight(code: string, lang: BundledLanguage) {
  return codeToHtml(code.trim(), { lang, themes: codeThemes, defaultColor: 'light' })
}

/**
 * Per-line tokens carrying their light and dark colors as inline styles. Client components that
 * animate code line by line render these, so the same theme applies everywhere.
 */
export async function tokenize(code: string, lang: BundledLanguage): Promise<CodeLine[]> {
  const { tokens } = await codeToTokens(code, { lang, themes: codeThemes, defaultColor: 'light' })
  return tokens.map((line) => line.map((token) => ({ text: token.content, style: token.htmlStyle })))
}

import type { CodeLine, CodeToken } from "@/lib/shiki"

/**
 * The first `count` characters of pre-tokenized lines, so a client can "type"
 * Shiki-highlighted code without touching Shiki: tokens are cut mid-word and
 * later lines drop away entirely until the typing reaches them. Newlines
 * between lines count as one character, matching the source string.
 */
export function sliceTokenLines(lines: CodeLine[], count: number): CodeLine[] {
  const out: CodeLine[] = []
  let left = count
  for (const line of lines) {
    if (left <= 0) break
    const cut: CodeToken[] = []
    for (const token of line) {
      if (left <= 0) break
      if (token.text.length <= left) {
        cut.push(token)
        left -= token.text.length
      } else {
        cut.push({ ...token, text: token.text.slice(0, left) })
        left = 0
      }
    }
    out.push(cut)
    left -= 1 // the "\n"
  }
  return out
}

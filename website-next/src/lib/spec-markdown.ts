import "server-only"

import type { SpecDoc } from "./specs"

// Spec markdown is written to stand alone on GitHub: every file opens with its
// own `# Title`. On the spec page the frontmatter title is the page's h1 and
// each extra doc gets an h2 of its own, so the files' headings move down to
// fit one outline.

/** Split markdown into fenced-code and prose chunks so heading rewrites never touch code. */
function mapProse(md: string, fn: (prose: string) => string) {
  return md
    .split(/(^(?:```|~~~)[\s\S]*?^(?:```|~~~)[ \t]*$)/m)
    .map((chunk, i) => (i % 2 === 1 ? chunk : fn(chunk)))
    .join("")
}

/** Drop the first `# Heading` (the file's title). */
export function stripTitle(md: string) {
  let done = false
  return mapProse(md, (prose) => {
    if (done) return prose
    return prose.replace(/^#\s+.+\n?/m, () => {
      done = true
      return ""
    })
  })
}

/** `## x` → `### x` (and so on) for a doc rendered beneath its own h2. h6 stays h6. */
export function demoteHeadings(md: string) {
  return mapProse(md, (prose) => prose.replace(/^(#{2,5})(?=\s)/gm, "#$1"))
}

/** The README body for the spec page: frontmatter already stripped, first h1 removed. */
export function readmeBody(doc: SpecDoc) {
  return stripTitle(doc.markdown)
}

/** An extra doc's body: title removed, every heading one level down. */
export function docBody(doc: SpecDoc) {
  return demoteHeadings(stripTitle(doc.markdown))
}

/** A doc title as plain text: `# Configuration — \`codegen.yml\`` loses its backticks and emphasis. */
export function plainTitle(title: string) {
  return title.replace(/`([^`]*)`/g, "$1").replace(/[*_]{1,2}([^*_]+)[*_]{1,2}/g, "$1")
}

/** `emitters.md` → `emitters`, the id prefix that keeps a doc's anchors unique on the page. */
export function docId(doc: SpecDoc) {
  return doc.file
    .replace(/\.md$/, "")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
}

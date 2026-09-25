import "server-only"

import rehypeShiki from "@shikijs/rehype"
import { toJsxRuntime } from "hast-util-to-jsx-runtime"
import type { ReactNode } from "react"
import { Fragment, jsx, jsxs } from "react/jsx-runtime"
import rehypeRaw from "rehype-raw"
import rehypeSlug from "rehype-slug"
import remarkGfm from "remark-gfm"
import remarkParse from "remark-parse"
import remarkRehype from "remark-rehype"
import type { BundledLanguage } from "shiki"
import { unified } from "unified"
import { CODE_THEMES } from "./shiki"

// The hast node types, taken from `toJsxRuntime`'s signature: `@types/hast` is
// only a transitive dependency here, so it is not importable by name.
type Nodes = Parameters<typeof toJsxRuntime>[0]
type Root = Extract<Nodes, { type: "root" }>
type Element = Extract<Nodes, { type: "element" }>
type Parent = Root | Element

export type Heading = { id: string; text: string; depth: 2 | 3 }

export type Rendered = {
  /** the article body as React nodes, for the children of `<Prose>` */
  content: ReactNode
  /** h2/h3 headings in document order, for a table of contents */
  headings: Heading[]
}

type Options = {
  /** rewrite an image src (e.g. a path relative to the source file) to a public URL */
  image?: (src: string) => string
  /**
   * Prefix for every `id` and same-page `#` link. Several documents rendered on
   * one page can each hand rehype-slug the same heading ("architecture"); the
   * prefix keeps their anchors distinct.
   */
  idPrefix?: string
}

/** Languages Shiki loads up front. Anything else falls back to plain text, never to an error. */
const LANGS: BundledLanguage[] = [
  "typescript",
  "tsx",
  "javascript",
  "json",
  "jsonc",
  "yaml",
  "toml",
  "bash",
  "shell",
  "rust",
  "python",
  "html",
  "css",
  "sql",
  "dockerfile",
  "mermaid",
]

const processor = unified()
  .use(remarkParse)
  .use(remarkGfm)
  .use(remarkRehype, { allowDangerousHtml: true })
  .use(rehypeRaw)
  .use(rehypeSlug)
  .use(rehypeShiki, {
    // Both themes in one pass. `.shiki span` in globals.css picks the variable for the active theme.
    themes: CODE_THEMES,
    defaultColor: false,
    langs: LANGS,
    fallbackLanguage: "text" as BundledLanguage,
  })

/**
 * Markdown → React for the long-form pages (blog posts, tech specs). GitHub
 * flavoured, inline HTML allowed, heading ids for deep links, code through
 * Shiki with the same Vesper / Min Light pair the landing page uses. The hast
 * tree is turned into elements directly, so no HTML string is ever injected.
 */
export async function renderMarkdown(markdown: string, options: Options = {}): Promise<Rendered> {
  const source = options.image
    ? markdown.replace(
        /!\[([^\]]*)\]\(([^)\s]+)([^)]*)\)/g,
        (_m, alt, src, rest) => `![${alt}](${options.image?.(src) ?? src}${rest})`,
      )
    : markdown
  const tree = (await processor.run(processor.parse(source))) as Root
  wrapTables(tree)
  lazyImages(tree)
  if (options.idPrefix) prefixIds(tree, options.idPrefix)
  const content = toJsxRuntime(tree, { Fragment, jsx, jsxs, development: false })
  return { content, headings: extractHeadings(tree) }
}

/** Depth-first walk over every element in the tree, parents before children. */
function visit(parent: Parent, fn: (node: Element, index: number, parent: Parent) => void) {
  for (let i = 0; i < parent.children.length; i++) {
    const child = parent.children[i]
    if (child.type !== "element") continue
    fn(child, i, parent)
    visit(child, fn)
  }
}

/** Tables get a scroll box: a wide table must never widen the page on a phone. */
function wrapTables(tree: Root) {
  visit(tree, (node, index, parent) => {
    if (node.tagName !== "table") return
    parent.children[index] = {
      type: "element",
      tagName: "div",
      properties: { className: ["table-scroll"] },
      children: [node],
    }
  })
}

/**
 * Article images load lazily. They sit below the fold in long posts, and React
 * only emits preload hints for eager images, so this also keeps a post's RSC
 * payload from preloading every picture when the index prefetches it.
 */
function lazyImages(tree: Root) {
  visit(tree, (node) => {
    if (node.tagName !== "img") return
    node.properties.loading ??= "lazy"
    node.properties.decoding ??= "async"
  })
}

/** `id="x"` → `id="<prefix>-x"`, and `href="#x"` → `href="#<prefix>-x"` to match. */
function prefixIds(tree: Root, prefix: string) {
  visit(tree, (node) => {
    const { id, href } = node.properties
    if (typeof id === "string" && id) node.properties.id = `${prefix}-${id}`
    if (typeof href === "string" && href.startsWith("#") && href.length > 1) {
      node.properties.href = `#${prefix}-${href.slice(1)}`
    }
  })
}

/** h2/h3 with ids, in document order; tags inside the heading are dropped for the label. */
function extractHeadings(tree: Root): Heading[] {
  const out: Heading[] = []
  visit(tree, (node) => {
    if (node.tagName !== "h2" && node.tagName !== "h3") return
    const id = node.properties.id
    if (typeof id !== "string" || !id) return
    const text = toText(node).trim()
    if (text) out.push({ depth: node.tagName === "h2" ? 2 : 3, id, text })
  })
  return out
}

/** The text content of a node, like `textContent`. */
function toText(node: Nodes): string {
  if (node.type === "text") return node.value
  if (node.type === "root" || node.type === "element") return node.children.map(toText).join("")
  return ""
}

/** Reading time from the raw markdown, rounded up, at 220 words a minute. */
export function readingTime(markdown: string) {
  const words = markdown
    .replace(/```[\s\S]*?```/g, "")
    .split(/\s+/)
    .filter(Boolean).length
  return Math.max(1, Math.ceil(words / 220))
}

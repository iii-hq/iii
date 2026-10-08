import fs from "node:fs/promises"
import path from "node:path"
import { fileURLToPath, pathToFileURL } from "node:url"
import { type HTMLElement, type Node, parse } from "node-html-parser"
import { AI_OVERVIEW } from "./ai-overview"
import { buildBlogLinksSection } from "./generate-blog-md"
import { SITE_ORIGIN } from "./routes"

const WEBSITE_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..")
// The homepage copy is scraped from the BUILT page, so this script runs after
// `next build` (see the package build script) and emits straight into dist/.
const INDEX_PATH = path.join(WEBSITE_ROOT, "dist", "index.html")
const LLMS_PATH = path.join(WEBSITE_ROOT, "dist", "llms.txt")
const AGENTS_PATH = path.join(WEBSITE_ROOT, "dist", "AGENTS.md")
const AGENTS_APPENDIX_PATH = path.join(WEBSITE_ROOT, "scripts", "agents-appendix.md")

/** llms.txt-style blockquote (one-line summary for crawlers). */
const LLMS_TAGLINE =
  "iii turns distributed backend complexity into a simple set of real-time, interoperable primitives called Functions, Triggers, and Workers. The result is coordinated execution that behaves as if it were a single runtime."

function isoDate(): string {
  return new Date().toISOString().slice(0, 10)
}

/** Non-empty optional section plus trailing blank line; empty input adds nothing. */
function optionalSection(section: string): string[] {
  const trimmed = section.trimEnd()
  if (!trimmed) return []
  return [trimmed, ""]
}

/** Drop the leading H1 so `llms.txt` keeps a single project `# iii` title per llms.txt guidance. */
export function overviewBodyWithoutLeadingH1(): string {
  return AI_OVERVIEW.replace(/^#\s+[^\n]*\n+/, "").trimStart()
}

function collapseWhitespace(s: string): string {
  return s.replace(/\s+/g, " ").trim()
}

// ---------------------------------------------------------------------------
// Homepage extraction. Every landing section is a `<section aria-labelledby>`
// whose head is an eyebrow <p>, the section's <h2> and an optional lede, all
// children of one element (the Section component's <header>). The hero is the
// one <h1> and the paragraph under it; the FAQ comes from the page's FAQPage
// JSON-LD (the answers live in collapsed panels). Everything else on the page
// is animation, code and controls, which is why the selectors stay this narrow.
// ---------------------------------------------------------------------------

const SR_ONLY = ".sr-only"

/** Sections read by their own rules rather than the generic section head. */
const SPECIAL_SECTIONS = new Set(["hero", "faq", "final-cta"])

function isElement(node: Node): node is HTMLElement {
  return node.nodeType === 1
}

/**
 * Visible text of an element. Lines set as stacked blocks (`<span class="block">More application.</span><span
 * class="block">Less infrastructure.</span>`) get the space the layout implied; screen-reader-only spans (e.g.
 * "(opens in a new tab)") are dropped.
 */
function textOf(el: HTMLElement): string {
  const copy = parse(el.outerHTML).firstChild as HTMLElement
  for (const n of copy.querySelectorAll(SR_ONLY)) n.remove()
  return collapseWhitespace(copy.text).replace(/([.!?])(?=[A-Z][a-z])/g, "$1 ")
}

function must<T>(value: T | null | undefined, what: string): T {
  if (value == null) throw new Error(`generate-llms-agents: ${what} not found in dist/index.html`)
  return value
}

/** Inside an element hidden from assistive tech (the graphics), so not copy. */
function isDecorative(el: HTMLElement): boolean {
  for (let n: HTMLElement | null = el; n; n = n.parentNode as HTMLElement | null) {
    if (n.getAttribute?.("aria-hidden") === "true") return true
  }
  return false
}

/** The section's eyebrow, heading and lede: the heading's siblings in the section head. */
function sectionHead(section: HTMLElement): { eyebrow?: string; title: string; lede?: string } {
  const id = section.getAttribute("id") ?? "section"
  const heading = must(section.querySelector("h1, h2"), `#${id} heading`)
  const head = heading.parentNode as HTMLElement
  const children = head.childNodes.filter(isElement)
  // The heading may be wrapped (a Reveal); find the child of the head that holds it.
  const index = children.findIndex((c) => c === heading || c.querySelector("h1, h2") === heading)
  const eyebrow = children
    .slice(0, index)
    .map(textOf)
    .find((t) => t)
  const lede = children
    .slice(index + 1)
    .filter((c) => !c.querySelector("form, button, input") && !isDecorative(c))
    .map(textOf)
    .filter(Boolean)
    .join(" ")
  return { eyebrow: eyebrow || undefined, title: textOf(heading), lede: lede || undefined }
}

function absoluteUrl(href: string): string {
  return href.startsWith("/") ? `${SITE_ORIGIN}${href}` : href
}

function heroLines(root: HTMLElement): string[] {
  const hero = must(root.querySelector("#hero"), "#hero")
  const h1 = must(hero.querySelector("h1"), "#hero h1")
  // The first visible paragraph is the hero copy; the install card's notes and the email row come after it.
  const lead = hero.querySelectorAll("p").find((p) => !isDecorative(p) && textOf(p))
  const lines = [`**${textOf(h1)}**`]
  if (lead) lines.push(textOf(lead))
  return lines
}

/** One block per landing section between the hero and the FAQ, in page order. */
function sectionChunks(root: HTMLElement): string[] {
  const main = must(root.querySelector("main"), "<main>")
  const chunks: string[] = []
  for (const section of main.querySelectorAll("section[aria-labelledby]")) {
    const id = section.getAttribute("id")
    if (!id || SPECIAL_SECTIONS.has(id) || section.parentNode !== main) continue
    const { eyebrow, title, lede } = sectionHead(section)
    chunks.push(`### ${eyebrow ?? title}`, `**${title}**`)
    if (lede) chunks.push(lede)
    chunks.push("")
  }
  return chunks
}

/** The FAQ as question / answer pairs, from the FAQPage JSON-LD the page ships. */
function faqLines(root: HTMLElement): string[] {
  for (const script of root.querySelectorAll('script[type="application/ld+json"]')) {
    let data: { "@type"?: string; mainEntity?: { name: string; acceptedAnswer: { text: string } }[] }
    try {
      data = JSON.parse(script.text)
    } catch {
      continue
    }
    if (data["@type"] !== "FAQPage" || !data.mainEntity) continue
    return data.mainEntity.flatMap((q) => [`**${q.name}**`, collapseWhitespace(q.acceptedAnswer.text), ""])
  }
  throw new Error("generate-llms-agents: FAQPage JSON-LD not found in dist/index.html")
}

/** The site footer: what iii is, the link columns, the small print. */
function footerLines(root: HTMLElement): string[] {
  const footer = must(root.querySelector("footer"), "<footer>")
  const lines: string[] = []
  const paragraphs = footer.querySelectorAll("p").map(textOf).filter(Boolean)
  const about = paragraphs.filter((p) => !p.startsWith("©"))
  if (about.length) lines.push(about.join(" "))
  for (const column of footer.querySelectorAll("nav h3")) {
    const title = textOf(column)
    // The assistant links carry a long prefilled prompt; they are actions, not pages.
    if (title.startsWith("Ask")) continue
    const list = (column.parentNode as HTMLElement).querySelector("ul")
    const links = (list?.querySelectorAll("a") ?? []).map((a) => {
      const href = a.getAttribute("href")
      const label = textOf(a)
      return href ? `[${label}](${absoluteUrl(href)})` : label
    })
    if (links.length) lines.push(`- ${title}: ${links.join(", ")}`)
  }
  const copyright = paragraphs.find((p) => p.startsWith("©"))
  if (copyright) lines.push(copyright.replace(/\s*·.*$/, ""))
  return lines
}

/** Plain-text extraction of homepage marketing copy (shared by llms.txt and AGENTS.md). */
export function buildHomepageExtractFromHtml(html: string): string {
  const root = parse(html)
  const chunks: string[] = ["## Homepage copy (extracted from iii.dev HTML)", ""]

  chunks.push("### Hero", ...heroLines(root), "")
  chunks.push(...sectionChunks(root))
  chunks.push("### Questions", ...faqLines(root))

  const cta = root.querySelector("#final-cta")
  if (cta) {
    const { title, lede } = sectionHead(cta)
    chunks.push("### Get started", `**${title}**`, ...(lede ? [lede] : []), "")
  }

  chunks.push("### Footer / links", ...footerLines(root), "")

  return `${chunks.join("\n").trimEnd()}\n`
}

/**
 * llms.txt: H1, blockquote summary, prose, homepage extract, then H2 sections with annotated links.
 */
export function buildLlmsTxt(html: string, blogSection = ""): string {
  const overview = overviewBodyWithoutLeadingH1()
  const home = buildHomepageExtractFromHtml(html)
  const tail = `
## Core pages

- [Homepage](https://iii.dev/) — positioning and visuals
- [Manifesto](https://iii.dev/manifesto) — paradigm argument
- [Documentation](https://iii.dev/docs) — full documentation
- [Blog index (markdown)](https://iii.dev/blog/index.md) — architecture posts for coding agents
- [llms.txt](https://iii.dev/llms.txt) — this file (AI / LLM discovery)
- [AGENTS.md](https://iii.dev/AGENTS.md) — build path: install, wire-level notes, and guardrails for coding agents
- [GitHub](https://github.com/iii-hq/iii) — engine, TypeScript/Python/Rust SDKs

## Optional

- [Worker registry](https://workers.iii.dev) — published workers

## Want to build on iii?

This file is for understanding iii. To install the engine and ship your first Worker, read **[AGENTS.md](https://iii.dev/AGENTS.md)** and the **[install guide](https://iii.dev/docs/install)**.

Last updated: ${isoDate()}
`.trimStart()

  const body = [
    "# iii",
    "",
    `> ${LLMS_TAGLINE}`,
    "",
    overview.trimEnd(),
    "",
    home.trimEnd(),
    "",
    ...optionalSection(blogSection),
    tail.trimEnd(),
    "",
  ].join("\n")

  return `${body.trimEnd()}\n`
}

/**
 * AGENTS.md: [agents.md](https://agents.md/) product context + same pre-written overview + homepage extract + wire-level appendix.
 */
export function buildAgentsMd(html: string, agentsAppendix: string, blogSection = ""): string {
  const overview = overviewBodyWithoutLeadingH1()
  const home = buildHomepageExtractFromHtml(html)
  const intro = [
    "# iii for AI Agents",
    "",
    "This file is public **[AGENTS.md](https://agents.md/)**-style guidance for **[iii](https://iii.dev/)** (the product): positioning, comparisons, scraped homepage copy, and wire-level notes for autonomous agents.",
    "",
    "## Overview and comparisons (pre-written)",
    "",
    overview.trimEnd(),
    "",
    home.trimEnd(),
    "",
    ...optionalSection(blogSection),
    agentsAppendix.trimEnd(),
    "",
    `Last updated: ${isoDate()}`,
    "",
  ].join("\n")

  return intro
}

async function main() {
  const html = await fs.readFile(INDEX_PATH, "utf8").catch(() => {
    throw new Error(`generate-llms-agents: ${INDEX_PATH} missing — run \`next build\` first`)
  })
  const [appendix, blogSection] = await Promise.all([
    fs.readFile(AGENTS_APPENDIX_PATH, "utf8"),
    buildBlogLinksSection(),
  ])
  const llms = buildLlmsTxt(html, blogSection)
  const agents = buildAgentsMd(html, appendix, blogSection)
  await Promise.all([fs.writeFile(LLMS_PATH, llms, "utf8"), fs.writeFile(AGENTS_PATH, agents, "utf8")])
  console.log(
    `wrote ${path.relative(WEBSITE_ROOT, LLMS_PATH)} (${llms.length} b), ${path.relative(WEBSITE_ROOT, AGENTS_PATH)} (${agents.length} b)`,
  )
}

const isMain = import.meta.url === pathToFileURL(path.resolve(process.argv[1] ?? "")).href
if (isMain) {
  main().catch((err) => {
    console.error(err)
    process.exitCode = 1
  })
}

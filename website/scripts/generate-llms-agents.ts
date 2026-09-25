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
// Homepage extraction. Landing sections are `<section id=...>`; each opens with
// a SectionIntro (`[data-llms="intro"]`: eyebrow <p>, <h2>, one <p>) and the
// hero's copy column is `[data-llms="hero"]`. Everything else on the page is
// animation, code and controls, which is why the selectors stay this narrow.
// ---------------------------------------------------------------------------

const SR_ONLY = ".sr-only"

function isElement(node: Node): node is HTMLElement {
  return node.nodeType === 1
}

/**
 * Visible text of an element. Direct children are joined with a space so
 * `<h1><span class="block">A.</span><span class="block">B.</span></h1>` reads
 * "A. B."; screen-reader-only spans (e.g. "(opens in a new tab)") are dropped.
 */
function textOf(el: HTMLElement): string {
  const copy = parse(el.outerHTML).firstChild as HTMLElement
  for (const n of copy.querySelectorAll(SR_ONLY)) n.remove()
  return collapseWhitespace(copy.childNodes.map((n) => n.text).join(" "))
}

function must<T>(value: T | null | undefined, what: string): T {
  if (value == null) throw new Error(`generate-llms-agents: ${what} not found in dist/index.html`)
  return value
}

/** The section's heading and one-paragraph intro, as two lines of prose. */
function sectionIntro(root: HTMLElement, sectionId: string): string[] {
  const section = must(root.querySelector(`#${sectionId}`), `#${sectionId}`)
  const intro = must(section.querySelector('[data-llms="intro"]'), `#${sectionId} [data-llms="intro"]`)
  const title = must(intro.querySelector("h2"), `#${sectionId} intro h2`)
  const children = intro.childNodes.filter(isElement)
  const titleIndex = children.indexOf(title)
  const body = children.slice(titleIndex + 1).find((c) => c.tagName === "P")
  const lines = [`**${textOf(title)}**`]
  if (body) lines.push(textOf(body))
  return lines
}

function absoluteUrl(href: string): string {
  return href.startsWith("/") ? `${SITE_ORIGIN}${href}` : href
}

function heroLines(root: HTMLElement): string[] {
  const hero = must(root.querySelector('#hero [data-llms="hero"]'), '#hero [data-llms="hero"]')
  const h1 = must(hero.querySelector("h1"), "#hero h1")
  // Only the copy column's own paragraphs: the actions block below them holds buttons and a form.
  const lead = hero.childNodes.filter(isElement).find((c) => c.tagName === "P")
  const lines = [`**${textOf(h1)}**`]
  if (lead) lines.push(textOf(lead))
  return lines
}

/** The `#workers` "view the registry" link, as a markdown link line. */
function workersCtaLine(root: HTMLElement): string | null {
  const link = root.querySelector("#workers .nw-cta-row a")
  if (!link) return null
  const href = link.getAttribute("href")
  const label = textOf(link)
  return href ? `[${label}](${absoluteUrl(href)})` : label
}

/** One bullet per worker card in `#hello` (`<article aria-label="Node.js worker, orchestrator">`), deduped. */
function helloWorkerLines(root: HTMLElement): string[] {
  const labels = new Set<string>()
  for (const card of root.querySelectorAll("#hello article[aria-label]")) {
    const label = collapseWhitespace(card.getAttribute("aria-label") ?? "")
    if (label) labels.add(label)
  }
  return [...labels].map((l) => `- ${l}`)
}

/** `#nutshell`: each trait group (h3 + tagline) followed by its cards (h4: p). */
function nutshellLines(root: HTMLElement): string[] {
  const section = must(root.querySelector("#nutshell"), "#nutshell")
  const lines: string[] = []
  for (const group of section.querySelectorAll("h3")) {
    const tagline = group.parentNode.childNodes.filter(isElement).find((c) => c.tagName === "P")
    lines.push("", `**${textOf(group)}**${tagline ? ` ${textOf(tagline)}` : ""}`)
    // The cards are the <ul> that follows the group's heading block.
    const block = group.parentNode
    const siblings = block.parentNode.childNodes.filter(isElement)
    const list = siblings.slice(siblings.indexOf(block) + 1).find((c) => c.tagName === "UL")
    for (const li of list?.querySelectorAll("li") ?? []) {
      const t = li.querySelector("h4")
      const p = li.querySelector("p")
      if (t && p) lines.push(`- ${textOf(t)}: ${textOf(p)}`)
    }
  }
  return lines
}

/** The site footer: what iii is, the link columns, the small print. */
function footerLines(root: HTMLElement): string[] {
  const footer = must(root.querySelector("footer"), "<footer>")
  const lines: string[] = []
  // The brand column and the copyright are the footer's only paragraphs; the link columns are <nav>s.
  const paragraphs = footer.querySelectorAll("p").map(textOf).filter(Boolean)
  const about = paragraphs.filter((p) => !p.startsWith("©"))
  if (about.length) lines.push(about.join(" "))
  for (const nav of footer.querySelectorAll("nav")) {
    const title = nav.querySelector("h2")
    const links = nav.querySelectorAll("a").map((a) => {
      const href = a.getAttribute("href")
      const label = textOf(a)
      return href ? `[${label}](${absoluteUrl(href)})` : label
    })
    if (!links.length) continue
    lines.push(`- ${title ? `${textOf(title)}: ` : ""}${links.join(", ")}`)
  }
  const copyright = paragraphs.find((p) => p.startsWith("©"))
  if (copyright) lines.push(copyright)
  return lines
}

/** Plain-text extraction of homepage marketing copy (shared by llms.txt and AGENTS.md). */
export function buildHomepageExtractFromHtml(html: string): string {
  const root = parse(html)
  const chunks: string[] = ["## Homepage copy (extracted from iii.dev HTML)", ""]

  chunks.push("### Hero", ...heroLines(root), "")
  chunks.push("### Experience", ...sectionIntro(root, "experience"), "")

  chunks.push("### Workers", ...sectionIntro(root, "workers"))
  const cta = workersCtaLine(root)
  if (cta) chunks.push(cta)
  chunks.push("")

  chunks.push("### Languages / protocol", ...sectionIntro(root, "hello"), ...helloWorkerLines(root), "")
  chunks.push("### Agents / console", ...sectionIntro(root, "console-live"), "")
  chunks.push("### Harness race", ...sectionIntro(root, "harness"), "")
  chunks.push("### iii in a nutshell", ...sectionIntro(root, "nutshell"), ...nutshellLines(root), "")

  // The roadmap preview renders only when the spec feed was reachable at build time.
  if (root.querySelector('#tech-specs [data-llms="intro"]')) {
    chunks.push("### Roadmap", ...sectionIntro(root, "tech-specs"), "")
  }

  chunks.push("### Get started", ...sectionIntro(root, "footer"), "")
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

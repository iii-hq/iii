import fs from 'node:fs/promises'
import path from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { type HTMLElement, parse } from 'node-html-parser'
import { AI_OVERVIEW } from './ai-overview'

const WEBSITE_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const INDEX_PATH = path.join(WEBSITE_ROOT, 'index.html')
const LLMS_PATH = path.join(WEBSITE_ROOT, 'llms.txt')
const AGENTS_PATH = path.join(WEBSITE_ROOT, 'AGENTS.md')
const AGENTS_APPENDIX_PATH = path.join(WEBSITE_ROOT, 'scripts', 'agents-appendix.md')

/** llms.txt-style blockquote (one-line summary for crawlers). */
const LLMS_TAGLINE =
  'iii turns distributed backend complexity into a simple set of real-time, interoperable primitives called Functions, Triggers, and Workers. The result is coordinated execution that behaves as if it were a single runtime.'

function isoDate(): string {
  return new Date().toISOString().slice(0, 10)
}

/** Drop the leading H1 so `llms.txt` keeps a single project `# iii` title per llms.txt guidance. */
export function overviewBodyWithoutLeadingH1(): string {
  return AI_OVERVIEW.replace(/^#\s+[^\n]*\n+/, '').trimStart()
}

function collapseWhitespace(s: string): string {
  return s.replace(/\s+/g, ' ').trim()
}

/** Subtrees removed before `.text` so buttons, forms, tabs, and code samples do not pollute output. */
const EXCLUDE_FROM_COPY_SELECTORS = [
  'button',
  'input',
  'textarea',
  'select',
  'form',
  'pre.hello-code',
  '[role="tablist"]',
]

function textFromSanitized(root: HTMLElement, selector: string, label: string): string {
  const el = root.querySelector(selector)
  if (!el) throw new Error(`generate-llms-agents: missing selector "${selector}" (${label})`)
  const frag = parse(el.outerHTML)
  const copy = frag.firstChild as HTMLElement | null
  if (!copy) throw new Error(`generate-llms-agents: empty fragment for "${selector}" (${label})`)
  for (const sel of EXCLUDE_FROM_COPY_SELECTORS) {
    copy.querySelectorAll(sel).forEach((n) => {
      n.remove()
    })
  }
  return collapseWhitespace(copy.text)
}

function textFrom(root: HTMLElement, selector: string, label: string): string {
  const el = root.querySelector(selector)
  if (!el) throw new Error(`generate-llms-agents: missing selector "${selector}" (${label})`)
  return collapseWhitespace(el.text)
}

function textsFrom(root: HTMLElement, selector: string): string[] {
  return root.querySelectorAll(selector).map((el) => collapseWhitespace(el.text))
}

/** Plain-text extraction of homepage marketing copy (shared by llms.txt and AGENTS.md). */
export function buildHomepageExtractFromHtml(html: string): string {
  const root = parse(html)
  const chunks: string[] = ['## Homepage copy (extracted from iii.dev HTML)', '']

  // `.hero-left` only: excludes hero CTA (button, form), viz, and tab buttons in aside.
  chunks.push('### Hero', textFrom(root, '.hero-left', 'hero-left'), '')
  chunks.push('### Experience', textFrom(root, '#experience .xp-head', 'experience head'), '')
  chunks.push('### Workers', textFrom(root, '#workers .nut-workers-head', 'workers head'), '')

  const cta = root.querySelector('#workers .nw-cta-row')
  if (cta) chunks.push(textFromSanitized(root, '#workers .nw-cta-row', 'workers cta'), '')

  chunks.push('### Languages / protocol', textFrom(root, '#hello .hello-head', 'hello head'), '')

  for (const card of root.querySelectorAll('#hello .hello-card')) {
    const title = card.querySelector('.hello-meta .t')
    const subtitle = card.querySelector('.hello-meta .s')
    const rawHead = [title?.text, subtitle?.text].filter((x): x is string => Boolean(x))
    const headBits = rawHead.map((x) => collapseWhitespace(x))
    if (headBits.length) chunks.push(headBits.join(' — '))
  }

  chunks.push('### Agents', textFrom(root, '#agents .agent-head', 'agents head'), '')

  chunks.push('### Console / observability', textFrom(root, '#cs-scroll .cs-section-head', 'console section head'))
  for (const cap of textsFrom(root, '#cs-scroll p.cs-cap')) {
    chunks.push(`- ${cap}`)
  }
  chunks.push('')

  chunks.push('### iii in a nutshell', textFrom(root, '#nutshell .nut-head', 'nutshell head'))
  for (const cell of root.querySelectorAll('#nutshell .nut-cell')) {
    const t = cell.querySelector('.nut-pt-title')
    const p = cell.querySelector('.nut-pt-body')
    if (t && p) chunks.push(`- ${collapseWhitespace(t.text)}: ${collapseWhitespace(p.text)}`)
  }
  chunks.push('')

  chunks.push(
    '### Footer / links',
    textFromSanitized(root, '#footer .foot-cta', 'footer cta'),
    textFrom(root, '#footer .foot-cols', 'footer cols'),
    textFrom(root, '#footer .foot-bottom', 'footer bottom'),
    '',
  )

  return `${chunks.join('\n').trimEnd()}\n`
}

/**
 * llms.txt: H1, blockquote summary, prose, homepage extract, then H2 sections with annotated links.
 */
export function buildLlmsTxt(html: string): string {
  const overview = overviewBodyWithoutLeadingH1()
  const home = buildHomepageExtractFromHtml(html)
  const tail = `
## Core pages

- [Homepage](https://iii.dev/) — positioning and visuals
- [Manifesto](https://iii.dev/manifesto) — paradigm argument
- [Documentation](https://iii.dev/docs) — full documentation
- [llms.txt](https://iii.dev/llms.txt) — this file (AI / LLM discovery)
- [AGENTS.md](https://iii.dev/AGENTS.md) — agent-focused product and wire-level notes
- [GitHub](https://github.com/iii-hq/iii) — engine, TypeScript/Python/Rust SDKs

## Optional

- [Worker registry](https://workers.iii.dev) — published workers

## Install / start

For **current install paths and prerequisites**, see **[iii.dev/docs/installation](https://iii.dev/docs/installation)**—this file may lag the docs.

\`\`\`bash
curl -fsSL https://install.iii.dev/iii/main/install.sh | sh
iii
\`\`\`

Engine **listeners, adapters, and ports** come from your project’s **\`config.yaml\`** (or the config path you pass). Read that file and the docs instead of relying on hardcoded port lists here.

- \`iii console\` — launch the web observability console
- \`iii --help\` and \`iii <subcommand> --help\` — discover CLI behavior
- \`iii trigger\` — useful for **manual** checks; **not** the main way apps invoke functions—use SDKs in workers for integration; **don’t** script against the CLI trigger

SDKs:

- Rust: \`cargo add iii-sdk\`
- Node (backend): \`npm install iii-sdk\`
- Node (browser, RBAC-scoped): \`npm install iii-browser-sdk\`
- Python: \`pip install iii-sdk\`

Last updated: ${isoDate()}
`.trimStart()

  const body = [
    '# iii',
    '',
    `> ${LLMS_TAGLINE}`,
    '',
    overview.trimEnd(),
    '',
    home.trimEnd(),
    '',
    tail.trimEnd(),
    '',
  ].join('\n')

  return `${body.trimEnd()}\n`
}

/**
 * AGENTS.md: [agents.md](https://agents.md/) product context + same pre-written overview + homepage extract + wire-level appendix.
 */
export function buildAgentsMd(html: string, agentsAppendix: string): string {
  const overview = overviewBodyWithoutLeadingH1()
  const home = buildHomepageExtractFromHtml(html)
  const intro = [
    '# iii for AI Agents',
    '',
    'This file is public **[AGENTS.md](https://agents.md/)**-style guidance for **[iii](https://iii.dev/)** (the product): positioning, comparisons, scraped homepage copy, and wire-level notes for autonomous agents.',
    '',
    '## Overview and comparisons (pre-written)',
    '',
    overview.trimEnd(),
    '',
    home.trimEnd(),
    '',
    agentsAppendix.trimEnd(),
    '',
    `Last updated: ${isoDate()}`,
    '',
  ].join('\n')

  return intro
}

async function main() {
  const [html, appendix] = await Promise.all([
    fs.readFile(INDEX_PATH, 'utf8'),
    fs.readFile(AGENTS_APPENDIX_PATH, 'utf8'),
  ])
  const llms = buildLlmsTxt(html)
  const agents = buildAgentsMd(html, appendix)
  await Promise.all([fs.writeFile(LLMS_PATH, llms, 'utf8'), fs.writeFile(AGENTS_PATH, agents, 'utf8')])
  console.log(
    `wrote ${path.relative(WEBSITE_ROOT, LLMS_PATH)} (${llms.length} b), ${path.relative(WEBSITE_ROOT, AGENTS_PATH)} (${agents.length} b)`,
  )
}

const isMain = import.meta.url === pathToFileURL(path.resolve(process.argv[1] ?? '')).href
if (isMain) {
  main().catch((err) => {
    console.error(err)
    process.exitCode = 1
  })
}

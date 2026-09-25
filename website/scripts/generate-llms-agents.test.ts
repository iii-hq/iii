import assert from "node:assert/strict"
import { existsSync } from "node:fs"
import fs from "node:fs/promises"
import path from "node:path"
import test from "node:test"
import { fileURLToPath } from "node:url"
import { buildBlogLinksSection } from "./generate-blog-md"
import {
  buildAgentsMd,
  buildHomepageExtractFromHtml,
  buildLlmsTxt,
  overviewBodyWithoutLeadingH1,
} from "./generate-llms-agents"

const INDEX_PATH = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../dist/index.html")
const APPENDIX_PATH = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "agents-appendix.md")

// A cut-down dist/index.html with the exact shapes the extractor selects on:
// the hero copy column (`data-llms="hero"`), one SectionIntro per section
// (`data-llms="intro"`: eyebrow <p>, <h2>, <p>), the #workers CTA row, the
// #hello worker cards, the #nutshell groups and the site <footer>.
const intro = (eyebrow: string, id: string, title: string, body: string) =>
  `<div data-llms="intro" class="max-w-[640px]"><p class="text-[13px]">${eyebrow}</p><h2 id="${id}-title">${title}</h2><p class="mt-4">${body}</p></div>`

const FIXTURE = `<!DOCTYPE html><html lang="en" class="dark"><head><title>iii</title></head><body>
<header><nav><a href="/">iii</a><button type="button">Menu</button></nav></header>
<main>
<section id="hero" aria-label="Hero">
  <div aria-hidden="true"><svg><text>decorative</text></svg></div>
  <div data-llms="hero" class="relative z-10 flex">
    <a href="/manifesto"><span class="rounded-full">Open source</span>Three primitives. Zero integration cost.<svg></svg></a>
    <h1><span class="block"><span class="intro-word">Stop</span> <span class="intro-word">integrating</span> <span class="intro-word">services.</span></span><span class="block"><span class="intro-word">Start</span> <span class="intro-word">adding</span> <span class="intro-word">them.</span></span></h1>
    <p class="intro mt-5">iii is one open-source engine. Every service and AI agent plugs in as a worker, in any language, with no glue code to write.</p>
    <div class="intro mt-9"><a href="https://iii.dev/docs/install">Get started</a><button type="button">curl -fsSL https://install.iii.dev/iii/main/install.sh | sh</button><button type="button">Using a coding agent? <span>Copy the prompt</span></button></div>
  </div>
  <div class="stage"><h3>Problem space</h3><p>01 / 03</p><p>Modern software stacks are an exercise in integrating services.</p></div>
</section>
<section id="console-live">${intro("Agents", "console-live", "Same run, from inside the harness.", "Your agentic harness is part of your system, so it runs faster, works better, and uses fewer tokens than any other harness.")}<div><p>iii console·payments ledger</p><pre><code>registerWorker()</code></pre></div></section>
<section id="harness">${intro("Harness", "harness", "Same work, two outcomes.", "One agent researches a stack and resolves merge conflicts. The other finds the workers it needs and ships in the same time, for a fraction of the tokens.")}<div><p>Same prompt, both sides.</p></div></section>
<section id="experience">${intro("Why iii", "experience", "Any task, one experience.", "iii makes it unreasonably efficient to create and extend software. Follow one request from a Slack message to running services.")}<div><p>Slack · #product</p><pre><code>iii.registerWorker()</code></pre></div></section>
<section id="hello">${intro("Languages", "hello", "Any language, one protocol.", "Python registers a function. Rust registers a function. Node consumes both.")}
  <div class="desktop"><article aria-label="Node.js worker, orchestrator"><pre><code>iii.registerFunction(\`\`\`)</code></pre></article><article aria-label="Rust worker, data transform"></article><article aria-label="Python worker, ML inference"></article></div>
  <div class="mobile"><article aria-label="Node.js worker, orchestrator"></article><article aria-label="Rust worker, data transform"></article><article aria-label="Python worker, ML inference"></article></div>
</section>
<section id="workers">${intro("Workers", "workers", "Any service, one abstraction.", "The answer to “we need X” stops being “evaluate, procure, integrate.” It becomes “add a worker.”")}
  <div role="img" aria-label="Searching the worker registry"><span>Search the registry</span><p>Postgres on iii</p></div>
  <div class="nw-cta-row mt-8"><a href="https://workers.iii.dev" target="_blank" rel="noopener noreferrer">View the worker registry<svg></svg></a></div>
</section>
<section id="nutshell">${intro("", "nutshell", "iii in a nutshell.", "Every capability, every framework, and every tool become a pattern on the same core system.")}
  <div class="mt-10">
    <div><div class="min-w-0"><h3>Execution model</h3><p>How iii runs work: durable, interoperable, simple.</p></div>
      <ul><li><h4>Durable orchestration</h4><p>Coordinate long-running, failure-tolerant execution across workers and triggers.</p></li><li><h4>Simple primitives</h4><p>Collapse distributed backend design into a paradigm humans and agents can reason about.</p></li></ul></div>
    <div><div class="min-w-0"><h3>Live system traits</h3><p>What iii becomes once running: discoverable, extensible, observable.</p></div>
      <ul><li><h4>Live discovery</h4><p>Functions and triggers exposed by one worker become visible across the system in real time.</p></li></ul></div>
  </div>
</section>
<section id="tech-specs">${intro("Tech specs", "roadmap", "Roadmap.", "Designs published before they’re built. Read the plan, step through the architecture.")}<ol><li><a href="/roadmap/2026-06-29-codegen/"><time>Jun 29, 2026</time><span>iii codegen</span><p>one command generates the types.</p></a></li></ol></section>
<section id="footer"><div data-llms="intro" class="max-w-[520px]"><p>Get started</p><h2 id="get-started-title">Install iii in one command.</h2><p>One line installs the engine. Then read the docs, star the repo, or come say hi on Discord.</p></div>
  <div><button type="button">curl -fsSL https://install.iii.dev/iii/main/install.sh | sh</button><a href="https://iii.dev/docs">Read the docs</a></div>
  <div><p>Follow development</p><p>New specs and releases, by email.</p><form><input type="email"><button type="submit">Subscribe</button></form></div>
</section>
</main>
<footer class="border-t">
  <div><svg viewBox="0 0 1075.74 1075.74"></svg><p>The open-source engine every service and AI agent plugs into.</p><p>Pronounced “three eye”.</p></div>
  <nav aria-label="Product"><h2>Product</h2><ul><li><a href="https://iii.dev/docs">Docs</a></li><li><a href="/roadmap">Roadmap</a></li><li><a href="https://workers.iii.dev" target="_blank">Worker registry<svg></svg><span class="sr-only">(opens in a new tab)</span></a></li></ul></nav>
  <nav aria-label="Company"><h2>Company</h2><ul><li><a href="/manifesto">Manifesto</a></li><li><a href="/blog">Blog</a></li></ul></nav>
  <div><div><span>Ask about iii on</span><ul><li><a href="https://chatgpt.com/?q=x" aria-label="Ask ChatGPT about iii"><svg></svg></a></li></ul></div><p>© 2026 Motia LLC</p><button type="button">Theme</button></div>
</footer>
</body></html>`

test("overviewBodyWithoutLeadingH1 drops duplicate H1 for llms.txt", () => {
  const body = overviewBodyWithoutLeadingH1()
  assert.ok(!body.startsWith("# "))
  assert.ok(body.includes("Three primitives"))
})

test("buildHomepageExtractFromHtml reads the hero as prose (headline words joined, one lead paragraph)", () => {
  const text = buildHomepageExtractFromHtml(FIXTURE)
  assert.ok(text.startsWith("## Homepage copy (extracted from iii.dev HTML)\n"))
  assert.ok(
    text.includes("### Hero\n**Stop integrating services. Start adding them.**\niii is one open-source engine."),
  )
  // Nothing from the hero's actions, eyebrow chip, or the animated stage window.
  const hero = text.slice(text.indexOf("### Hero"), text.indexOf("### Experience"))
  assert.ok(!hero.includes("Get started"))
  assert.ok(!hero.includes("Open source"))
  assert.ok(!text.includes("install.iii.dev"))
  assert.ok(!text.includes("Copy the prompt"))
  assert.ok(!text.includes("Problem space"))
  assert.ok(!text.includes("01 / 03"))
})

test("buildHomepageExtractFromHtml takes each section's intro only, never its demo copy or code", () => {
  const text = buildHomepageExtractFromHtml(FIXTURE)
  assert.ok(text.includes("### Experience\n**Any task, one experience.**\niii makes it unreasonably efficient"))
  assert.ok(text.includes("### Agents / console\n**Same run, from inside the harness.**\nYour agentic harness"))
  assert.ok(text.includes("### Harness race\n**Same work, two outcomes.**\nOne agent researches a stack"))
  assert.ok(!text.includes("Slack · #product"))
  assert.ok(!text.includes("payments ledger"))
  assert.ok(!text.includes("Same prompt, both sides"))
  assert.ok(!text.includes("registerWorker"))
  assert.ok(!text.includes("registerFunction"))
  assert.ok(!text.includes("```"))
})

test("buildHomepageExtractFromHtml keeps the workers CTA as a link and lists the hello workers once", () => {
  const text = buildHomepageExtractFromHtml(FIXTURE)
  assert.ok(
    text.includes(
      "### Workers\n**Any service, one abstraction.**\nThe answer to “we need X” stops being “evaluate, procure, integrate.” It becomes “add a worker.”\n[View the worker registry](https://workers.iii.dev)\n",
    ),
  )
  assert.ok(!text.includes("Search the registry"))
  assert.ok(
    text.includes(
      "### Languages / protocol\n**Any language, one protocol.**\nPython registers a function. Rust registers a function. Node consumes both.\n- Node.js worker, orchestrator\n- Rust worker, data transform\n- Python worker, ML inference\n",
    ),
  )
  assert.equal(text.match(/Node\.js worker, orchestrator/g)?.length, 1, "desktop and mobile flows dedupe")
})

test("buildHomepageExtractFromHtml renders nutshell groups with their cards as bullets", () => {
  const text = buildHomepageExtractFromHtml(FIXTURE)
  assert.ok(
    text.includes(
      [
        "### iii in a nutshell",
        "**iii in a nutshell.**",
        "Every capability, every framework, and every tool become a pattern on the same core system.",
        "",
        "**Execution model** How iii runs work: durable, interoperable, simple.",
        "- Durable orchestration: Coordinate long-running, failure-tolerant execution across workers and triggers.",
        "- Simple primitives: Collapse distributed backend design into a paradigm humans and agents can reason about.",
        "",
        "**Live system traits** What iii becomes once running: discoverable, extensible, observable.",
        "- Live discovery: Functions and triggers exposed by one worker become visible across the system in real time.",
        "",
      ].join("\n"),
    ),
  )
})

test("buildHomepageExtractFromHtml includes the roadmap intro only when the preview rendered", () => {
  const withRoadmap = buildHomepageExtractFromHtml(FIXTURE)
  assert.ok(withRoadmap.includes("### Roadmap\n**Roadmap.**\nDesigns published before they’re built."))
  assert.ok(!withRoadmap.includes("Jun 29, 2026"))

  const without = buildHomepageExtractFromHtml(FIXTURE.replace(/<section id="tech-specs">[\s\S]*?<\/section>/, ""))
  assert.ok(!without.includes("### Roadmap"))
  assert.ok(without.includes("### Get started"))
})

test("buildHomepageExtractFromHtml renders get-started prose and the footer as links, no controls", () => {
  const text = buildHomepageExtractFromHtml(FIXTURE)
  assert.ok(text.includes("### Get started\n**Install iii in one command.**\nOne line installs the engine."))
  assert.ok(!text.includes("Subscribe"))
  assert.ok(!text.includes("Follow development"))
  assert.ok(
    text.endsWith(
      [
        "### Footer / links",
        "The open-source engine every service and AI agent plugs into. Pronounced “three eye”.",
        "- Product: [Docs](https://iii.dev/docs), [Roadmap](https://iii.dev/roadmap), [Worker registry](https://workers.iii.dev)",
        "- Company: [Manifesto](https://iii.dev/manifesto), [Blog](https://iii.dev/blog)",
        "© 2026 Motia LLC",
        "",
      ].join("\n"),
    ),
  )
  assert.ok(!text.includes("(opens in a new tab)"))
  assert.ok(!text.includes("Theme"))
  assert.ok(!text.includes("Ask about iii on"))
})

test("buildHomepageExtractFromHtml fails loudly when a section hook is missing", () => {
  const broken = FIXTURE.replace('<section id="experience">', '<section id="experience-renamed">')
  assert.throws(() => buildHomepageExtractFromHtml(broken), /#experience not found/)
  const noHook = FIXTURE.replace('data-llms="hero"', "data-hero")
  assert.throws(() => buildHomepageExtractFromHtml(noHook), /\[data-llms="hero"\] not found/)
})

test("buildLlmsTxt is an understanding-first explainer (no spin-up instructions)", async () => {
  const blogSection = await buildBlogLinksSection()
  const text = buildLlmsTxt(FIXTURE, blogSection)
  assert.ok(text.startsWith("# iii\n"))
  assert.ok(text.includes("> iii turns distributed"))
  assert.ok(text.includes("## Three primitives"))
  assert.ok(text.includes("## How iii compares"))
  assert.ok(text.includes("## Core pages"))
  assert.ok(text.includes("[llms.txt](https://iii.dev/llms.txt)"))
  assert.ok(text.includes("Homepage copy (extracted"))
  // Chat mode explains iii; it must NOT tell the reader to install / spin up iii.
  // Those action blocks live in AGENTS.md, which llms.txt points to as the build path.
  assert.ok(!text.includes("## Guardrails"))
  assert.ok(!text.includes("## Install / start"))
  assert.ok(!text.includes("npx skills add iii-hq/iii/skills"))
  assert.ok(!text.includes("install.iii.dev"))
  assert.ok(text.includes("[AGENTS.md](https://iii.dev/AGENTS.md)"))
  assert.ok(text.includes("## Blog (knowledge base for coding agents)"))
  assert.ok(text.includes("https://iii.dev/blog/index.md"))
})

test("buildAgentsMd includes agents.md framing and appendix", async () => {
  const appendix = await fs.readFile(APPENDIX_PATH, "utf8")
  const blogSection = await buildBlogLinksSection()
  const md = buildAgentsMd(FIXTURE, appendix, blogSection)
  assert.ok(md.startsWith("# iii for AI Agents"))
  assert.ok(md.includes("agents.md"))
  assert.ok(md.includes("## Overview and comparisons"))
  assert.ok(md.includes("## Primitives (wire-level)"))
  assert.ok(md.includes("## Guardrails"))
  assert.ok(md.includes("## Agent skills (after onboarding)"))
  assert.ok(md.includes("npx skills add iii-hq/iii/skills"))
  assert.ok(md.includes("## Blog (knowledge base for coding agents)"))
  assert.ok(md.includes("https://iii.dev/blog/index.md"))
  assert.ok(md.includes("Last updated:"))
})

// The real built page, when present: the fixture above must not drift from it.
const needsDist = { skip: existsSync(INDEX_PATH) ? false : "dist/index.html missing — run `pnpm build` first" }

test("the built dist/index.html still carries every hook the extractor needs", needsDist, async () => {
  const html = await fs.readFile(INDEX_PATH, "utf8")
  const text = buildHomepageExtractFromHtml(html)
  assert.ok(text.includes("### Hero\n**Stop integrating services. Start adding them.**"))
  assert.ok(text.includes("unreasonably efficient"))
  assert.ok(text.includes("- Node.js worker, orchestrator"))
  assert.ok(text.includes("[View the worker registry](https://workers.iii.dev)"))
  assert.ok(text.includes("- Durable orchestration:"))
  assert.ok(text.includes("### Footer / links\nThe open-source engine"))
  assert.ok(!text.includes("registerWorker"))
  assert.ok(!text.includes("```"))
  assert.ok(!text.includes("(opens in a new tab)"))
})

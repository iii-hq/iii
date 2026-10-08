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
// the hero (<h1> + the first visible <p>), one Section head per section
// (eyebrow <p>, <h2>, lede, children of one <header>), the FAQPage JSON-LD,
// the final CTA and the site <footer> with its link columns.
const head = (eyebrow: string, id: string, title: string, lede: string) =>
  `<header class="reveal max-w-3xl"><p class="text-xs uppercase">${eyebrow}</p><h2 id="${id}-title"><span class="print block">${title}</span></h2><div class="mt-4">${lede}</div></header>`

const faqJsonLd = JSON.stringify({
  "@context": "https://schema.org",
  "@type": "FAQPage",
  mainEntity: [
    {
      "@type": "Question",
      name: "What is iii?",
      acceptedAnswer: { "@type": "Answer", text: "An engine that programs connect to over WebSocket." },
    },
    {
      "@type": "Question",
      name: "Do I need AI?",
      acceptedAnswer: { "@type": "Answer", text: "No. The harness is an optional worker." },
    },
  ],
})

const FIXTURE = `<!DOCTYPE html><html lang="en" class="dark"><head><title>iii</title></head><body>
<header class="fixed"><nav><a href="/">iii</a><button type="button">Menu</button></nav></header>
<main>
<section id="hero" aria-labelledby="hero-title">
  <div aria-hidden="true"><video></video><p>decorative caption</p></div>
  <div><span class="rounded-full">Three primitives: Function, Trigger, Worker</span>
    <h1 id="hero-title"><span class="print block"><span class="block">More application.</span><span class="block">Less infrastructure.</span></span></h1>
    <p><span class="text-foreground">The convenience of a platform, without the lock-in.</span> Connect any functions into one live system.</p>
    <div role="tablist"><button type="button">curl</button></div><p>Install the engine.</p><pre><code>curl -fsSL https://install.iii.dev/iii/main/install.sh | sh</code></pre>
  </div>
  <div><p>Release notes and new workers, by email</p><form><input type="email"><button type="submit">Subscribe</button></form></div>
</section>
<section id="overview" aria-labelledby="overview-title">${head("Overview", "overview", "Code shouldn't care where code runs.", "One engine. Any workload.")}<div aria-hidden="true"><svg><text>agent::run</text></svg></div></section>
<section id="demo" aria-labelledby="demo-title">${head("Live demo", "demo", "Watch iii execute.", "Follow one request through the code.")}<div><pre><code>iii.registerFunction()</code></pre><p>Trace · 12 spans</p></div></section>
<section id="numbers" aria-labelledby="numbers-title"><header><p>Numbers</p><h2 id="numbers-title">Built to execute.</h2></header><dl><dt>p50</dt><dd>0.4 ms</dd></dl></section>
<section id="faq" aria-labelledby="faq-title">${head("Questions", "faq", "Questions.", 'Something else? Ask in <a href="https://discord.gg/iiidev">Discord</a>.')}<div><button type="button">What is iii?</button></div></section>
<section id="final-cta" aria-labelledby="final-cta-title"><div><h2 id="final-cta-title">Make anything callable.</h2><p>Start building your iii system.</p><div><button type="button">curl -fsSL https://install.iii.dev/iii/main/install.sh | sh</button></div></div></section>
</main>
<footer class="border-t">
  <div><a href="#hero"><svg viewBox="0 0 933.61 1050.31"></svg></a><p>A next-generation software system. Workers. Triggers. Functions.</p><p>Pronounced "three eye"</p></div>
  <nav aria-label="Footer">
    <div><h3>Product</h3><ul><li><a href="https://iii.dev/docs">Docs</a></li><li><a href="/roadmap">Roadmap</a></li><li><a href="https://workers.iii.dev/" target="_blank">Worker registry<span class="sr-only">(opens in a new tab)</span></a></li></ul></div>
    <div><h3>Community</h3><ul><li><a href="https://github.com/iii-hq/iii">GitHub</a></li></ul></div>
    <div><h3>Ask about iii</h3><ul><li><a href="https://chatgpt.com/?q=x">ChatGPT</a></li></ul></div>
  </nav>
  <div><p>© 2026 Motia LLC <span>·</span> <a href="/privacy-policy">Privacy</a></p></div>
</footer>
<script type="application/ld+json">${faqJsonLd}</script>
</body></html>`

test("overviewBodyWithoutLeadingH1 drops duplicate H1 for llms.txt", () => {
  const body = overviewBodyWithoutLeadingH1()
  assert.ok(!body.startsWith("# "))
  assert.ok(body.includes("Three primitives"))
})

test("buildHomepageExtractFromHtml reads the hero as prose (headline lines joined, one lead paragraph)", () => {
  const text = buildHomepageExtractFromHtml(FIXTURE)
  assert.ok(text.startsWith("## Homepage copy (extracted from iii.dev HTML)\n"))
  assert.ok(
    text.includes(
      "### Hero\n**More application. Less infrastructure.**\nThe convenience of a platform, without the lock-in. Connect any functions into one live system.\n",
    ),
  )
  // Nothing from the decorative backdrop, the install card or the email row.
  assert.ok(!text.includes("decorative caption"))
  assert.ok(!text.includes("install.iii.dev"))
  assert.ok(!text.includes("Install the engine"))
  assert.ok(!text.includes("Release notes"))
  assert.ok(!text.includes("Subscribe"))
})

test("buildHomepageExtractFromHtml takes each section's head only, never its demo copy or code", () => {
  const text = buildHomepageExtractFromHtml(FIXTURE)
  assert.ok(text.includes("### Overview\n**Code shouldn't care where code runs.**\nOne engine. Any workload.\n"))
  assert.ok(text.includes("### Live demo\n**Watch iii execute.**\nFollow one request through the code.\n"))
  assert.ok(text.includes("### Numbers\n**Built to execute.**\n\n"))
  assert.ok(!text.includes("agent::run"))
  assert.ok(!text.includes("Trace · 12 spans"))
  assert.ok(!text.includes("registerFunction"))
  assert.ok(!text.includes("0.4 ms"))
  assert.ok(!text.includes("```"))
})

test("buildHomepageExtractFromHtml reads the FAQ from the FAQPage JSON-LD", () => {
  const text = buildHomepageExtractFromHtml(FIXTURE)
  assert.ok(
    text.includes(
      "### Questions\n**What is iii?**\nAn engine that programs connect to over WebSocket.\n\n**Do I need AI?**\nNo. The harness is an optional worker.\n",
    ),
  )
  assert.ok(!text.includes("Something else?"), "the FAQ section head is replaced by its questions")
})

test("buildHomepageExtractFromHtml renders the final CTA and the footer as links, no controls", () => {
  const text = buildHomepageExtractFromHtml(FIXTURE)
  assert.ok(text.includes("### Get started\n**Make anything callable.**\nStart building your iii system.\n"))
  assert.ok(
    text.endsWith(
      [
        "### Footer / links",
        'A next-generation software system. Workers. Triggers. Functions. Pronounced "three eye"',
        "- Product: [Docs](https://iii.dev/docs), [Roadmap](https://iii.dev/roadmap), [Worker registry](https://workers.iii.dev/)",
        "- Community: [GitHub](https://github.com/iii-hq/iii)",
        "© 2026 Motia LLC",
        "",
      ].join("\n"),
    ),
  )
  assert.ok(!text.includes("(opens in a new tab)"))
  assert.ok(!text.includes("chatgpt.com"))
})

test("buildHomepageExtractFromHtml fails loudly when a required hook is missing", () => {
  assert.throws(
    () => buildHomepageExtractFromHtml(FIXTURE.replace('id="hero"', 'id="hero-renamed"')),
    /#hero not found/,
  )
  assert.throws(
    () => buildHomepageExtractFromHtml(FIXTURE.replace(/<script type="application\/ld\+json">[\s\S]*?<\/script>/, "")),
    /FAQPage JSON-LD not found/,
  )
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
  assert.ok(text.includes("### Hero\n**More application. Less infrastructure.**"))
  assert.ok(text.includes("### Overview\n**Code shouldn't care where code runs.**"))
  assert.ok(text.includes("### Questions\n**What is iii?**"))
  assert.ok(text.includes("### Get started\n**Make anything callable.**"))
  assert.ok(text.includes("### Footer / links\nA next-generation software system."))
  assert.ok(!text.includes("install.iii.dev"))
  assert.ok(!text.includes("registerWorker"))
  assert.ok(!text.includes("```"))
  assert.ok(!text.includes("(opens in a new tab)"))
})

import { existsSync, mkdirSync, writeFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { parseGodoc, parseHelpersGodoc } from './parsers/parse-godoc.mjs'
import { parseHelpersGriffe, parseGriffe } from './parsers/parse-griffe.mjs'
import { parseHelpersRustdoc, parseRustdoc } from './parsers/parse-rustdoc.mjs'
import { parseBrowserTypedoc, parseHelpersTypedoc, parseNodeTypedoc } from './parsers/parse-typedoc.mjs'
import { renderSdkMdx } from './renderers/render-mdx.mjs'
import type { SdkDoc } from './types.mjs'

const __dirname = dirname(fileURLToPath(import.meta.url))
// Inputs are resolved relative to the repo root (the pipeline runs from there).
const ROOT = process.cwd()
const DOCS_OUTPUT = resolve(__dirname, '../reference')
// rustdoc writes its JSON under the cargo target dir.
const CARGO_TARGET = process.env.CARGO_TARGET_DIR ? resolve(process.env.CARGO_TARGET_DIR) : resolve(ROOT, 'target')
// --strict: a missing extraction JSON fails the run (the drift check needs every page).
const STRICT = process.argv.includes('--strict')

interface GenerationTarget {
  name: string
  jsonPath: string
  outputPath: string
  parser: (jsonPath: string, helpersJsonPath?: string) => SdkDoc
  /** SDK source dir whose doc-comments produce this page's prose. */
  sourcePath: string
  /** Helpers crate JSON, so types the SDK re-exports from it get documented. */
  helpersJsonPath?: string
}

const targets: GenerationTarget[] = [
  // ── Core SDKs (client + worker entry point) ──
  {
    name: 'Node.js SDK',
    jsonPath: resolve(ROOT, 'sdk/packages/node/iii/api-docs.json'),
    outputPath: resolve(DOCS_OUTPUT, 'sdk-node.mdx'),
    parser: parseNodeTypedoc,
    sourcePath: 'sdk/packages/node/iii/src',
  },
  {
    name: 'Python SDK',
    jsonPath: resolve(ROOT, 'sdk/packages/python/iii/api-docs.json'),
    outputPath: resolve(DOCS_OUTPUT, 'sdk-python.mdx'),
    parser: parseGriffe,
    sourcePath: 'sdk/packages/python/iii/src',
  },
  {
    name: 'Rust SDK',
    jsonPath: resolve(CARGO_TARGET, 'doc/iii_sdk.json'),
    outputPath: resolve(DOCS_OUTPUT, 'sdk-rust.mdx'),
    parser: parseRustdoc,
    sourcePath: 'sdk/packages/rust/iii/src',
    helpersJsonPath: resolve(CARGO_TARGET, 'doc/iii_helpers.json'),
  },
  {
    // One go/doc dump (docs/next/scripts/godoc-json) covers the whole module:
    // this page takes package iii and its non-helper subpackages.
    name: 'Go SDK',
    jsonPath: resolve(ROOT, 'sdk/packages/go/iii/api-docs.json'),
    outputPath: resolve(DOCS_OUTPUT, 'sdk-go.mdx'),
    parser: parseGodoc,
    sourcePath: 'sdk/packages/go/iii',
  },
  {
    name: 'Browser SDK',
    jsonPath: resolve(ROOT, 'sdk/packages/node/iii-browser/api-docs.json'),
    outputPath: resolve(DOCS_OUTPUT, 'sdk-browser.mdx'),
    parser: parseBrowserTypedoc,
    sourcePath: 'sdk/packages/node/iii-browser/src',
  },
  // ── @iii-dev/helpers / iii-helpers (library: per-submodule) ──
  {
    name: 'Helpers (Node.js)',
    jsonPath: resolve(ROOT, 'sdk/packages/node/helpers/api-docs.json'),
    outputPath: resolve(DOCS_OUTPUT, 'helpers-node.mdx'),
    parser: parseHelpersTypedoc,
    sourcePath: 'sdk/packages/node/helpers/src',
  },
  {
    name: 'Helpers (Python)',
    jsonPath: resolve(ROOT, 'sdk/packages/python/helpers/api-docs.json'),
    outputPath: resolve(DOCS_OUTPUT, 'helpers-python.mdx'),
    parser: parseHelpersGriffe,
    sourcePath: 'sdk/packages/python/helpers/src',
  },
  {
    name: 'Helpers (Rust)',
    jsonPath: resolve(CARGO_TARGET, 'doc/iii_helpers.json'),
    outputPath: resolve(DOCS_OUTPUT, 'helpers-rust.mdx'),
    parser: parseHelpersRustdoc,
    sourcePath: 'sdk/packages/rust/helpers/src',
  },
  {
    // Same go/doc dump as the Go SDK page: iii/helpers/... and iii/observability.
    name: 'Helpers (Go)',
    jsonPath: resolve(ROOT, 'sdk/packages/go/iii/api-docs.json'),
    outputPath: resolve(DOCS_OUTPUT, 'helpers-go.mdx'),
    parser: parseHelpersGodoc,
    sourcePath: 'sdk/packages/go/iii',
  },
]

mkdirSync(DOCS_OUTPUT, { recursive: true })

let hasErrors = false

for (const target of targets) {
  console.log(`\n[generate-api-docs] Processing ${target.name}...`)

  if (!existsSync(target.jsonPath)) {
    const log = STRICT ? console.error : console.warn
    log(`  [${STRICT ? 'ERROR' : 'SKIP'}] JSON file not found: ${target.jsonPath}`)
    log(`  Run the extraction step first (see .github/workflows/generate-api-docs.yml).`)
    if (STRICT) hasErrors = true
    continue
  }

  try {
    const doc = target.parser(target.jsonPath, target.helpersJsonPath)
    doc.metadata.docSourcePath = target.sourcePath
    const mdx = renderSdkMdx(doc)
    writeFileSync(target.outputPath, mdx, 'utf-8')
    const counts = doc.isLibrary
      ? `Modules: ${doc.modules?.length ?? 0}, Types: ${(doc.modules ?? []).reduce((n, m) => n + m.types.length, 0)}`
      : `Methods: ${doc.methods.length}, Types: ${doc.types.length}`
    console.log(`  [OK] ${target.outputPath}  (${counts})`)
  } catch (err) {
    console.error(`  [ERROR] Failed to generate ${target.name}:`, err)
    hasErrors = true
  }
}

console.log('\n[generate-api-docs] Done.')
if (hasErrors) process.exit(1)

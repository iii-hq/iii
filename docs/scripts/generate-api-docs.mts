import { existsSync, writeFileSync } from 'node:fs'
import { resolve, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import { parseTypedoc } from './parsers/parse-typedoc.mjs'
import { parseGriffe } from './parsers/parse-griffe.mjs'
import { parseRustdoc } from './parsers/parse-rustdoc.mjs'
import { renderSdkMdx } from './renderers/render-mdx.mjs'
import type { SdkDoc, FunctionDoc, TypeDoc } from './types.mjs'

const __dirname = dirname(fileURLToPath(import.meta.url))
const ROOT = resolve(__dirname, '../..')
const DOCS_OUTPUT = resolve(__dirname, '../api-reference')

interface GenerationTarget {
  name: string
  jsonPath: string
  outputPath: string
  parser: (jsonPath: string) => ReturnType<typeof parseTypedoc>
}

const targets: GenerationTarget[] = [
  {
    name: 'Node.js',
    jsonPath: resolve(ROOT, 'sdk/packages/node/iii/api-docs.json'),
    outputPath: resolve(DOCS_OUTPUT, 'sdk-node.mdx'),
    parser: parseTypedoc,
  },
  {
    name: 'Python',
    jsonPath: resolve(ROOT, 'sdk/packages/python/iii/api-docs.json'),
    outputPath: resolve(DOCS_OUTPUT, 'sdk-python.mdx'),
    parser: parseGriffe,
  },
  {
    name: 'Rust',
    jsonPath: resolve(ROOT, 'target/doc/iii_sdk.json'),
    outputPath: resolve(DOCS_OUTPUT, 'sdk-rust.mdx'),
    parser: parseRustdoc,
  },
]

function escapeRegExp(str: string): string {
  return str.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
}

function filterReferencedTypes(doc: SdkDoc): SdkDoc {
  const typesByName = new Map(doc.types.map(t => [t.name, t]))
  const allNames = new Set(typesByName.keys())
  const referenced = new Set<string>()

  function scan(typeStr: string) {
    for (const name of allNames) {
      if (referenced.has(name)) continue
      if (new RegExp(`\\b${escapeRegExp(name)}\\b`).test(typeStr)) {
        referenced.add(name)
        const typeDef = typesByName.get(name)
        if (typeDef) {
          for (const field of typeDef.fields) scan(field.type)
          if (typeDef.codeBlock) scan(typeDef.codeBlock)
        }
      }
    }
  }

  function scanFunction(fn: FunctionDoc) {
    for (const param of fn.params) scan(param.type)
    scan(fn.returns.type)
    scan(fn.signature)
  }

  scanFunction(doc.initialization.entryPoint)
  for (const method of doc.methods) scanFunction(method)

  return { ...doc, types: doc.types.filter(t => referenced.has(t.name)) }
}

let hasErrors = false

for (const target of targets) {
  console.log(`\n[generate-api-docs] Processing ${target.name} SDK...`)

  if (!existsSync(target.jsonPath)) {
    console.warn(`  [SKIP] JSON file not found: ${target.jsonPath}`)
    console.warn(`  Run the extraction step first.`)
    continue
  }

  try {
    const raw = target.parser(target.jsonPath)
    const doc = filterReferencedTypes(raw)
    const mdx = renderSdkMdx(doc)
    writeFileSync(target.outputPath, mdx, 'utf-8')
    console.log(`  [OK] Generated ${target.outputPath}`)
    console.log(`  Methods: ${doc.methods.length}, Types: ${doc.types.length} (${raw.types.length} extracted, ${raw.types.length - doc.types.length} filtered)`)
  } catch (err) {
    console.error(`  [ERROR] Failed to generate ${target.name} docs:`, err)
    hasErrors = true
  }
}

console.log('\n[generate-api-docs] Done.')

if (hasErrors) {
  process.exit(1)
}

// Generates the pretty-URL → .html route map that deploy-website.yml syncs into
// the CloudFront KeyValueStore. The map is derived from the set of top-level
// *.html files in the built dist/, minus the ones that are not pages: index.html
// (the apex default root object, served without a rewrite), 404.html (the
// export's not-found document; the edge answers unknown paths itself) and
// Next's underscore-prefixed internals (_not-found.html). Adding a page is
// therefore a content-only change: add src/app/(site)/foo/page.tsx, link to
// /foo, and the next deploy makes the clean URL resolve — no `terraform apply`
// (MOT-3669). See infra/terraform/website/README.md.
//
// Usage (after `pnpm build`):
//   tsx scripts/routes-kvs.ts                    # print desired map: [{Key,Value}]
//   tsx scripts/routes-kvs.ts --current cur.json # print {Puts,Deletes} vs current
import { readdirSync, readFileSync } from "node:fs"
import { dirname, resolve } from "node:path"
import { fileURLToPath, pathToFileURL } from "node:url"

export interface KvsEntry {
  Key: string
  Value: string
}

export interface KvsOps {
  Puts: KvsEntry[]
  Deletes: { Key: string }[]
}

const HTML_EXT = ".html"

// Files Next emits at the top level that must never become pretty routes.
const NOT_PAGES = new Set(["index.html", "404.html"])

export function isRoutablePage(file: string): boolean {
  return file.endsWith(HTML_EXT) && !NOT_PAGES.has(file) && !file.startsWith("_")
}

// Map each top-level page file (foo.html) to its pretty-URL key (/foo → /foo.html).
export function desiredRoutes(htmlFiles: string[]): KvsEntry[] {
  return htmlFiles
    .filter(isRoutablePage)
    .map((f) => ({ Key: `/${f.slice(0, -HTML_EXT.length)}`, Value: `/${f}` }))
    .sort((a, b) => a.Key.localeCompare(b.Key))
}

// Diff desired against the store's current contents into a single batch update:
// Puts for new or changed values, Deletes for keys no longer backed by a file.
export function diff(desired: KvsEntry[], current: KvsEntry[]): KvsOps {
  const currentByKey = new Map(current.map((e) => [e.Key, e.Value]))
  const desiredKeys = new Set(desired.map((e) => e.Key))
  return {
    Puts: desired.filter((e) => currentByKey.get(e.Key) !== e.Value),
    Deletes: current.filter((e) => !desiredKeys.has(e.Key)).map((e) => ({ Key: e.Key })),
  }
}

function listFiles(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true })
    .filter((d) => d.isFile())
    .map((d) => d.name)
}

function main(): void {
  const distDir = resolve(dirname(fileURLToPath(import.meta.url)), "../dist")
  const desired = desiredRoutes(listFiles(distDir))

  const currentFlagIdx = process.argv.indexOf("--current")
  if (currentFlagIdx === -1) {
    process.stdout.write(`${JSON.stringify(desired)}\n`)
    return
  }

  const currentPath = process.argv[currentFlagIdx + 1]
  if (!currentPath) {
    console.error("--current requires a file path")
    process.exitCode = 1
    return
  }
  // `aws cloudfront-keyvaluestore list-keys --query Items` yields [] for an empty
  // store and may yield null when the query misses — normalize both to [].
  const raw = readFileSync(currentPath, "utf8").trim()
  const current: KvsEntry[] = raw && raw !== "null" ? JSON.parse(raw) : []
  process.stdout.write(`${JSON.stringify(diff(desired, current))}\n`)
}

const invokedDirectly = process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href
if (invokedDirectly) {
  main()
}

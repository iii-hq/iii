// Generates the pretty-URL → .html route map that deploy-website.yml syncs into
// the CloudFront KeyValueStore. The map is derived from the set of top-level
// *.html files in website/ (index.html excluded — it's the apex default root
// object, served without a rewrite). Adding a page is therefore a content-only
// change: drop foo.html, link to /foo, and the next deploy makes the clean URL
// resolve — no `terraform apply` (MOT-3669). See infra/terraform/website/README.md.
//
// Usage (run from the website/ package dir):
//   tsx scripts/routes-kvs.ts                    # print desired map: [{Key,Value}]
//   tsx scripts/routes-kvs.ts --current cur.json # print {Puts,Deletes} vs current
import { readdirSync, readFileSync } from 'node:fs'
import { pathToFileURL } from 'node:url'

export interface KvsEntry {
  Key: string
  Value: string
}

export interface KvsOps {
  Puts: KvsEntry[]
  Deletes: { Key: string }[]
}

const HTML_EXT = '.html'

// Map each top-level page file (foo.html) to its pretty-URL key (/foo → /foo.html).
// index.html is the apex default_root_object and is served without a rewrite.
export function desiredRoutes(htmlFiles: string[]): KvsEntry[] {
  return htmlFiles
    .filter((f) => f.endsWith(HTML_EXT) && f !== 'index.html')
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
  const desired = desiredRoutes(listFiles(process.cwd()))

  const currentFlagIdx = process.argv.indexOf('--current')
  if (currentFlagIdx === -1) {
    process.stdout.write(`${JSON.stringify(desired)}\n`)
    return
  }

  const currentPath = process.argv[currentFlagIdx + 1]
  if (!currentPath) {
    console.error('--current requires a file path')
    process.exitCode = 1
    return
  }
  // `aws cloudfront-keyvaluestore list-keys --query Items` yields [] for an empty
  // store and may yield null when the query misses — normalize both to [].
  const raw = readFileSync(currentPath, 'utf8').trim()
  const current: KvsEntry[] = raw && raw !== 'null' ? JSON.parse(raw) : []
  process.stdout.write(`${JSON.stringify(diff(desired, current))}\n`)
}

const invokedDirectly = process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href
if (invokedDirectly) {
  main()
}

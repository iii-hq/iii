// Runs right after `next build` (see the package build script). Next exports
// every page as <route>.html. The edge (infra/terraform/website/cloudfront_functions/
// redirects.js) serves two prefixes as directory sites: /blog/<x>/ and /roadmap/<x>/
// are rewritten to <x>/index.html, and the bare /blog and /roadmap 301 to the slash
// form. Top-level pages stay extensionless via the KVS route map (/manifesto →
// /manifesto.html, scripts/routes-kvs.ts). So under blog/ and roadmap/ every
// page file moves into a directory as index.html; everything else (top-level
// *.html, the *.txt RSC payloads, route-handler files such as rss.xml and
// index.json, raw spec markdown) stays where Next put it.
import { existsSync, mkdirSync, readdirSync, renameSync } from "node:fs"
import { dirname, join, resolve } from "node:path"
import { fileURLToPath } from "node:url"

export const DIR_SITES = ["blog", "roadmap"]

/** dist/blog/foo.html → dist/blog/foo/index.html */
function moveIntoDir(file: string): string {
  const dir = file.slice(0, -".html".length)
  mkdirSync(dir, { recursive: true })
  const target = join(dir, "index.html")
  renameSync(file, target)
  return target
}

function walk(dir: string, out: string[]): void {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const p = join(dir, entry.name)
    if (entry.isDirectory()) walk(p, out)
    else if (entry.name.endsWith(".html") && entry.name !== "index.html") out.push(p)
  }
}

export function finalizeDist(dist: string): string[] {
  const moved: string[] = []
  for (const site of DIR_SITES) {
    const top = join(dist, `${site}.html`)
    if (existsSync(top)) moved.push(moveIntoDir(top))
    const dir = join(dist, site)
    if (!existsSync(dir)) continue
    const pages: string[] = []
    walk(dir, pages)
    for (const page of pages) moved.push(moveIntoDir(page))
  }
  return moved
}

const isMain = process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)
if (isMain) {
  const dist = resolve(dirname(fileURLToPath(import.meta.url)), "../dist")
  if (!existsSync(dist)) throw new Error("finalize-dist: dist/ missing — run `next build` first")
  const moved = finalizeDist(dist)
  console.log(`finalize-dist: ${moved.length} page(s) moved into directory form`)
}

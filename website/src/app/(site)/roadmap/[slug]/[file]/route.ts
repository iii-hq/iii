import { readFileSync } from "node:fs"
import { join, resolve } from "node:path"
import { getAllSpecs } from "@/lib/specs"

// /roadmap/<slug>/<file>.md — the raw spec markdown beside its rendered page,
// as the Astro site served it (frontmatter included). One file per markdown
// document in tech-specs/<slug>/, drafts included; prerendered at build time.
export const dynamic = "force-static"
export const dynamicParams = false

const SPECS_DIR = resolve(process.cwd(), "../tech-specs")

type Params = { slug: string; file: string }

export function generateStaticParams(): Params[] {
  return getAllSpecs().flatMap((spec) => spec.docs.map((doc) => ({ slug: spec.slug, file: doc.file })))
}

export async function GET(_request: Request, { params }: { params: Promise<Params> }): Promise<Response> {
  const { slug, file } = await params
  const spec = getAllSpecs().find((s) => s.slug === slug)
  if (!spec || !spec.docs.some((d) => d.file === file)) return new Response("not found", { status: 404 })
  return new Response(readFileSync(join(SPECS_DIR, slug, file), "utf8"), {
    headers: { "Content-Type": "text/markdown; charset=utf-8" },
  })
}

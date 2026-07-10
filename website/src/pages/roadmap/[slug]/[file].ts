// /roadmap/<slug>/<file>.md — every spec's raw markdown, directly linkable
// beside its rendered page (all .md files in the spec dir, matching what the
// old roadmap build copied verbatim).
import { readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import type { APIContext } from 'astro'
import { readSpecs, SPECS_DIR } from '../../../../roadmap/scripts/manifest.mjs'

export function getStaticPaths() {
  const { specs } = readSpecs()
  return specs.flatMap((spec) =>
    readdirSync(join(SPECS_DIR, spec.slug))
      .filter((f) => f.endsWith('.md'))
      .map((file) => ({ params: { slug: spec.slug, file } })),
  )
}

export function GET({ params }: APIContext): Response {
  const body = readFileSync(join(SPECS_DIR, String(params.slug), String(params.file)), 'utf8')
  return new Response(body, {
    headers: { 'Content-Type': 'text/markdown; charset=utf-8' },
  })
}

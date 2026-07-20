// /roadmap/<slug>/spec.json — the manifest the generic markdown viewer
// (roadmap/_viewer) fetches at runtime. Only emitted for specs WITHOUT a
// deck, matching the old roadmap build.
import type { APIContext } from 'astro'
import { listSpecDocs, readSpecs } from '../../../../roadmap/scripts/manifest.mjs'

export function getStaticPaths() {
  const { specs } = readSpecs()
  return specs.filter((spec) => !spec.hasDeck).map((spec) => ({ params: { slug: spec.slug }, props: { spec } }))
}

export function GET({ props }: APIContext): Response {
  const { spec } = props
  const manifest = {
    slug: spec.slug,
    title: spec.title,
    tagline: spec.tagline,
    docs: listSpecDocs(spec.slug),
  }
  return new Response(`${JSON.stringify(manifest, null, 2)}\n`, {
    headers: { 'Content-Type': 'application/json' },
  })
}

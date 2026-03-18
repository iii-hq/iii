import { loader } from 'fumadocs-core/source'
import { docs } from '@/.source'

// See https://fumadocs.vercel.app/docs/headless/source-api for more info
export const baseSource = loader({
  // it assigns a URL to your pages
  baseUrl: '/docs',
  source: docs.toFumadocsSource(),
})

const isProduction = process.env.NODE_ENV === 'production'

export function isLegacyExamplesSlug(slug?: string[]) {
  return slug?.[0] === 'examples'
}

export const source = {
  ...baseSource,
  getPage(slug?: string[]) {
    // Keep legacy examples files in-repo, but hide all example routes in production.
    if (isProduction && isLegacyExamplesSlug(slug)) return undefined
    return baseSource.getPage(slug)
  },
  generateParams() {
    const params = baseSource.generateParams()
    if (!isProduction) return params
    return params.filter((item) => !isLegacyExamplesSlug(item.slug))
  },
}

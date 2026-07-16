// Top-level marketing routes, served extensionless from dist/*.html via the
// CloudFront KVS route map (see routes-kvs.ts). Page titles, descriptions,
// and OG meta live in each page's frontmatter under src/pages/ — this
// registry only feeds the sitemap generator.
export interface RouteMeta {
  path: string
}

export const INDEXABLE_ROUTES: RouteMeta[] = [{ path: '/' }, { path: '/manifesto' }, { path: '/privacy-policy' }]

export const SITE_ORIGIN = 'https://iii.dev'

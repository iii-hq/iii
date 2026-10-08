import type { MetadataRoute } from 'next'

import { site } from '@/lib/seo'

/**
 * The pages this app serves that should be indexed. /a is left out on purpose while it is a test variant (its
 * canonical is "/"), and /a/stage is a recording page. /roadmap is served but not listed: the live iii.dev robots.txt
 * keeps crawlers off /roadmap (app/robots.ts mirrors it), so listing it would contradict that. The docs keep their own
 * sitemap at /docs/sitemap.xml.
 */
export default function sitemap(): MetadataRoute.Sitemap {
  const now = new Date()
  return [
    { url: `${site.url}/`, lastModified: now, changeFrequency: 'weekly', priority: 1 },
    { url: `${site.url}/manifesto`, lastModified: now, changeFrequency: 'monthly', priority: 0.8 },
  ]
}

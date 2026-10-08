import type { MetadataRoute } from 'next'

import { site } from '@/lib/seo'

/**
 * robots.txt. Same rules as the live iii.dev (website/public/robots.txt) so serving this app at the apex changes
 * nothing for crawlers, plus the recording stage. Search and AI-search crawlers are welcome: being cited by
 * assistants is part of how developers find iii.
 */
const disallow = [
  '/a/stage',
  '/preview',
  '/roadmap',
  '/docs/_next/',
  '/docs/_mintlify/',
  '/docs/0-10-0/',
  '/*?ref=',
  '/*?dpl=',
]

const aiCrawlers = [
  'GPTBot',
  'ChatGPT-User',
  'OAI-SearchBot',
  'ClaudeBot',
  'anthropic-ai',
  'PerplexityBot',
  'Perplexity-User',
  'Google-Extended',
  'Applebot-Extended',
  'Bingbot',
  'DuckAssistBot',
  'CCBot',
]

export default function robots(): MetadataRoute.Robots {
  return {
    rules: [
      { userAgent: '*', allow: '/', disallow },
      { userAgent: aiCrawlers, allow: '/', disallow: ['/a/stage', '/preview', '/roadmap'] },
    ],
    sitemap: [`${site.url}/sitemap.xml`, `${site.url}/docs/sitemap.xml`],
    host: site.url,
  }
}

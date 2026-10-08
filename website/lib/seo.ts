import type { Metadata } from 'next'

import { links } from '@/lib/site'

/**
 * Search and sharing metadata for the site. One source for the canonical origin, the names, the keywords and the
 * structured data, so the homepage and the /a test page describe iii the same way. The Organization, WebSite and
 * SoftwareApplication entries mirror the live iii.dev (website/src/lib/json-ld.ts), so nothing changes for search
 * engines when this app replaces it.
 */

export const site = {
  url: 'https://iii.dev',
  name: 'iii',
  /** Default title, used where a page sets none. Under 60 characters so it is not truncated in results. */
  title: 'iii: anything that opens a WebSocket can join your backend',
  /** Under 160 characters. */
  description:
    'iii is one engine your APIs, jobs, devices, and AI agents connect to. Each registers functions by name; any other can call, react to, and trace them.',
  twitter: '@iiidevs',
  locale: 'en_US',
  keywords: [
    'iii',
    'worker',
    'trigger',
    'function',
    'three primitives',
    'backend engine',
    'distributed systems',
    'durable execution',
    'polyglot runtime',
    'AI agents',
    'agent runtime',
    'agent harness',
    'TypeScript',
    'Python',
    'Rust',
    'WebSocket',
    'live discovery',
    'observability',
    'OpenTelemetry',
    'queues',
    'cron',
    'http',
    'state',
    'sandbox',
  ],
} as const

/** The share card, served from app/opengraph-image.png (the live iii.dev card). */
const shareImage = {
  url: '/opengraph-image.png',
  width: 1200,
  height: 630,
  alt: 'iii: unreasonably simple software engineering',
}

/**
 * Full metadata for one page. Next.js replaces (does not merge) a parent's `openGraph` and `twitter` objects, so a
 * page that sets its own title must restate the type, site name, card and image too; this keeps every page whole.
 */
export function pageMetadata({
  title,
  description,
  path = '/',
  canonical = path,
}: {
  title: string
  description: string
  /** The URL the page is served at. */
  path?: string
  /** Where search engines should consolidate it (defaults to itself). */
  canonical?: string
}): Metadata {
  return {
    title: { absolute: title },
    description,
    alternates: { canonical },
    openGraph: {
      type: 'website',
      siteName: site.name,
      locale: site.locale,
      url: canonical,
      title,
      description,
      images: [shareImage],
    },
    twitter: {
      card: 'summary_large_image',
      site: site.twitter,
      creator: site.twitter,
      title,
      description,
      images: [shareImage.url],
    },
  }
}

const organization = {
  '@type': 'Organization',
  '@id': `${site.url}/#organization`,
  name: 'iii',
  legalName: 'III, Inc.',
  url: `${site.url}/`,
  logo: `${site.url}/icon.svg`,
  sameAs: [links.github, links.x, links.linkedin, links.discord],
}

/** Organization, WebSite and SoftwareApplication, linked by @id so they read as one entity graph. */
export function siteJsonLd() {
  return {
    '@context': 'https://schema.org',
    '@graph': [
      organization,
      {
        '@type': 'WebSite',
        '@id': `${site.url}/#website`,
        name: 'iii',
        alternateName: 'iii.dev',
        url: `${site.url}/`,
        publisher: { '@id': organization['@id'] },
        inLanguage: 'en',
      },
      {
        '@type': 'SoftwareApplication',
        '@id': `${site.url}/#software`,
        name: 'iii',
        applicationCategory: 'DeveloperApplication',
        operatingSystem: 'Linux, macOS, Windows',
        description:
          'An engine and one open protocol (JSON over WebSocket) built on three primitives: Function, Trigger, Worker. Any process, in any language, on any runtime, that speaks the protocol joins as a worker.',
        url: `${site.url}/`,
        downloadUrl: links.install,
        softwareHelp: links.docs,
        license: 'https://www.elastic.co/licensing/elastic-license',
        programmingLanguage: ['Rust', 'TypeScript', 'Python'],
        offers: { '@type': 'Offer', price: '0', priceCurrency: 'USD' },
        publisher: { '@id': organization['@id'] },
      },
    ],
  }
}

/** FAQPage from the questions the page actually shows (answers as plain text, code backticks removed). */
export function faqJsonLd(faqs: readonly { q: string; a: string }[]) {
  return {
    '@context': 'https://schema.org',
    '@type': 'FAQPage',
    mainEntity: faqs.map((item) => ({
      '@type': 'Question',
      name: item.q,
      acceptedAnswer: { '@type': 'Answer', text: item.a.replace(/`/g, '') },
    })),
  }
}

/** Serialises JSON-LD for a <script> tag, escaping `<` so the payload cannot close the tag (Next.js guide). */
export const jsonLdHtml = (data: unknown) => JSON.stringify(data).replace(/</g, '\\u003c')

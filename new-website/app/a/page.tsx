import type { Metadata } from 'next'

import { VersionA } from '@/components/a/version-a'
import { faqs } from '@/components/sections/faq'
import { JsonLd } from '@/components/site/json-ld'
import { faqJsonLd, pageMetadata, siteJsonLd } from '@/lib/seo'

const title = 'iii: More application. Less infrastructure.'
const description =
  'Connect functions, agents, APIs, models, browsers, and machines into one live, composable system. The convenience of a platform, without the lock-in.'

/**
 * /a is variant A of the landing page test. While the test runs it is a duplicate of the homepage's purpose, so it
 * points its canonical at "/" (Google's guidance for test variants: canonical to the original, not noindex) and is
 * left out of the sitemap. When this version replaces the homepage, move the component to app/page.tsx and delete
 * this route (add a redirect from /a to / so shared links keep working).
 */
export const metadata: Metadata = pageMetadata({ title, description, path: '/a', canonical: '/' })

export default function Page() {
  return (
    <>
      <VersionA />
      <JsonLd data={siteJsonLd()} />
      <JsonLd data={faqJsonLd(faqs)} />
    </>
  )
}

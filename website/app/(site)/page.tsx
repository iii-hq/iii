import type { Metadata } from 'next'

import { Landing } from '@/components/landing/landing'
import { faqs } from '@/components/sections/faq'
import { JsonLd } from '@/components/site/json-ld'
import { faqJsonLd, pageMetadata, siteJsonLd } from '@/lib/seo'

const title = 'iii: More application. Less infrastructure.'
const description =
  'Connect functions, agents, APIs, models, browsers, and machines into one live, composable system. The convenience of a platform, without the lock-in.'

/* The apex canonical, so the homepage is never described by a query string or a mirror. */
export const metadata: Metadata = pageMetadata({ title, description })

export default function Page() {
  return (
    <>
      <Landing />
      <JsonLd data={siteJsonLd()} />
      <JsonLd data={faqJsonLd(faqs)} />
    </>
  )
}

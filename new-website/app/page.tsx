import type { Metadata } from 'next'

import { Agents } from '@/components/sections/agents'
import { Anywhere } from '@/components/sections/anywhere'
import { Coder } from '@/components/sections/coder'
import { Faq, faqs } from '@/components/sections/faq'
import { FinalCta } from '@/components/sections/final-cta'
import { GetStarted } from '@/components/sections/get-started'
import { Hero } from '@/components/sections/hero'
import { Model } from '@/components/sections/model'
import { OneFunction } from '@/components/sections/one-function'
import { Problem } from '@/components/sections/problem'
import { Registry } from '@/components/sections/registry'
import { Footer } from '@/components/site/footer'
import { JsonLd } from '@/components/site/json-ld'
import { SiteHeader } from '@/components/site/site-header'
import { faqJsonLd, pageMetadata, site, siteJsonLd } from '@/lib/seo'

/* The apex canonical, so the homepage is never described by a query string or a mirror. */
export const metadata: Metadata = pageMetadata({ title: site.title, description: site.description })

export default function Page() {
  return (
    <>
      <JsonLd data={siteJsonLd()} />
      <JsonLd data={faqJsonLd(faqs)} />
      <SiteHeader />
      <main>
        <Hero />
        <Problem />
        <Model />
        <OneFunction />
        <Coder />
        <Agents />
        <Anywhere />
        <Registry />
        <GetStarted />
        <Faq />
        <FinalCta />
      </main>
      <Footer />
    </>
  )
}

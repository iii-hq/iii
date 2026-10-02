import { Agents } from '@/components/sections/agents'
import { Anywhere } from '@/components/sections/anywhere'
import { Coder } from '@/components/sections/coder'
import { Faq } from '@/components/sections/faq'
import { FinalCta } from '@/components/sections/final-cta'
import { GetStarted } from '@/components/sections/get-started'
import { Hero } from '@/components/sections/hero'
import { Model } from '@/components/sections/model'
import { OneFunction } from '@/components/sections/one-function'
import { Problem } from '@/components/sections/problem'
import { Registry } from '@/components/sections/registry'
import { Footer } from '@/components/site/footer'
import { SiteHeader } from '@/components/site/site-header'

export default function Page() {
  return (
    <>
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

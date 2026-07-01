import { SPECS } from 'virtual:spec-manifest'
import { Prompt } from '@lib/components/schematic/Prompt'
import { Sheet } from '@lib/components/schematic/Sheet'
import { Gallery } from './Gallery'
import { SiteFooter } from './SiteFooter'
import { SiteHeader } from './SiteHeader'
import { SITE } from './site'

function Hero() {
  const live = SPECS.filter((s) => s.status !== 'draft').length
  const decks = SPECS.filter((s) => s.hasDeck).length
  const draft = SPECS.length - live

  const stats = [
    { value: String(SPECS.length), label: SPECS.length === 1 ? 'spec' : 'specs' },
    { value: String(decks), label: 'interactive' },
    ...(draft > 0 ? [{ value: String(draft), label: 'draft' }] : []),
  ]

  return (
    <section className="border-b border-rule px-4 py-16 @3xl:px-9 @3xl:py-20">
      <div className="font-mono text-[11px] uppercase tracking-[0.18em] text-ink-faint mb-6">
        <Prompt symbol="//">{SITE.heroEyebrow}</Prompt>
      </div>
      <h1 className="font-mono text-[34px] @3xl:text-[52px] font-semibold lowercase leading-[1.05] tracking-[-0.03em] text-ink max-w-[20ch]">
        {SITE.heroTitle}
      </h1>
      <p className="mt-6 font-mono text-[14px] @3xl:text-[15px] leading-[1.7] text-ink-faint max-w-[64ch]">
        {SITE.heroLead}
      </p>

      <div className="mt-10 flex flex-wrap items-stretch border border-rule w-fit">
        {stats.map((s, i) => (
          <div key={s.label} className={`px-5 py-4 ${i > 0 ? 'border-l border-rule' : ''}`}>
            <div className="font-mono text-[24px] @3xl:text-[28px] font-semibold leading-none text-ink">{s.value}</div>
            <div className="mt-1.5 font-mono text-[11px] uppercase tracking-[0.06em] text-ink-faint">{s.label}</div>
          </div>
        ))}
      </div>
    </section>
  )
}

export default function App() {
  return (
    <div className="@container min-h-screen">
      <Sheet>
        <SiteHeader />
        <main>
          <Hero />
          <section id="specs" className="px-4 py-12 @3xl:px-9 @3xl:py-16">
            <div className="font-mono text-[11px] uppercase tracking-[0.14em] text-ink-ghost mb-6">the specs</div>
            <Gallery />
          </section>
        </main>
        <SiteFooter />
      </Sheet>
    </div>
  )
}

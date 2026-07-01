import { Gallery } from '@/components/Gallery'
import { SiteFooter } from '@/components/SiteFooter'
import { SiteHeader } from '@/components/SiteHeader'
import { Prompt } from '@/components/schematic/Prompt'
import { Sheet } from '@/components/schematic/Sheet'
import { GALLERY_META, PRESENTATIONS } from '@/content/presentations'

function Hero() {
  const live = PRESENTATIONS.filter((p) => p.status !== 'draft').length
  const total = PRESENTATIONS.length
  const draft = total - live

  const stats = [
    { value: String(total), label: total === 1 ? 'presentation' : 'presentations' },
    { value: String(live), label: 'live' },
    ...(draft > 0 ? [{ value: String(draft), label: 'draft' }] : []),
  ]

  return (
    <section className="border-b border-rule px-4 py-16 @3xl:px-9 @3xl:py-20">
      <div className="font-mono text-[11px] uppercase tracking-[0.18em] text-ink-faint mb-6">
        <Prompt symbol="//">{GALLERY_META.heroEyebrow}</Prompt>
      </div>
      <h1 className="font-mono text-[34px] @3xl:text-[52px] font-semibold lowercase leading-[1.05] tracking-[-0.03em] text-ink max-w-[20ch]">
        {GALLERY_META.heroTitle}
      </h1>
      <p className="mt-6 font-mono text-[14px] @3xl:text-[15px] leading-[1.7] text-ink-faint max-w-[64ch]">
        {GALLERY_META.heroLead}
      </p>

      <div className="mt-10 flex flex-wrap items-stretch border border-rule w-fit">
        {stats.map((s, i) => (
          <div
            key={s.label}
            className={
              'px-5 py-4 ' + (i > 0 ? 'border-l border-rule' : '')
            }
          >
            <div className="font-mono text-[24px] @3xl:text-[28px] font-semibold leading-none text-ink">
              {s.value}
            </div>
            <div className="mt-1.5 font-mono text-[11px] uppercase tracking-[0.06em] text-ink-faint">
              {s.label}
            </div>
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
          <section
            id="decks"
            className="px-4 py-12 @3xl:px-9 @3xl:py-16"
          >
            <div className="font-mono text-[11px] uppercase tracking-[0.14em] text-ink-ghost mb-6">
              the decks
            </div>
            <Gallery />
          </section>
        </main>
        <SiteFooter />
      </Sheet>
    </div>
  )
}

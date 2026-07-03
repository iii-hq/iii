import { PresentationCard } from '@/components/PresentationCard'
import { PRESENTATIONS, type Presentation } from '@/content/presentations'

/** featured first, then newest date first, then title for a stable order. */
function ordered(items: Presentation[]): Presentation[] {
  return [...items].sort((a, b) => {
    if (!!a.featured !== !!b.featured) return a.featured ? -1 : 1
    if (a.date !== b.date) return a.date < b.date ? 1 : -1
    return a.title.localeCompare(b.title)
  })
}

export function Gallery() {
  const items = ordered(PRESENTATIONS)

  if (items.length === 0) {
    return (
      <div className="border border-rule bg-bg px-6 py-16 text-center font-mono text-[13px] lowercase text-ink-faint">
        no presentations yet. run{' '}
        <span className="text-ink">/presentation &lt;spec-dir&gt;</span> to add
        the first one.
      </div>
    )
  }

  return (
    <div className="grid grid-cols-1 gap-4 @3xl:grid-cols-2 @3xl:gap-5">
      {items.map((p, i) => (
        <PresentationCard key={p.slug} presentation={p} index={i} />
      ))}
    </div>
  )
}

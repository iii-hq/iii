import { SPECS } from 'virtual:spec-manifest'
import { PresentationCard } from './PresentationCard'

export function Gallery() {
  // SPECS arrives pre-sorted (featured first, newest date, then title) from
  // the manifest — the same ordering dist/index.json ships, so the landing
  // page's "latest" always matches this grid's top row.
  if (SPECS.length === 0) {
    return (
      <div className="border border-rule bg-bg px-6 py-16 text-center font-mono text-[13px] lowercase text-ink-faint">
        no specs yet. add <span className="text-ink">tech-specs/&lt;yyyy-mm-slug&gt;/README.md</span> with a frontmatter
        block to list the first one.
      </div>
    )
  }

  return (
    <div className="grid grid-cols-1 gap-4 @3xl:grid-cols-2 @3xl:gap-5">
      {SPECS.map((s, i) => (
        <PresentationCard key={s.slug} spec={s} index={i} />
      ))}
    </div>
  )
}

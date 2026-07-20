import { Funnel } from '@lib/components/diagrams/Funnel'
import { Section } from '@lib/components/Section'
import { SpecRow, SpecSheet } from '@lib/components/SpecSheet'
import { CodeBlock } from '@lib/components/schematic/CodeBlock'
import { IMPORT_MAP, REACT_PATHS, REACT_REJECT, REACT_TARGET, SHIM_ROWS } from '../content/react'

/**
 * A8 — react sharing. Five bare specifiers funnel into the SPA's own bundled
 * react; the rejected path is the classic two-react hooks failure.
 */
export function ReactSection() {
  return (
    <Section
      id="one-react"
      index="06"
      eyebrow="react sharing"
      title="five specifiers, one react instance."
      lede="two reacts break hooks and context, so injected scripts must resolve react to the console's own. a static import map in index.html routes the five shared specifiers to /vendor shims that re-export window.__III_CONSOLE__ — zero changes to the console's own bundling."
    >
      <div className="grid grid-cols-1 @4xl:grid-cols-[minmax(0,1fr)_360px] gap-6 items-start">
        <Funnel
          title="bundle-external imports, resolved at runtime"
          paths={REACT_PATHS}
          target={REACT_TARGET}
          reject={REACT_REJECT}
        />
        <CodeBlock title="web/index.html — static, before any module load">{IMPORT_MAP}</CodeBlock>
      </div>

      <div className="mt-6">
        <SpecSheet title="the shims" meta="/vendor/*, generated">
          <div className="flex flex-col">
            {SHIM_ROWS.map((row) => (
              <SpecRow key={row.name} name={row.name} type={row.type}>
                {row.desc}
              </SpecRow>
            ))}
          </div>
        </SpecSheet>
      </div>
    </Section>
  )
}

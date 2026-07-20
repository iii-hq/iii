import { Section } from '@lib/components/Section'
import { SpecRow, SpecSheet } from '@lib/components/SpecSheet'
import { Cell } from '@lib/components/schematic/Cell'
import { CodeBlock } from '@lib/components/schematic/CodeBlock'
import { CSS_AUTHORED, CSS_SHIPPED, PRESET_CELLS, STYLING_RESIDUE } from '../content/styling'

/**
 * A13 — the styling contract: authored vanilla tailwind vs shipped scoped css,
 * the generated preset, and the honest residue.
 */
export function StylingSection() {
  return (
    <Section
      id="styling"
      index="07"
      eyebrow="styling"
      title="scoped by construction."
      lede="the host mounts every injected render inside <div data-iii-ui='<worker>' style='display:contents'>; @iii-dev/console-build compiles worker css so every selector sits under that attribute. console elements are never inside the wrapper — nothing leaks in either direction."
    >
      <div className="grid grid-cols-1 @4xl:grid-cols-2 gap-4">
        <CodeBlock title="what the author writes">{CSS_AUTHORED}</CodeBlock>
        <CodeBlock title="what actually ships">{CSS_SHIPPED}</CodeBlock>
      </div>

      <div className="mt-4 grid grid-cols-1 @2xl:grid-cols-2 @5xl:grid-cols-4 gap-px bg-rule border border-rule">
        {PRESET_CELLS.map((cell) => (
          <Cell key={cell.title} title={cell.title} className="border-0" bodyClassName="text-[12.5px]">
            {cell.body}
          </Cell>
        ))}
      </div>

      <div className="mt-6">
        <SpecSheet title="the honest residue" meta="what stays not-quite-normal">
          <div className="flex flex-col">
            {STYLING_RESIDUE.map((row) => (
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

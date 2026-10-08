import { primitives } from '@/components/graphics/model-example'
import { ModelExplorer } from '@/components/graphics/model-explorer'
import { Reveal } from '@/components/site/reveal'
import { Section } from '@/components/site/section'
import { highlight } from '@/lib/highlight'

export async function Model() {
  const entries = await Promise.all(primitives.map(async ({ id, code }) => [id, await highlight(code, 'ts')] as const))
  const code = Object.fromEntries(entries) as Record<(typeof primitives)[number]['id'], string>
  return (
    // biome-ignore lint/correctness/useUniqueElementIds: This homepage section has one stable public anchor.
    <Section
      id="model"
      eyebrow="The model"
      title="Worker, function, trigger."
      lede="iii has three primitives. Every part of the system is built from them, including the parts we ship."
    >
      <Reveal delay={0.1}>
        <ModelExplorer code={code} />
      </Reveal>
    </Section>
  )
}

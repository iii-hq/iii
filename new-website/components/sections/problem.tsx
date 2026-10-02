import { ProblemLayer } from '@/components/graphics/problem-layer'
import { Reveal } from '@/components/site/reveal'
import { Section } from '@/components/site/section'

export function Problem() {
  return (
    // biome-ignore lint/correctness/useUniqueElementIds: This homepage section has one stable public anchor.
    <Section
      id="problem"
      eyebrow="The problem"
      title="Most agents work on your system from the outside."
      lede="iii removes the layer. Services, infrastructure, and agents register on the same engine, so an agent calls the functions your services call, under the same permissions, in the same trace."
    >
      <Reveal delay={0.1}>
        <ProblemLayer />
      </Reveal>
    </Section>
  )
}

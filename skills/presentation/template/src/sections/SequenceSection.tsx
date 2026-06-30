import { SequencePlayer } from '@/components/diagrams/SequencePlayer'
import { Section } from '@/components/Section'
import { SEQ_LANES, SEQ_STEPS } from '@/content/example'

/**
 * A5 — a step-through sequence. The interactive proof: play it, step it, watch
 * one request move through the system. Front-loads understanding before any
 * field-level detail.
 */
export function SequenceSection() {
  return (
    <Section
      id="turn"
      index="02"
      eyebrow="anatomy of a request"
      title="follow one request, end to end."
      lede="press play, or step through it yourself. each arrow is a real call or event — the same contracts you saw on the map, now in motion."
    >
      <SequencePlayer
        title="one request, start to finish"
        lanes={SEQ_LANES}
        steps={SEQ_STEPS}
      />
    </Section>
  )
}

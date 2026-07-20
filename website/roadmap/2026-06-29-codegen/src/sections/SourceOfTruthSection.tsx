import { SequencePlayer } from '@lib/components/diagrams/SequencePlayer'
import { Section } from '@lib/components/Section'
import { StatusPanel } from '@lib/components/schematic/StatusPanel'
import { SEQ_LANES, SEQ_STEPS } from '../content/sequence'

/**
 * A5 - the source of truth. Steps through the generation lifecycle to make the
 * load-bearing point: the schema codegen needs already exists in the engine,
 * derived from each worker's real types - so the generated code is tied to the
 * actual code, never hand-mirrored. Closes with the honest limit (live catalog
 * only).
 */
export function SourceOfTruthSection() {
  return (
    <Section
      id="discover"
      index="04"
      eyebrow="the source of truth"
      title="iii already describes every function in json schema."
      lede="each worker's macro derives a json schema straight from its real input and output types and registers it at startup; the engine hands it back through engine::*::info. codegen maps that schema to your types, so the generated code is tied to the worker's actual code: rename a field there and it shows up the next time you generate. nothing hand-mirrored, nothing guessed."
    >
      <SequencePlayer title="generate, step by step" lanes={SEQ_LANES} steps={SEQ_STEPS} />

      <div className="mt-6">
        <StatusPanel
          variant="info"
          headline="the catalog is live"
          detail="codegen sees the workers connected to the engine right now, so a glob that matches nothing is a warning, not an error. a checked-in catalog snapshot, so ci needn't boot every worker, is the main planned v2 addition; a go emitter is reserved."
        />
      </div>
    </Section>
  )
}

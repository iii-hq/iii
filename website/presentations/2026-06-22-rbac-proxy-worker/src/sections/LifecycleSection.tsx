import { SequencePlayer } from '@lib/components/diagrams/SequencePlayer'
import { Section } from '@lib/components/Section'
import { SpecRow, SpecSheet } from '@lib/components/SpecSheet'
import { SEQ_LANES, SEQ_STEPS } from '../content/flow'

/**
 * A5 — the request lifecycle. The interactive proof, early: step one connection
 * from upgrade through a gated, rewritten round-trip.
 */
export function LifecycleSection() {
  return (
    <Section
      id="lifecycle"
      index="02"
      eyebrow="request lifecycle"
      title="every frame is intercepted on its way through."
      lede="press play, or step it yourself. one upstream websocket per downstream connection, with the interceptor spliced into the middle: the console proxy pattern, plus rbac."
    >
      <SequencePlayer title="one connection, upgrade to rewritten result" lanes={SEQ_LANES} steps={SEQ_STEPS} />

      <div className="mt-6 grid grid-cols-1 @4xl:grid-cols-2 gap-4">
        <SpecSheet title="what the proxy preserves" meta="wire fidelity">
          <div className="flex flex-col">
            <SpecRow name="invocation_id" type="byte-for-byte">
              echoed through every rewrite, or the sdk pending-call map never resolves.
            </SpecRow>
            <SpecRow name="traceparent / baggage" type="forwarded">
              w3c trace context rides both frames and passes through untouched.
            </SpecRow>
            <SpecRow name="unrecognised frames" type="pass-through">
              anything the proxy cannot classify is forwarded as-is; protocol additions degrade to relay, never
              breakage.
            </SpecRow>
          </div>
        </SpecSheet>

        <SpecSheet title="on a denied call" meta="no upstream hop">
          <div className="flex flex-col">
            <SpecRow name="synthesized result" type="InvocationResult">
              the proxy answers FORBIDDEN itself; the frame never reaches the engine.
            </SpecRow>
            <SpecRow name="void actions" type="fabricated id">
              a denied void call still gets a reply with a fabricated invocation_id, matching the engine deny path
              exactly.
            </SpecRow>
            <SpecRow name="engine:: calls" type="bypass middleware">
              connection setup, channels, and the discovery overrides are never wrapped by middleware.
            </SpecRow>
          </div>
        </SpecSheet>
      </div>
    </Section>
  )
}

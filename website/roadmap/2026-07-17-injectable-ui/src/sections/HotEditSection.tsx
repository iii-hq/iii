import { SequencePlayer } from '@lib/components/diagrams/SequencePlayer'
import { Section } from '@lib/components/Section'
import { SpecRow, SpecSheet } from '@lib/components/SpecSheet'
import { SEQ_LANES, SEQ_STEPS } from '../content/hotedit'

/**
 * A5 — the interactive proof, early: one hot edit from save to hot swap,
 * the loop vite's hmr runs, rebuilt on iii primitives.
 */
export function HotEditSection() {
  return (
    <Section
      id="hot-edit"
      index="02"
      eyebrow="hot reload"
      title="one hot edit, end to end."
      lede="the dispose → re-import → re-register loop vite's hmr runtime performs, built on iii primitives instead of vite: registration is the file-change event, a console:assets push is the update signal, import(?v=hash) is the swap."
    >
      <SequencePlayer title="save to hot swap, seven frames" lanes={SEQ_LANES} steps={SEQ_STEPS} />

      <div className="mt-6 grid grid-cols-1 @4xl:grid-cols-2 gap-4">
        <SpecSheet title="hash, not counter" meta="cache busting">
          <div className="flex flex-col">
            <SpecRow name="version" type="sha256[..16]">
              the browser module map is append-only for the life of the page; nothing evicts an imported url.
            </SpecRow>
            <SpecRow name="counter failure" type="rejected">
              a counter resets when the console restarts while tabs keep their module maps — ?v=2 could alias a
              different, already-imported ?v=2 and silently serve a stale module.
            </SpecRow>
            <SpecRow name="free dedupe" type="bonus">
              re-delivered registrations with unchanged content publish nothing; replay bursts are absorbed.
            </SpecRow>
          </div>
        </SpecSheet>

        <SpecSheet title="never a full reload" meta="deliberate divergence">
          <div className="flex flex-col">
            <SpecRow name="vite" type="full-reload frame">
              when no hmr boundary accepts an update, vite reloads the page.
            </SpecRow>
            <SpecRow name="this design" type="never">
              every script is its own accepting boundary and the console shell is never hot-swapped — a full reload
              of live traces and in-flight chat is worse than one missing extension.
            </SpecRow>
            <SpecRow name="failed update" type="drop-out">
              import/setup failure: console.error + one non-fatal toast; contributions drop out until the next good
              version.
            </SpecRow>
          </div>
        </SpecSheet>
      </div>
    </Section>
  )
}

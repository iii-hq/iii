import { CliPlayground } from '@lib/components/diagrams/CliPlayground'
import { PageShell } from '@lib/components/PageShell'
import { SpecRow, SpecSheet } from '@lib/components/SpecSheet'
import { CodeBlock } from '@lib/components/schematic/CodeBlock'
import { StatusPanel } from '@lib/components/schematic/StatusPanel'
import { CLI_TRACKS, CONTENT_FN_ROWS, DEV_LOOP_NOTE, REGISTRATION_CODE } from '../content/authoring'

/**
 * A14 — the author's chair: what a worker author actually writes (one content
 * function, one trigger registration, one console-build invocation) and the
 * dev loop that gives hot reload.
 */
export function AuthoringPage() {
  return (
    <PageShell
      eyebrow="deep dive"
      title="the author's chair"
      description="three pieces ship a worker's console ui: an ordinary react component, a content function that serves bytes by path, and one trigger registration per asset. the build is @iii-dev/console-build; the wire contract is bytes, so any pipeline producing the same output is equally valid."
    >
      <CliPlayground title="golden path and dev loop" tracks={CLI_TRACKS} />

      <div className="grid grid-cols-1 @4xl:grid-cols-2 gap-4 items-start">
        <CodeBlock title="registration — node sdk shown, wire is sdk-agnostic">{REGISTRATION_CODE}</CodeBlock>

        <div className="flex flex-col gap-4">
          <SpecSheet title="the content function" meta="<worker>::ui-content" defaultOpen>
            <div className="flex flex-col">
              {CONTENT_FN_ROWS.map((row) => (
                <SpecRow key={row.name} name={row.name} type={row.type}>
                  {row.desc}
                </SpecRow>
              ))}
            </div>
          </SpecSheet>

          <StatusPanel variant="info" headline="who registers, and why it matters" detail={DEV_LOOP_NOTE} />
        </div>
      </div>
    </PageShell>
  )
}

import { type SeqLane, type SeqStep, SequencePlayer } from '@lib/components/diagrams/SequencePlayer'
import { SpecRow, SpecSheet } from '@lib/components/SpecSheet'
import { Cell } from '@lib/components/schematic/Cell'
import { FnChip } from '@lib/components/schematic/FnChip'
import { UseCaseShell } from './UseCaseShell'

const LANES: SeqLane[] = [
  { id: 'console', label: 'console', x: 100 },
  { id: 'harness', label: 'harness', x: 340 },
  { id: 'session', label: 'session-manager', x: 580 },
  { id: 'gate', label: 'approval-gate', x: 820 },
]

const STEPS: SeqStep[] = [
  {
    from: 'console',
    to: 'harness',
    label: 'harness::send { metadata: { owner } }',
    title: 'send, with tenancy riding along',
    desc: 'the composer sends through the harness and stamps the session with owner metadata — the same field the console later filters its event bindings and session lists by. multi-tenant by convention, not by special casing.',
  },
  {
    from: 'harness',
    to: 'session',
    label: 'session::update-message',
    title: 'the transcript streams',
    desc: 'deltas persist as they arrive; the console renders snapshots last-write-wins by revision. thinking content renders as a collapsible block, function calls as live cards.',
    event: 'session::message-updated',
  },
  {
    from: 'harness',
    to: 'gate',
    label: 'pre_trigger → hold',
    title: 'a risky call pauses',
    desc: 'the model asked for something the policy routes to a human. the call parks, and the pending record fires an event the console inbox is bound to.',
    event: 'approval::pending_created',
  },
  {
    from: 'console',
    to: 'gate',
    label: 'approval::resolve',
    title: 'approve / deny — inline in the chat',
    desc: 'the held call renders as a card with the arguments, right where the conversation is. one click resolves it; the released call executes through the normal trigger pipeline.',
  },
  {
    from: 'harness',
    to: 'session',
    label: 'session::set-status done',
    title: 'the turn closes, fully traceable',
    desc: 'the spinner stops on the status event, and the session id deep-links into the trace explorer — every hop of the turn (send, assemble, generate, dispatch) is a span you can open.',
    event: 'harness::turn_completed',
  },
]

export function ConsolePage() {
  return (
    <UseCaseShell
      eyebrow="web app"
      title="the console chat — an operator's cockpit."
      description="the same triangle as every consumer — send, render reactively, observe boundaries — plus the operator extras: inline approvals, live catalogs, and a one-click jump from any conversation into its full trace."
    >
      <SequencePlayer title="console ↔ the stack — one governed turn" lanes={LANES} steps={STEPS} width={940} />

      <div className="grid grid-cols-1 @2xl:grid-cols-3 gap-4">
        <Cell title="live catalogs" bodyClassName="max-w-none">
          the model picker reads the router's capability catalog; the composer's @-mentions search every function
          registered on the engine — both update as workers come and go. the ui never ships a hardcoded list.
        </Cell>
        <Cell title="approvals where you read" bodyClassName="max-w-none">
          held calls appear as cards in the transcript with their arguments, grouped when consecutive. approve and deny
          are one click — the parked turn resumes instantly.
        </Cell>
        <Cell title="context, visible" bodyClassName="max-w-none">
          a usage meter tracks the window with warn and danger thresholds and nudges compaction — the same budgeting the
          loop runs is legible to the operator.
        </Cell>
      </div>

      <div className="grid grid-cols-1 @4xl:grid-cols-2 gap-4">
        <SpecSheet title="what the console binds" meta="reactive surface" defaultOpen>
          <div className="flex flex-col">
            <SpecRow name="session::message-added / message-updated" type="render">
              the transcript paints from events, reconciled by revision — joining mid-stream just works.
            </SpecRow>
            <SpecRow name="session::status-changed" type="spinner">
              working / done / error drive the session list and the composer state.
            </SpecRow>
            <SpecRow name="approval::pending_created / resolved" type="inbox">
              held calls arrive and clear without any refresh logic.
            </SpecRow>
            <SpecRow name="harness::turn_completed" type="outcomes">
              toasts on failures, auto-titling after first turns, anything that reacts to results rather than text.
            </SpecRow>
          </div>
        </SpecSheet>

        <SpecSheet title="observability — every hop is a span" meta="opentelemetry">
          <div className="flex flex-col">
            <SpecRow name="harness.turn" type="per step">
              session, turn, and step attributes on every loop iteration.
            </SpecRow>
            <SpecRow name="router.chat" type="per stream">
              provider, model, token usage, cost.
            </SpecRow>
            <SpecRow name="context.assemble" type="per turn">
              tokens before and after; pruned / compacted flags.
            </SpecRow>
            <SpecRow name="session.append / update" type="per write">
              entry id and revision — the write path is visible too.
            </SpecRow>
            <SpecRow name="metadata propagates" type="end-to-end">
              consumer-supplied tracing metadata rides through every nested call, so one conversation stitches into one
              trace.
            </SpecRow>
          </div>
        </SpecSheet>
      </div>

      <div className="border border-rule bg-bg px-4 py-3.5 flex flex-wrap items-center gap-x-3 gap-y-2">
        <span className="font-mono text-[11px] uppercase tracking-[0.14em] text-ink-faint">composition note</span>
        <span className="font-mono text-[12.5px] text-ink-faint lowercase">
          the console adds zero privileged surface — it speaks the same
        </span>
        <FnChip tone="faint">harness::send</FnChip>
        <FnChip tone="faint">session::* events</FnChip>
        <FnChip tone="faint">approval::resolve</FnChip>
        <span className="font-mono text-[12.5px] text-ink-faint lowercase">any of your apps would.</span>
      </div>
    </UseCaseShell>
  )
}

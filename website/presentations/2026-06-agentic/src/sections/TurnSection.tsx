import { type SeqLane, type SeqStep, SequencePlayer } from '@lib/components/diagrams/SequencePlayer'
import { Section } from '@lib/components/Section'
import { SpecRow, SpecSheet } from '@lib/components/SpecSheet'

const LANES: SeqLane[] = [
  { id: 'consumer', label: 'consumer', x: 80 },
  { id: 'harness', label: 'harness', x: 280 },
  { id: 'session', label: 'session-manager', x: 480 },
  { id: 'context', label: 'context-manager', x: 650 },
  { id: 'router', label: 'llm-router', x: 800 },
  { id: 'functions', label: 'functions', x: 920 },
]

const STEPS: SeqStep[] = [
  {
    from: 'consumer',
    to: 'harness',
    label: 'harness::send',
    title: 'a message arrives',
    desc: 'any consumer drops a message and gets { session_id, turn_id } back immediately. the harness ensures the session, persists the user entry, and seeds the turn record — one turn per session, enforced atomically. a second send while the turn runs merges in as steering, never a competing turn.',
  },
  {
    from: 'harness',
    to: 'harness',
    label: 'enqueue turn step',
    title: 'durable execution begins',
    desc: 'the loop runs as enqueued steps on a durable queue — ordered per session, parallel across sessions. a crash or restart resumes the turn mid-flight instead of starting over, because every step tolerates redelivery.',
  },
  {
    from: 'harness',
    to: 'session',
    label: 'session::set-status working',
    title: 'the session goes live',
    desc: 'status flips to working and the turn-started event fires. every bound consumer shows a spinner without asking anyone — the write itself is the notification.',
    event: 'harness::turn_started',
  },
  {
    from: 'harness',
    to: 'session',
    label: 'session::messages',
    title: 'load the active path',
    desc: 'the loop reads the transcript, including the latest compaction record — so old history that was already summarised is never re-paid for, turn after turn.',
  },
  {
    from: 'harness',
    to: 'context',
    label: 'context::assemble',
    title: 'fit the window',
    desc: 'raw history plus the target model in; a system prompt and budgeted messages out. verbose function output pruned, the head compacted when it overflows, headroom reserved — all tuned to the exact model\u2019s limits.',
  },
  {
    from: 'harness',
    to: 'router',
    label: 'router::chat',
    title: 'generate',
    desc: 'one front door for every provider. the completion streams back frame by frame over a channel, with usage and cost attached — and the request id is recorded so a stop can abort it mid-flight.',
  },
  {
    from: 'harness',
    to: 'session',
    label: 'session::update-message',
    title: 'stream to everyone',
    desc: 'every delta is persisted as it arrives, and every write emits an event. the chat ui, the telegram bridge, and any dashboard all render the same write, live — one stream in, any number of surfaces out.',
    event: 'session::message-updated',
  },
  {
    from: 'harness',
    to: 'functions',
    label: 'agent_trigger → shell::exec',
    title: 'the model asks for work',
    desc: 'function calls dispatch through the fail-closed allow-list and the hook chain, with a checkpoint per call. results land in the transcript as function_result entries — at-most-once side effects, even across a crash.',
  },
  {
    from: 'harness',
    to: 'harness',
    label: 're-enqueue turn step',
    title: 'the loop continues',
    desc: 'with results in the transcript, another generate step is enqueued so the model can react. the steering check also folds in any user message that arrived mid-turn — context, not interruption.',
  },
  {
    from: 'harness',
    to: 'session',
    label: 'session::set-status done',
    title: 'the turn completes',
    desc: 'the turn-completed event carries the terminal status and the typed result — the orchestration surface for anything that reacts to outcomes: chain a follow-up, notify a channel, or settle a parent agent.',
    event: 'harness::turn_completed',
  },
]

export function TurnSection() {
  return (
    <Section
      id="turn"
      index="03"
      eyebrow="anatomy of a turn"
      title="one turn, end to end — every hop is a bus call you can watch."
      lede="press play. this is the exact path a message takes through the stack: persist, assemble, generate, dispatch, repeat. nothing is hidden in a process boundary — every hop is a named function with a spec."
    >
      <SequencePlayer title="harness::turn — the loop" lanes={LANES} steps={STEPS} width={990} />

      <div className="mt-6 grid grid-cols-1 @4xl:grid-cols-2 gap-4">
        <SpecSheet title="steering — messages mid-turn" meta="concurrency">
          <div className="flex flex-col">
            <SpecRow name="one turn per session" type="atomic check-and-set">
              two concurrent sends create exactly one turn — the loser of the race merges instead of forking.
            </SpecRow>
            <SpecRow name="merged sends fold in" type="watermark">
              a message that lands mid-turn is appended and picked up by the running loop's steering check — the model
              sees it in the same turn, like a colleague adding context.
            </SpecRow>
            <SpecRow name="never silently dropped" type="double-check">
              the merge path re-reads the turn record after appending; if the turn just ended, a fresh turn starts for
              the message.
            </SpecRow>
          </div>
        </SpecSheet>

        <SpecSheet title="the streaming contract" meta="typed frames">
          <div className="flex flex-col">
            <SpecRow name="AssistantMessageEvent" type="discriminated union">
              start, text_delta, thinking_delta, functioncall_delta, usage, stop, done, error — providers stream one
              shape, every consumer reads one shape.
            </SpecRow>
            <SpecRow name="partial accumulator" type="on every frame">
              non-terminal frames carry the assembled message so far — a ui can render from any frame it joins on.
            </SpecRow>
            <SpecRow name="revision numbers" type="last-write-wins">
              events echo a revision per entry; consumers reconcile snapshots in any arrival order.
            </SpecRow>
            <SpecRow name="usage + cost" type="per stream">
              token splits (including cache reads) and usd cost are attached by the router from its catalog pricing.
            </SpecRow>
          </div>
        </SpecSheet>
      </div>
    </Section>
  )
}

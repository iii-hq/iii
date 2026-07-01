import { type SeqLane, type SeqStep, SequencePlayer } from '@lib/components/diagrams/SequencePlayer'
import { SpecRow, SpecSheet } from '@lib/components/SpecSheet'
import { C, CodeBlock, K, M, S } from '@lib/components/schematic/CodeBlock'
import { StatusPanel } from '@lib/components/schematic/StatusPanel'
import { UseCaseShell } from './UseCaseShell'

const LANES: SeqLane[] = [
  { id: 'source', label: 'event source', x: 90 },
  { id: 'fn', label: 'reports::daily', x: 300 },
  { id: 'harness', label: 'harness', x: 520 },
  { id: 'registry', label: 'functions', x: 730 },
  { id: 'out', label: 'downstream', x: 900 },
]

const STEPS: SeqStep[] = [
  {
    from: 'source',
    to: 'fn',
    label: 'cron 09:00 fires',
    title: 'any event can carry a goal',
    desc: 'a schedule here — but the binding works the same for a webhook, a state change, a stream item, or another agent finishing. reactive wiring: trigger type in, function out.',
  },
  {
    from: 'fn',
    to: 'harness',
    label: 'harness::run { output: json + schema }',
    title: 'call an agent like a function',
    desc: 'run holds the call open until the turn ends and returns a typed result. the output contract turns "parse the transcript yourself" into a schema-validated json value.',
  },
  {
    from: 'harness',
    to: 'registry',
    label: 'agent_trigger → database::query, search::web',
    title: 'the agent works the ecosystem',
    desc: 'within its allow-list, the agent reaches whatever the job needs — queries, lookups, file reads. capability is the registry itself, gated by policy per run.',
  },
  {
    from: 'harness',
    to: 'harness',
    label: 'harness::spawn × 2',
    title: 'it delegates on its own',
    desc: 'the goal is big? the agent fans out sub-agents — an analyst per data source — under the same budgets and a narrowed policy, joins their typed results, and keeps going.',
  },
  {
    from: 'harness',
    to: 'fn',
    label: 'result: { metrics, anomalies, summary }',
    title: 'a typed deliverable comes back',
    desc: 'validated against the schema before the turn completes — with bounded retry nudges if the model misses it. the calling function gets data, not prose to regex.',
    event: 'harness::turn_completed',
  },
  {
    from: 'fn',
    to: 'out',
    label: 'email::send · state::set',
    title: 'the result fans out — and can chain',
    desc: 'the handler emails the report and writes state. every one of those writes is itself an event other functions can bind — the next loop starts where this one ends, each chain carrying its own termination condition.',
  },
]

export function LoopsPage() {
  return (
    <UseCaseShell
      eyebrow="autonomous backend"
      title="agentic loops — bind any event to a goal."
      description="no consumer, no chat window, no one watching: an event fires, a function frames a goal, an agent pursues it with real capabilities and returns a typed result. this is the pattern that turns the stack from a chat backend into an autonomous one."
    >
      <SequencePlayer
        title="schedule → goal → typed result → next actions"
        lanes={LANES}
        steps={STEPS}
        width={1000}
        intervalMs={3000}
      />

      <div className="grid grid-cols-1 @4xl:grid-cols-2 gap-4 items-start">
        <CodeBlock title="the entire loop — one worker file">
          iii.<K>registerFunction</K>(<S>"reports::daily"</S>, <K>async</K> () <M>{'=>'}</M> <M>{'{'}</M>
          {'\n'}
          {'  '}
          <K>const</K> run = <K>await</K> iii.<K>trigger</K>(<M>{'{'}</M>
          {'\n'}
          {'    '}function_id: <S>"harness::run"</S>,{'\n'}
          {'    '}payload: <M>{'{'}</M>
          {'\n'}
          {'      '}message: <S>"compile yesterday's ops report"</S>,{'\n'}
          {'      '}model: <S>"claude-sonnet-4"</S>,{'\n'}
          {'      '}options: <M>{'{'}</M>
          {'\n'}
          {'        '}output: <M>{'{'}</M> type: <S>"json"</S>, schema: ReportSchema <M>{'}'}</M>,{'\n'}
          {'        '}functions: <M>{'{'}</M>
          {'\n'}
          {'          '}allow: [<S>"database::*"</S>, <S>"search::web"</S>, <S>"harness::spawn"</S>]{'\n'}
          {'        '}
          <M>{'}'}</M>
          {'\n'}
          {'      '}
          <M>{'}'}</M>
          {'\n'}
          {'    '}
          <M>{'}'}</M>
          {'\n'}
          {'  '}
          <M>{'}'}</M>);
          {'\n\n'}
          {'  '}
          <K>await</K> iii.<K>trigger</K>(<M>{'{'}</M> function_id: <S>"email::send"</S>, payload: render(run.result){' '}
          <M>{'}'}</M>);
          {'\n'}
          <M>{'}'}</M>);
          {'\n\n'}
          <C>// reactive: the schedule is just a trigger type</C>
          {'\n'}
          iii.<K>registerTrigger</K>(<M>{'{'}</M>
          {'\n'}
          {'  '}type: <S>"cron"</S>,{'\n'}
          {'  '}function_id: <S>"reports::daily"</S>,{'\n'}
          {'  '}config: <M>{'{'}</M> expression: <S>"0 0 9 * * *"</S> <M>{'}'}</M>,{'\n'}
          <M>{'}'}</M>);
        </CodeBlock>

        <div className="flex flex-col gap-4">
          <SpecSheet title="the output contract" meta="typed deliverables" defaultOpen>
            <div className="flex flex-col">
              <SpecRow name='{ type: "text" }' type="default">
                the result is the final message text — fine for chat.
              </SpecRow>
              <SpecRow name='{ type: "json", schema }' type="automation grade">
                the turn must produce json matching the schema. delivered provider-natively when the model supports
                structured output, or through a synthetic submit_result call when it does not — callers never see the
                difference.
              </SpecRow>
              <SpecRow name="validation retries" type="bounded">
                a miss appends the validation errors as a nudge and regenerates — a bounded number of times, then an
                explicit result_error.
              </SpecRow>
            </div>
          </SpecSheet>

          <SpecSheet title="chains that know when to stop" meta="turn_completed">
            <div className="flex flex-col">
              <SpecRow name="event-driven chains" type="completed → send → …">
                bind harness::turn_completed and start the next turn from the handler — the supported way to build
                multi-stage pipelines.
              </SpecRow>
              <SpecRow name="termination is yours" type="by design">
                max_turns bounds one turn, not a chain. carry a hop counter in session metadata or a budget sibling —
                the spec is explicit about who owns the brake.
              </SpecRow>
            </div>
          </SpecSheet>

          <StatusPanel
            variant="success"
            headline="goal in, data out"
            detail="upstream systems never learn what 'an agent' is — they call a function and receive json. the intelligence is an implementation detail."
          />
        </div>
      </div>
    </UseCaseShell>
  )
}

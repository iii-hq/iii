import { type SeqLane, type SeqStep, SequencePlayer } from '@lib/components/diagrams/SequencePlayer'
import { SpecRow, SpecSheet } from '@lib/components/SpecSheet'
import { Cell } from '@lib/components/schematic/Cell'
import { C, CodeBlock, K, M, S } from '@lib/components/schematic/CodeBlock'
import { UseCaseShell } from './UseCaseShell'

const LANES: SeqLane[] = [
  { id: 'user', label: 'telegram user', x: 90 },
  { id: 'tg', label: 'telegram-bot', x: 300 },
  { id: 'harness', label: 'harness', x: 520 },
  { id: 'session', label: 'session-manager', x: 720 },
  { id: 'router', label: 'llm-router', x: 900 },
]

const STEPS: SeqStep[] = [
  {
    from: 'user',
    to: 'tg',
    label: '/start',
    title: 'the webhook lands',
    desc: 'the worker receives the update and replies with an inline keyboard built from the live model catalog — no harness call yet. the chosen model is written into the session metadata and enforced by the worker for the whole conversation.',
  },
  {
    from: 'tg',
    to: 'router',
    label: 'router::models::list',
    title: 'a live model picker',
    desc: 'the catalog is populated by provider discovery, so the keyboard always offers what the deployment actually has — provider-grouped, capability-filtered, current.',
  },
  {
    from: 'user',
    to: 'tg',
    label: 'a question arrives',
    title: 'ordinary text, extraordinary plumbing',
    desc: 'telegram redelivers webhooks. the worker does not care: every send carries an idempotency key derived from the update id.',
  },
  {
    from: 'tg',
    to: 'harness',
    label: 'harness::send { idempotency_key }',
    title: 'deduped at the front door',
    desc: 'a redelivered update returns the original { session_id, turn_id } with deduplicated: true and appends nothing. chat ↔ session mapping rides in session.metadata; the worker keeps a small kv for the active session per chat.',
  },
  {
    from: 'harness',
    to: 'session',
    label: 'session::update-message',
    title: 'the agent streams into the store',
    desc: 'the loop persists deltas as they arrive — and each write emits the event the worker is already bound to.',
    event: 'session::message-updated',
  },
  {
    from: 'session',
    to: 'tg',
    label: 'event → editMessageText',
    title: 'live edits, reactively',
    desc: 'the bound handler edits the telegram message in place, throttled to ~1/s and reconciled by revision so out-of-order deliveries can never paint stale text. the user watches the answer grow — like any modern ai chat, inside telegram.',
  },
  {
    from: 'user',
    to: 'tg',
    label: '/stop',
    title: 'the user changes their mind',
    desc: 'commands are intercepted by the worker — /stop is an instruction to the stack, never a user message for the model.',
  },
  {
    from: 'tg',
    to: 'harness',
    label: 'harness::stop',
    title: 'a stop that actually stops',
    desc: 'the in-flight stream aborts mid-generation, the partial message finalises with an explicit aborted marker, spawned children cascade-stop, and the session lands at done. the next message starts clean.',
    event: 'harness::turn_completed',
  },
]

export function TelegramPage() {
  return (
    <UseCaseShell
      eyebrow="messaging surface"
      title="a telegram bot in one small worker."
      description="the bridge owns telegram ux — commands, inline keyboards, message edits — and delegates everything hard to the stack: streaming, durability, model routing, cancellation. the whole integration is three function calls and two reactive bindings."
    >
      <SequencePlayer title="telegram ↔ the stack — one conversation" lanes={LANES} steps={STEPS} width={1000} />

      <div className="grid grid-cols-1 @4xl:grid-cols-2 gap-4 items-start">
        <CodeBlock title="the whole reactive render path">
          <C>// bind once at startup — then every assistant delta</C>
          {'\n'}
          <C>// in any session this worker cares about flows in.</C>
          {'\n'}
          iii.<K>registerFunction</K>(<S>"tg::on-message-updated"</S>, <K>async</K> (evt) <M>{'=>'}</M> <M>{'{'}</M>
          {'\n'}
          {'  '}
          <K>const</K> chatId = <K>await</K> chatIdForSession(evt.session_id);
          {'\n'}
          {'  '}
          <K>if</K> (!shouldEdit(evt.revision)) <K>return</K>; <C>// ~1/s throttle</C>
          {'\n'}
          {'  '}
          <K>await</K> telegram.editMessageText(chatId, msgId(evt), text(evt));
          {'\n'}
          <M>{'}'}</M>);
          {'\n\n'}
          iii.<K>registerTrigger</K>(<M>{'{'}</M>
          {'\n'}
          {'  '}type: <S>"session::message-updated"</S>,{'\n'}
          {'  '}function_id: <S>"tg::on-message-updated"</S>,{'\n'}
          {'  '}config: <M>{'{'}</M> roles: [<S>"assistant"</S>] <M>{'}'}</M>,{'\n'}
          <M>{'}'}</M>);
        </CodeBlock>

        <div className="flex flex-col gap-4">
          <Cell title="what the worker owns" bodyClassName="max-w-none">
            telegram ux only: the command fsm, the inline keyboard, the chat ↔ session map, edit throttling. roughly a
            hundred lines — everything else is the stack's job.
          </Cell>
          <Cell title="what it never reimplements" bodyClassName="max-w-none">
            streaming, retries, context budgeting, provider routing, crash recovery, cancellation semantics, model
            catalogs. five workers already solved those — for every consumer at once.
          </Cell>
        </div>
      </div>

      <div className="grid grid-cols-1 @4xl:grid-cols-2 gap-4">
        <SpecSheet title="worker surface — functions & bindings" meta="2 + 4">
          <div className="flex flex-col">
            <SpecRow name="telegram::webhook" type="function">
              receives updates; routes commands and messages.
            </SpecRow>
            <SpecRow name="telegram::model_selected" type="function">
              inline-keyboard callback — locks the model, creates the session.
            </SpecRow>
            <SpecRow name="session::message-added → tg::on-message-added" type="binding">
              creates the telegram message for each new assistant entry.
            </SpecRow>
            <SpecRow name="session::message-updated → tg::on-message-updated" type="binding">
              streams edits, throttled, last-write-wins by revision.
            </SpecRow>
            <SpecRow name="session::status-changed → tg::on-status-changed" type="binding">
              typing indicator while working; ready marker on done.
            </SpecRow>
            <SpecRow name="harness::turn_completed → tg::on-turn-completed" type="binding">
              drains the local queue, or posts a single final message when live edits are not wanted.
            </SpecRow>
          </div>
        </SpecSheet>

        <div className="flex flex-col gap-4">
          <SpecSheet title="steering vs strict fifo" meta="a product choice">
            <div className="flex flex-col">
              <SpecRow name="steering (default)" type="harness-native">
                messages sent mid-turn fold into the running turn — the model treats them as added context. zero extra
                worker code.
              </SpecRow>
              <SpecRow name="strict fifo" type="worker-local queue">
                one question, one answer: the worker checks harness::status and queues locally, draining on
                turn_completed. the stack supports both; the product decides.
              </SpecRow>
            </div>
          </SpecSheet>

          <SpecSheet title="worker state — a tiny kv" meta="5 keys">
            <div className="flex flex-col">
              <SpecRow name="tg:chat:{id}:session" type="session_id">
                the active session for a chat.
              </SpecRow>
              <SpecRow name="tg:chat:{id}:fsm" type="idle | awaiting_model">
                the /start model-picker state.
              </SpecRow>
              <SpecRow name="tg:session:{id}:chat" type="chat_id">
                reverse lookup for the event handlers.
              </SpecRow>
              <SpecRow name="tg:entry:{session}:{entry}:msg" type="message_id">
                assistant entry → telegram message, for edits.
              </SpecRow>
              <SpecRow name="model" type="session.metadata">
                authoritative in the session store; kv only caches it.
              </SpecRow>
            </div>
          </SpecSheet>
        </div>
      </div>
    </UseCaseShell>
  )
}

import { EventFanOut } from '@lib/components/diagrams/EventFanOut'
import { Section } from '@lib/components/Section'
import { SpecRow, SpecSheet } from '@lib/components/SpecSheet'
import { C, CodeBlock, K, M, S } from '@lib/components/schematic/CodeBlock'
import { FnChip } from '@lib/components/schematic/FnChip'
import { FANOUT_EDGES, FANOUT_HANDLERS } from '../content/fanout'

const EVENT_SOURCES = [
  'http',
  'cron',
  'state',
  'stream',
  'subscribe',
  'durable queue',
  'session::message-added',
  'session::status-changed',
  'harness::turn_completed',
  'approval::pending_created',
] as const

export function ReactiveSection() {
  return (
    <Section
      id="reactive"
      index="05"
      eyebrow="reactive by design"
      title="every mutation emits an event. bind a function once — render live everywhere."
      lede="the stack pushes; consumers never chase. a single write into the session store reaches the chat ui, the telegram bridge, and any dashboard in the same instant — because reactivity is the substrate's native shape: trigger types bound to functions."
    >
      <EventFanOut
        heading="one write, every surface"
        headingNote="ambient — always on"
        source="session::update-message"
        sourceSub="revision 7 — one write, no publish step"
        trigger="session::message-updated"
        handlers={FANOUT_HANDLERS}
        edges={FANOUT_EDGES}
        footnote="bound functions — subscribe once, render live"
        ariaLabel="an event fanning out from one write to three bound functions"
      />

      <div className="mt-6 grid grid-cols-1 @4xl:grid-cols-2 gap-4 items-start">
        <CodeBlock title="the whole reactive pattern — two calls">
          <C>// 1. register the function</C>
          {'\n'}
          iii.<K>registerFunction</K>(<S>"tg::on-message-updated"</S>, handler);
          {'\n\n'}
          <C>// 2. bind it to a trigger type</C>
          {'\n'}
          iii.<K>registerTrigger</K>(<M>{'{'}</M>
          {'\n'}
          {'  '}type: <S>"session::message-updated"</S>,{'\n'}
          {'  '}function_id: <S>"tg::on-message-updated"</S>,{'\n'}
          {'  '}config: <M>{'{'}</M> roles: [<S>"assistant"</S>] <M>{'}'}</M>,{'\n'}
          <M>{'}'}</M>);
          {'\n\n'}
          <C>// that's it. no publish step, no broker to run —</C>
          {'\n'}
          <C>// the emitting worker filters and delivers.</C>
        </CodeBlock>

        <div className="flex flex-col gap-4 min-w-0">
          <div className="border border-rule bg-bg">
            <div className="bg-panel px-3.5 py-2 border-b border-rule font-mono text-[11px] font-medium uppercase tracking-[0.06em] text-ink-faint">
              anything can start work
            </div>
            <div className="px-4 py-4">
              <div className="flex flex-wrap gap-2">
                {EVENT_SOURCES.map((src) => (
                  <FnChip key={src} tone="faint">
                    {src}
                  </FnChip>
                ))}
                <FnChip tone="ghost">any custom type a worker registers</FnChip>
              </div>
              <p className="mt-4 font-mono text-[12px] leading-[1.65] text-ink-faint lowercase max-w-[58ch]">
                every one of these is a trigger type a function can be bound to — and that function can call{' '}
                <span className="text-ink">harness::run</span> with a goal. a webhook, a schedule, a state change, or
                another agent finishing: any event in the system can set an agent in motion toward an outcome.
              </p>
            </div>
          </div>

          <SpecSheet title="events vs hooks — know or act" meta="two surfaces">
            <div className="flex flex-col">
              <SpecRow name="need to know? bind an event" type="async, observe-only">
                turn_started / turn_completed and the six session events fire at the boundaries — cheap, unordered,
                at-least-once; reconciled by revision.
              </SpecRow>
              <SpecRow name="must block or change? bind a hook" type="sync, in-path">
                five points — pre_turn, pre_generate, post_generate, pre_trigger, post_trigger — run inside the loop in
                deterministic order and can veto, hold, or rewrite.
              </SpecRow>
              <SpecRow name="hooks are operator code" type="never model-reachable">
                bindings are a worker-plane surface with per-binding timeout and an explicit failure policy — a dead
                hook can never brick the loop silently.
              </SpecRow>
            </div>
          </SpecSheet>
        </div>
      </div>
    </Section>
  )
}

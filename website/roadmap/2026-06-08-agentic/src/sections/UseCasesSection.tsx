import { Section } from '@lib/components/Section'
import { FnChip } from '@lib/components/schematic/FnChip'

const CASES = [
  {
    id: 'telegram',
    href: '#/telegram',
    name: 'telegram bot',
    blurb:
      'a webhook bridge in one worker: live message edits while the agent streams, idempotent redeliveries, /stop that actually stops.',
    chips: ['harness::send', 'session::message-updated', 'harness::stop'],
    foot: 'messaging surface',
  },
  {
    id: 'console',
    href: '#/console',
    name: 'console chat',
    blurb:
      'the operator cockpit: streaming transcripts, approve/deny inline, model picker from the live catalog, every turn traceable end to end.',
    chips: ['session::* events', 'approval::resolve', 'router::models::list'],
    foot: 'web app',
  },
  {
    id: 'loops',
    href: '#/loops',
    name: 'agentic loops',
    blurb:
      'bind a schedule, a webhook, or a state change to a goal: an agent runs with a typed output contract, and its result fans the next actions out.',
    chips: ['cron / state / http', 'harness::run', 'output: json + schema'],
    foot: 'autonomous backend',
  },
] as const

export function UseCasesSection() {
  return (
    <Section
      id="use-cases"
      index="09"
      eyebrow="use cases"
      title="three consumers, one contract — send, render, observe."
      lede="every consumer is the same triangle: send through the harness, render reactively from session events, observe turn boundaries. each walkthrough below is the full wiring for one surface, sequence diagram included."
    >
      <div className="grid grid-cols-1 @3xl:grid-cols-3 gap-4">
        {CASES.map((useCase) => (
          <a
            key={useCase.id}
            href={useCase.href}
            className="group border border-rule bg-bg flex flex-col transition-colors hover:bg-panel hover:border-l-2 hover:border-l-accent"
          >
            <header className="flex items-center justify-between px-4 py-3 border-b border-rule-2">
              <span className="font-mono text-[16px] font-semibold lowercase text-ink">{useCase.name}</span>
              <span className="font-mono text-[12px] text-ink-ghost transition-transform group-hover:translate-x-0.5 group-hover:text-accent">
                →
              </span>
            </header>
            <p className="px-4 py-3.5 font-mono text-[12.5px] leading-[1.7] text-ink-faint lowercase">
              {useCase.blurb}
            </p>
            <div className="px-4 pb-4 mt-auto flex flex-wrap gap-1.5">
              {useCase.chips.map((chip) => (
                <FnChip key={chip} tone="faint">
                  {chip}
                </FnChip>
              ))}
            </div>
            <footer className="flex items-center justify-between px-4 py-2 border-t border-rule-2">
              <span className="font-mono text-[10px] uppercase tracking-[0.06em] text-ink-faint">{useCase.foot}</span>
              <span className="font-mono text-[10px] uppercase tracking-[0.06em] text-ink-ghost">walkthrough</span>
            </footer>
          </a>
        ))}
      </div>
    </Section>
  )
}

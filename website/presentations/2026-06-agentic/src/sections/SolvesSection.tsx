import { Section } from '@lib/components/Section'

const ROWS = [
  {
    problem: 'provider lock-in',
    problemDetail: 'call sites welded to one vendor sdk; every new model is a migration.',
    answer: 'one front door',
    answerDetail:
      'router::chat is the only call consumers know. providers are workers behind a narrow protocol — add or swap them with zero call-site changes, and the capability catalog updates itself.',
  },
  {
    problem: 'fragile in-process loops',
    problemDetail: 'an agent dies with its process; restarts replay side effects or lose answers.',
    answer: 'durable execution',
    answerDetail:
      'turns are queued, replay-safe steps with deterministic writes and per-call checkpoints. crashes resume mid-stream without duplicating a message; side effects stay at-most-once.',
  },
  {
    problem: 'opaque agent state',
    problemDetail: 'what is it doing? consumers guess, scrape logs, or chase status endpoints.',
    answer: 'a reactive surface',
    answerDetail:
      'every mutation emits a typed event — message added, updated, status changed, turn completed. any ui, bridge, or auditor binds once and sees everything, live.',
  },
  {
    problem: 'unbounded agents',
    problemDetail: 'a capable model with implicit access is an incident report waiting to be written.',
    answer: 'fail-closed policy',
    answerDetail:
      'deny-all defaults, glob allow-lists per turn, provenance that survives nested calls, human holds on the risky ones, and budgets on every spawn tree.',
  },
  {
    problem: 'the harness monolith',
    problemDetail: 'one process that owns storage, context, providers, approvals — and every change risks all of it.',
    answer: 'five standalone workers',
    answerDetail:
      'each one installable and useful alone; the loop stays thin because every concern that grows logic becomes a sibling. evolve, scale, and replace pieces independently.',
  },
  {
    problem: 'the capability integration tax',
    problemDetail: 'every new ability an agent needs becomes a bespoke adapter to write and maintain.',
    answer: 'the substrate is the catalog',
    answerDetail:
      'anything any worker registers on the bus is immediately reachable — discovered at runtime, gated by policy. a new worker connects, and every allowed agent can use it. sub-agents included: spawning one is just another function.',
  },
] as const

export function SolvesSection() {
  return (
    <Section
      id="solves"
      index="11"
      eyebrow="why it holds"
      title="the hard problems of agent backends, answered structurally."
      lede="not patches around a loop — properties of the architecture. each row is a failure mode this design makes hard to write, not just easy to avoid."
    >
      <div className="border border-rule">
        {/* head row */}
        <div className="hidden @3xl:grid grid-cols-2 bg-panel border-b border-rule">
          <div className="px-5 py-2.5 font-mono text-[11px] font-medium uppercase tracking-[0.14em] text-ink-faint border-r border-rule">
            the usual failure
          </div>
          <div className="px-5 py-2.5 font-mono text-[11px] font-medium uppercase tracking-[0.14em] text-ink-faint">
            this architecture
          </div>
        </div>
        {ROWS.map((row, i) => (
          <div
            key={row.problem}
            className={`grid grid-cols-1 @3xl:grid-cols-2 ${i < ROWS.length - 1 ? 'border-b border-rule' : ''}`}
          >
            <div className="px-5 py-5 border-l-2 border-l-alert/60 bg-alert/5 @3xl:border-r @3xl:border-r-rule">
              <div className="font-mono text-[14px] font-semibold lowercase text-ink">{row.problem}</div>
              <p className="mt-1.5 font-mono text-[12px] leading-[1.65] text-ink-faint lowercase max-w-[48ch]">
                {row.problemDetail}
              </p>
            </div>
            <div className="px-5 py-5 border-l-2 border-l-accent bg-bg">
              <div className="font-mono text-[14px] font-semibold lowercase text-ink">{row.answer}</div>
              <p className="mt-1.5 font-mono text-[12px] leading-[1.65] text-ink-faint lowercase max-w-[52ch]">
                {row.answerDetail}
              </p>
            </div>
          </div>
        ))}
      </div>
    </Section>
  )
}

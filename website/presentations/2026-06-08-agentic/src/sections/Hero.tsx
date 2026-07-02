import { Caret } from '@lib/components/schematic/Caret'
import { Cell } from '@lib/components/schematic/Cell'
import { Prompt } from '@lib/components/schematic/Prompt'
import { StatusDot } from '@lib/components/schematic/StatusDot'
import { Terminal, TerminalRow } from '@lib/components/schematic/Terminal'
import { STATS } from '../content/workers'

const CLAIMS = [
  {
    title: 'standalone first',
    body: 'each worker installs alone and is useful alone. the loop is one command away — not a framework to adopt.',
  },
  {
    title: 'reactive everywhere',
    body: 'every mutation emits an event. bind a function to a trigger type once and render live — no status chasing, ever.',
  },
  {
    title: 'durable execution',
    body: 'turns run as queued, resumable steps. a crash, a restart, or a week-long approval never loses work.',
  },
  {
    title: 'fail closed',
    body: 'agents reach only what policy allows. provenance follows every nested call — nothing gets laundered.',
  },
] as const

export function Hero() {
  return (
    <section id="hero">
      <div className="grid grid-cols-1 @4xl:grid-cols-2 gap-x-16 gap-y-12 px-4 pt-16 pb-14 @3xl:px-9 @3xl:pt-24 @3xl:pb-20">
        <div className="min-w-0">
          <div className="font-mono text-[11px] uppercase tracking-[0.06em] text-ink-faint mb-6">
            <Prompt symbol="$">tech-specs / 2026-06-08-agentic</Prompt>
          </div>
          <h1 className="font-mono font-semibold lowercase text-ink text-[44px] leading-[1.05] tracking-[-0.02em] @3xl:text-[64px] @3xl:leading-[1.02]">
            five small workers.
            <br />
            one agent backend.
          </h1>
          <p className="mt-6 font-mono text-[14px] leading-[1.7] text-ink-faint lowercase max-w-[56ch]">
            an agentic stack assembled from standalone iii workers that talk only over the bus —{' '}
            <span className="text-ink">reactive</span> at every surface, <span className="text-ink">durable</span> at
            every step, <span className="text-ink">governable</span> at every call.
          </p>
          <div className="mt-8 flex flex-wrap items-center gap-3">
            <a
              href="#map"
              className="inline-flex h-11 items-center bg-ink text-bg border border-ink px-5 font-mono text-[14px] lowercase transition-colors hover:bg-bg hover:text-ink"
            >
              walk the architecture →
            </a>
            <a
              href="#use-cases"
              className="inline-flex h-11 items-center bg-bg text-ink border border-ink px-5 font-mono text-[14px] lowercase transition-colors hover:bg-ink hover:text-bg"
            >
              jump to use cases
            </a>
          </div>
        </div>

        <div className="min-w-0 self-center">
          <Terminal title="a complete agent backend, from zero">
            <div className="flex flex-col gap-y-2.5">
              <TerminalRow
                command="iii worker add harness"
                output={
                  <span className="flex items-center gap-x-2">
                    <StatusDot pulse />
                    <span>connected — with session-manager, context-manager, llm-router</span>
                  </span>
                }
              />
              <TerminalRow
                command={
                  <>
                    iii trigger harness::send <span className="text-ink-faint">{'{ message, model }'}</span>
                  </>
                }
                output={
                  <span className="tabular-nums">{'→ { session_id: "s_7a1", turn_id: "t_001", accepted: true }'}</span>
                }
              />
              <div className="flex items-center gap-x-2">
                <Prompt symbol="$" />
                <Caret />
              </div>
            </div>
          </Terminal>

          <div className="grid grid-cols-2 @lg:grid-cols-4 border-x border-b border-rule bg-rule gap-px">
            {STATS.map((stat) => (
              <div key={stat.label} className="bg-bg px-4 py-3.5 min-w-0">
                <div className="font-mono text-[20px] font-semibold text-ink tabular-nums leading-none">
                  {stat.value}
                </div>
                <div className="mt-1.5 font-mono text-[10px] uppercase tracking-[0.06em] text-ink-faint leading-[1.4]">
                  {stat.label}
                </div>
              </div>
            ))}
          </div>
        </div>
      </div>

      <div className="border-t border-rule">
        <div className="grid grid-cols-1 @2xl:grid-cols-2 @5xl:grid-cols-4 gap-px bg-rule">
          {CLAIMS.map((claim, i) => (
            <Cell
              key={claim.title}
              title={
                <span className="flex items-center gap-x-2.5">
                  <span className="font-mono text-[11px] text-ink-ghost tabular-nums">0{i + 1}</span>
                  {claim.title}
                </span>
              }
              className="border-0"
              bodyClassName="max-w-[36ch]"
            >
              {claim.body}
            </Cell>
          ))}
        </div>
      </div>
    </section>
  )
}

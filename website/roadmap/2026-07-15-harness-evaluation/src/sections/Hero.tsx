import { Caret } from '@lib/components/schematic/Caret'
import { Cell } from '@lib/components/schematic/Cell'
import { Prompt } from '@lib/components/schematic/Prompt'
import { StatusDot } from '@lib/components/schematic/StatusDot'
import { Terminal, TerminalRow } from '@lib/components/schematic/Terminal'
import { HERO_CLAIMS, HERO_STATS } from '../content/hero'

export function Hero() {
  return (
    <section id="hero">
      <div className="grid grid-cols-1 gap-x-16 gap-y-12 px-4 pt-16 pb-14 @3xl:px-9 @3xl:pt-24 @3xl:pb-20 @4xl:grid-cols-2">
        <div className="min-w-0">
          <div className="mb-6 font-mono text-[11px] uppercase tracking-[0.06em] text-ink-faint">
            <Prompt symbol="$">harness / evaluation</Prompt>
          </div>
          <h1 className="font-mono text-[44px] leading-[1.05] font-semibold tracking-[-0.02em] text-ink lowercase @3xl:text-[64px] @3xl:leading-[1.02]">
            prove the system.
            <br />
            measure the agent.
          </h1>
          <p className="mt-6 max-w-[58ch] font-mono text-[14px] leading-[1.7] text-ink-faint lowercase">
            <span className="text-ink">deterministic integration</span> ·{' '}
            <span className="text-ink">real-model quality</span> ·{' '}
            <span className="text-ink">shared durable evidence</span>. two tracks exercise one public harness path while
            keeping their model boundaries, oracles, and gates explicit.
          </p>
          <div className="mt-8 flex flex-wrap items-center gap-3">
            <a
              href="#run"
              className="inline-flex h-11 items-center border border-ink bg-ink px-5 font-mono text-[14px] text-bg lowercase transition-colors hover:bg-bg hover:text-ink"
            >
              compare the tracks →
            </a>
            <a
              href="#map"
              className="inline-flex h-11 items-center border border-ink bg-bg px-5 font-mono text-[14px] text-ink lowercase transition-colors hover:bg-ink hover:text-bg"
            >
              inspect the stack
            </a>
          </div>
        </div>

        <div className="min-w-0 self-center">
          <Terminal title="two tracks · one public path">
            <div className="flex flex-col gap-y-2.5">
              <TerminalRow
                command="make -C harness integration-e2e"
                output={
                  <span>
                    ✓ 4 deterministic fixtures <span className="text-ink-faint">· no model key</span>
                  </span>
                }
              />
              <TerminalRow
                command="quality / e2e · one pinned subject"
                output={
                  <span className="flex items-center gap-x-2">
                    <StatusDot pulse />
                    <span>✓ 5 real-model scenarios · correctness + benchmark</span>
                  </span>
                }
              />
              <div className="flex items-center gap-x-2">
                <Prompt symbol="$" />
                <Caret />
              </div>
            </div>
          </Terminal>

          <div className="grid grid-cols-2 gap-px border-x border-b border-rule bg-rule @lg:grid-cols-4">
            {HERO_STATS.map((stat) => (
              <div key={stat.label} className="min-w-0 bg-bg px-4 py-3.5">
                <div className="font-mono text-[20px] leading-none font-semibold text-ink tabular-nums">
                  {stat.value}
                </div>
                <div className="mt-1.5 font-mono text-[10px] leading-[1.4] tracking-[0.06em] text-ink-faint uppercase">
                  {stat.label}
                </div>
              </div>
            ))}
          </div>
        </div>
      </div>

      <div className="border-t border-rule">
        <div className="grid grid-cols-1 gap-px bg-rule @2xl:grid-cols-2 @5xl:grid-cols-4">
          {HERO_CLAIMS.map((claim, index) => (
            <Cell
              key={claim.title}
              title={
                <span className="flex items-center gap-x-2.5">
                  <span className="font-mono text-[11px] text-ink-ghost tabular-nums">0{index + 1}</span>
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

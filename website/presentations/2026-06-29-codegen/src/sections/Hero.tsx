import { Caret } from '@lib/components/schematic/Caret'
import { Cell } from '@lib/components/schematic/Cell'
import { Prompt } from '@lib/components/schematic/Prompt'
import { StatusDot } from '@lib/components/schematic/StatusDot'
import { Terminal, TerminalRow } from '@lib/components/schematic/Terminal'
import { HERO_CLAIMS, HERO_STATS } from '../content/hero'

/**
 * A1 - the hero. Leads with the call every developer already writes by hand,
 * iii.trigger(...), and states the win: codegen makes that call typed. The
 * terminal shows generate-then-call; the stat strip quantifies it.
 */
export function Hero() {
  return (
    <section id="hero">
      <div className="grid grid-cols-1 @4xl:grid-cols-2 gap-x-16 gap-y-12 px-4 pt-16 pb-14 @3xl:px-9 @3xl:pt-24 @3xl:pb-20">
        <div className="min-w-0">
          <div className="font-mono text-[11px] uppercase tracking-[0.06em] text-ink-faint mb-6">
            <Prompt symbol="$">codegen · for iii workers</Prompt>
          </div>
          <h1 className="font-mono font-semibold lowercase break-words text-ink text-[44px] leading-[1.05] tracking-[-0.02em] @3xl:text-[64px] @3xl:leading-[1.02]">
            every iii.trigger()
            <br />
            call, fully typed.
          </h1>
          <p className="mt-6 font-mono text-[14px] leading-[1.7] text-ink-faint lowercase max-w-[58ch]">
            you already write <span className="text-ink">iii.trigger({'{ function_id, payload }'})</span> by hand.
            codegen reads the engine's live catalog and generates the <span className="text-ink">typed wrapper</span>{' '}
            for you, in typescript, rust, python, and javascript.
          </p>
          <div className="mt-8 flex flex-wrap items-center gap-3">
            <a
              href="#languages"
              className="inline-flex h-11 items-center bg-ink text-bg border border-ink px-5 font-mono text-[14px] lowercase transition-colors hover:bg-bg hover:text-ink focus-visible:ring-1 focus-visible:ring-accent focus:outline-none"
            >
              see the generated code →
            </a>
            <a
              href="#map"
              className="inline-flex h-11 items-center bg-bg text-ink border border-ink px-5 font-mono text-[14px] lowercase transition-colors hover:bg-ink hover:text-bg focus-visible:ring-1 focus-visible:ring-accent focus:outline-none"
            >
              walk the architecture
            </a>
          </div>
        </div>

        <div className="min-w-0 self-center">
          <Terminal title="generate, then call it typed">
            <div className="flex flex-col gap-y-2.5">
              <TerminalRow
                command="codegen generate --config codegen.yml"
                output={
                  <span className="flex items-center gap-x-2">
                    <StatusDot pulse />
                    <span>wrote src/iii/harness.ts · 7 fns · 11 types</span>
                  </span>
                }
              />
              <TerminalRow
                command={
                  <>
                    await harness.send(
                    <span className="text-ink-faint">{'{ message: "summarise the readme" }'}</span>)
                  </>
                }
                output={<span className="tabular-nums">{'↳ SendResult { session_id, turn_id, accepted }'}</span>}
              />
              <div className="flex items-center gap-x-2">
                <Prompt symbol="$" />
                <Caret />
              </div>
            </div>
          </Terminal>

          <div className="grid grid-cols-2 @lg:grid-cols-4 border-x border-b border-rule bg-rule gap-px">
            {HERO_STATS.map((stat) => (
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
        <div className="grid grid-cols-1 @2xl:grid-cols-3 gap-px bg-rule">
          {HERO_CLAIMS.map((claim, i) => (
            <Cell
              key={claim.title}
              title={
                <span className="flex items-center gap-x-2.5">
                  <span className="font-mono text-[11px] text-ink-ghost tabular-nums">0{i + 1}</span>
                  {claim.title}
                </span>
              }
              className="border-0"
              bodyClassName="max-w-[40ch]"
            >
              {claim.body}
            </Cell>
          ))}
        </div>
      </div>
    </section>
  )
}

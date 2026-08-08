import { Caret } from '@lib/components/schematic/Caret'
import { Cell } from '@lib/components/schematic/Cell'
import { Prompt } from '@lib/components/schematic/Prompt'
import { StatusDot } from '@lib/components/schematic/StatusDot'
import { Terminal, TerminalRow } from '@lib/components/schematic/Terminal'
import { HERO_CLAIMS, HERO_STATS } from '../content/hero'

/**
 * A1 — the hero. The win in one line, never the mechanism. The terminal shows
 * the boundary in two frames: a clean up, and a collision that fails loud.
 */
export function Hero() {
  return (
    <section id="hero">
      <div className="grid grid-cols-1 @4xl:grid-cols-2 gap-x-16 gap-y-12 px-4 pt-16 pb-14 @3xl:px-9 @3xl:pt-24 @3xl:pb-20">
        <div className="min-w-0">
          <div className="font-mono text-[11px] uppercase tracking-[0.06em] text-ink-faint mb-6">
            <Prompt symbol="$">iii / tech-specs / worker-compose</Prompt>
          </div>
          <h1 className="font-mono font-semibold lowercase text-ink text-[44px] leading-[1.05] tracking-[-0.02em] @3xl:text-[64px] @3xl:leading-[1.02]">
            one file.
            <br />
            one contract.
            <br />
            zero zombies.
          </h1>
          <p className="mt-6 font-mono text-[14px] leading-[1.7] text-ink-faint lowercase max-w-[58ch]">
            a compose daemon per machine supervises the workers one file declares:{' '}
            <span className="text-ink">namespaced</span> so nothing collides,{' '}
            <span className="text-ink">config-fed</span> so nothing boots wrong, and{' '}
            <span className="text-ink">crash-cascading</span> so nothing fails quietly.
          </p>
          <div className="mt-8 flex flex-wrap items-center gap-3">
            <a
              href="#run-it"
              className="inline-flex h-11 items-center bg-ink text-bg border border-ink px-5 font-mono text-[14px] lowercase transition-colors hover:bg-bg hover:text-ink"
            >
              see it run →
            </a>
            <a
              href="#/spec"
              className="inline-flex h-11 items-center bg-bg text-ink border border-ink px-5 font-mono text-[14px] lowercase transition-colors hover:bg-ink hover:text-bg"
            >
              read the spec
            </a>
          </div>
        </div>

        <div className="min-w-0 self-center">
          <Terminal title="the boundary, in two frames">
            <div className="flex flex-col gap-y-2.5">
              <TerminalRow
                command="iii trigger compose::up id=host-a"
                output={
                  <span className="flex items-center gap-x-2">
                    <StatusDot pulse />
                    <span>database ready · api ready · ns orders</span>
                  </span>
                }
              />
              <TerminalRow
                command="iii trigger compose::up id=host-b   # same file, again"
                output={<span className="text-alert">✗ rejected · database already registered in ns orders</span>}
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
        <div className="grid grid-cols-1 @2xl:grid-cols-2 @5xl:grid-cols-4 gap-px bg-rule">
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

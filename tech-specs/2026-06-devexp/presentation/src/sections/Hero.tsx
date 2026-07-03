import { Caret } from '@/components/schematic/Caret'
import { Cell } from '@/components/schematic/Cell'
import { Prompt } from '@/components/schematic/Prompt'
import { Terminal, TerminalRow } from '@/components/schematic/Terminal'
import { CLAIMS, GOLDEN_CHIPS, STATS } from '@/content/changes'

export function Hero() {
  return (
    <section id="hero">
      <div className="grid grid-cols-1 @4xl:grid-cols-2 gap-x-16 gap-y-12 px-4 pt-16 pb-14 @3xl:px-9 @3xl:pt-24 @3xl:pb-20">
        <div className="min-w-0">
          <div className="font-mono text-[11px] uppercase tracking-[0.06em] text-ink-faint mb-6">
            <Prompt symbol="$">tech-specs / 2026-06-devexp</Prompt>
          </div>
          <h1 className="font-mono font-semibold lowercase text-ink text-[40px] leading-[1.05] tracking-[-0.02em] @3xl:text-[60px] @3xl:leading-[1.02]">
            one declarative file.
            <br />
            four clean planes.
            <br />
            zero zombies.
          </h1>
          <p className="mt-6 font-mono text-[14px] leading-[1.7] text-ink-faint lowercase max-w-[58ch]">
            today’s iii is a tangle — process management, config.yaml,
            iii-worker-manager, iii-exec, sandboxes, and ~30 cli leaf-commands,
            all operated separately. this overhaul re-wires that sound machinery
            into <span className="text-ink">four clean planes</span> that touch
            in exactly <span className="text-ink">three places</span>, behind
            one <span className="text-ink">worker-compose.yml</span>.
          </p>

          <div className="mt-7 flex flex-wrap items-center gap-2">
            {GOLDEN_CHIPS.map((chip, i) => (
              <span key={chip} className="flex items-center gap-x-2">
                <span className="inline-flex items-center border border-rule bg-bg px-2.5 py-1 font-mono text-[12.5px] text-ink">
                  {chip}
                </span>
                {i < GOLDEN_CHIPS.length - 1 ? (
                  <span className="font-mono text-[12px] text-ink-ghost">→</span>
                ) : null}
              </span>
            ))}
          </div>

          <div className="mt-8 flex flex-wrap items-center gap-3">
            <a
              href="#playground"
              className="inline-flex h-11 items-center bg-ink text-bg border border-ink px-5 font-mono text-[14px] lowercase transition-colors hover:bg-bg hover:text-ink"
            >
              run the playground →
            </a>
            <a
              href="#map"
              className="inline-flex h-11 items-center bg-bg text-ink border border-ink px-5 font-mono text-[14px] lowercase transition-colors hover:bg-ink hover:text-bg"
            >
              walk the architecture
            </a>
          </div>
        </div>

        <div className="min-w-0 self-center">
          <Terminal title="from zero to a running worker">
            <div className="flex flex-col gap-y-2.5">
              <TerminalRow command="curl -fsSL https://install.iii.dev/… | sh" output="✓ iii 0.12.0 installed" />
              <TerminalRow command="iii init quickstart && cd quickstart" output="✓ scaffolded worker-compose.yml" />
              <TerminalRow
                command="iii up"
                output={
                  <span>
                    all workers READY (3 managed) on port 49134 — depends_on gated
                  </span>
                }
              />
              <TerminalRow
                command="iii trigger math::add_two_numbers a=10 b=20"
                output={<span className="tabular-nums">{'{ "c": 30 }'}</span>}
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
                <div className="font-mono text-[24px] font-semibold text-ink tabular-nums leading-none">
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
                  <span className="font-mono text-[11px] text-ink-ghost tabular-nums">
                    0{i + 1}
                  </span>
                  {claim.title}
                </span>
              }
              className="border-0"
              bodyClassName="max-w-[38ch]"
            >
              {claim.body}
            </Cell>
          ))}
        </div>
      </div>
    </section>
  )
}

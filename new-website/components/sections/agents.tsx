import { AgentsHarness } from '@/components/graphics/agents-harness'
import { CodeBlock } from '@/components/site/code-block'
import { Reveal } from '@/components/site/reveal'
import { Section } from '@/components/site/section'

const sendCode = `
await iii.trigger({
  function_id: 'harness::send',
  payload: {
    message: 'Refund order 1042 and email the customer',
    options: { functions: { allow: ['orders::*', 'email::send'] } },
  },
})
`

const controls = [
  {
    title: 'Access you decide',
    body: 'Agents start with no function access. Allow ids or globs per session, send sensitive calls to human approval, rewrite or deny calls in hooks.',
  },
  {
    title: 'Agents that coordinate agents',
    body: 'An agent spawns sub-agents with depth and fan-out limits, hands out kanban tickets, gives each its own git worktree, and every run lands in one trace.',
  },
]

const features = [
  {
    title: 'Your coding agents, as workers',
    label: 'agents',
    chips: ['Claude Code', 'Codex', 'Cursor', 'OpenCode', 'pi', 'Devin', 'Grok', 'Hermes'],
  },
  {
    title: 'A harness built from replaceable workers',
    label: 'models',
    chips: ['OpenAI', 'Anthropic', 'DeepSeek', 'xAI', 'Z.AI', 'Kimi', 'OpenRouter', 'llama.cpp'],
  },
  {
    title: 'Where your team already is',
    label: 'channels',
    chips: ['ADE', 'Slack', 'Telegram', 'Hermes · 27+', 'ACP editors', 'MCP clients'],
  },
]

export function Agents() {
  return (
    // biome-ignore lint/correctness/useUniqueElementIds: This homepage section has one stable public anchor.
    <Section
      id="agents"
      eyebrow="AI agents"
      title="Keep Claude Code and Codex. Run them inside your system."
      lede="They don't see its functions, share its queues, follow its permissions, or appear in its traces. iii gives them, and any agent you build, a place inside it."
    >
      <div className="mt-12 grid grid-cols-1 items-start gap-10 md:mt-14 lg:grid-cols-[minmax(0,1fr)_minmax(0,480px)] lg:grid-rows-[auto_1fr] lg:gap-x-12">
        <Reveal delay={0.1} className="min-w-0 lg:col-start-1 lg:row-start-1">
          <CodeBlock code={sendCode} lang="ts" title="agent.ts" typed />
        </Reveal>
        <Reveal delay={0.15} className="mx-auto w-full max-w-[480px] lg:col-start-2 lg:row-span-2 lg:row-start-1">
          <div className="graphic-stage">
            <AgentsHarness />
          </div>
        </Reveal>
        <div className="grid gap-8 sm:grid-cols-2 lg:col-start-1 lg:row-start-2 lg:grid-cols-1">
          {controls.map((c, i) => (
            <Reveal key={c.title} delay={0.05 + i * 0.05}>
              <h3 className="text-balance font-medium text-lg tracking-tight">{c.title}</h3>
              <p className="mt-2 text-pretty text-[15px] text-muted-foreground leading-relaxed">{c.body}</p>
            </Reveal>
          ))}
        </div>
      </div>

      <ul className="mt-16 grid gap-4 md:grid-cols-3">
        {features.map((f, i) => (
          <Reveal
            as="li"
            key={f.title}
            delay={i * 0.06}
            className="flex flex-col overflow-hidden rounded-xl border bg-card"
          >
            <div className="flex min-h-28 flex-col gap-3 border-b bg-faint px-6 pt-5 pb-5">
              <p className="font-mono text-[12px] text-muted-foreground uppercase tracking-[0.08em]">{f.label}</p>
              <ul className="flex flex-wrap gap-1.5" aria-label={f.label}>
                {f.chips.map((chip) => (
                  <li
                    key={chip}
                    className="rounded-md border bg-card px-2 py-0.5 font-mono text-[12px] text-foreground/85"
                  >
                    {chip}
                  </li>
                ))}
              </ul>
            </div>
            <h3 className="text-balance px-6 py-5 font-medium text-[15px] tracking-tight">{f.title}</h3>
          </Reveal>
        ))}
      </ul>
    </Section>
  )
}

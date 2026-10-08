import { ArrowUpRightIcon } from 'lucide-react'
import type { ComponentType } from 'react'

import { RegistryDemo } from '@/components/graphics/registry-demo'
import { IconBot, IconChat, IconCode, IconPackage, IconServer, IconSetting } from '@/components/icons/iconly'
import { CopyCommand } from '@/components/site/copy-command'
import { Reveal } from '@/components/site/reveal'
import { Section } from '@/components/site/section'
import { links } from '@/lib/site'
import { cn } from '@/lib/utils'

type Category = { label: string; Icon: ComponentType<{ className?: string }>; items: string[]; className?: string }

const categories: Category[] = [
  {
    label: 'Infrastructure',
    Icon: IconServer,
    items: ['http', 'queue', 'cron', 'pubsub', 'state', 'database', 'storage', 'bridge'],
    className: 'lg:col-start-1 lg:row-start-1',
  },
  {
    label: 'Coding agents',
    Icon: IconCode,
    items: ['claude-code', 'codex', 'cursor', 'opencode', 'pi', 'devin', 'grok'],
    className: 'lg:col-start-1 lg:row-start-2',
  },
  {
    label: 'Agents',
    Icon: IconBot,
    items: ['harness', 'llm-router', 'memory', 'approval-gate', 'judge', 'eval'],
    className: 'lg:col-start-3 lg:row-start-1',
  },
  {
    label: 'Tools for agents',
    Icon: IconSetting,
    items: ['browser', 'computer', 'ios-simulator', 'code-runner', 'github', 'worktree', 'pdf'],
    className: 'lg:col-start-3 lg:row-start-2',
  },
  {
    label: 'Interfaces',
    Icon: IconChat,
    items: ['ADE', 'Slack', 'Telegram', 'voice', 'MCP', 'ACP', 'canvas', 'kanban'],
    className: 'lg:col-start-3 lg:row-start-3',
  },
]

/**
 * Registry: a hairline grid. The live console demo sits in the centre; the worker categories frame
 * it as cells, and the install command closes the grid along the bottom.
 */
export function Registry() {
  return (
    // biome-ignore lint/correctness/useUniqueElementIds: This homepage section has one stable public anchor.
    <Section
      id="registry"
      eyebrow="The registry"
      title="Install capabilities as running services."
      lede={
        <>
          The worker starts, its functions and triggers appear in the registry, and the pinned version goes into{' '}
          <code className="font-mono text-[0.9em] text-foreground">worker-compose.yaml</code>. A running agent can call
          the new functions on its next step.
        </>
      }
    >
      <Reveal
        delay={0.1}
        className="mt-12 grid gap-px overflow-hidden rounded-2xl border bg-border md:mt-14 lg:grid-cols-[minmax(0,1fr)_minmax(0,2.4fr)_minmax(0,1fr)] lg:grid-rows-[1fr_1fr_auto]"
      >
        <div className="-order-1 min-w-0 bg-card lg:order-none lg:col-start-2 lg:row-span-2 lg:row-start-1">
          <RegistryDemo embedded className="h-full" />
        </div>
        {categories.map((c) => (
          <CategoryCell key={c.label} {...c} />
        ))}
        <div className="flex min-w-0 flex-col gap-4 bg-card p-5 lg:col-span-2 lg:col-start-1 lg:row-start-3">
          <p className="flex items-center gap-2 font-medium text-[13px]">
            <IconPackage className="size-4 text-muted-foreground" />
            Install a worker
          </p>
          <div className="flex min-w-0 flex-col gap-3 sm:flex-row sm:items-center">
            <CopyCommand
              command="iii trigger compose::add worker=database"
              className="min-w-0 flex-1 border-0 bg-faint"
            />
            <a
              href={links.registry}
              className="group inline-flex h-11 shrink-0 items-center gap-1.5 whitespace-nowrap rounded-lg px-3 text-[13px] text-muted-foreground transition-colors hover:text-foreground focus-visible:outline-2 focus-visible:outline-foreground focus-visible:outline-offset-2"
            >
              Browse all workers
              <ArrowUpRightIcon
                aria-hidden
                className="size-3.5 transition-transform duration-150 group-hover:translate-x-px group-hover:-translate-y-px motion-reduce:transition-none"
              />
            </a>
          </div>
        </div>
      </Reveal>
    </Section>
  )
}

function CategoryCell({ label, Icon, items, className }: Category) {
  return (
    <div className={cn('flex min-w-0 flex-col gap-4 bg-card p-5', className)}>
      <p className="flex items-center gap-2 font-medium text-[13px]">
        <Icon className="size-4 text-muted-foreground" />
        {label}
      </p>
      <ul className="flex flex-wrap gap-1.5" aria-label={label}>
        {items.map((item) => (
          <li key={item} className="rounded-md border px-2 py-[3px] font-mono text-[12px] text-muted-foreground">
            {item}
          </li>
        ))}
      </ul>
    </div>
  )
}

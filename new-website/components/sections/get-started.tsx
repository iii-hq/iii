import { ArrowRightIcon } from 'lucide-react'

import { IconTerminal } from '@/components/icons/iconly'
import { CodeBlock } from '@/components/site/code-block'
import { Reveal } from '@/components/site/reveal'
import { Section } from '@/components/site/section'
import { buttonVariants } from '@/components/ui/button'
import { installCommand, links } from '@/lib/site'
import { cn } from '@/lib/utils'

type Step = {
  title: string
  optional?: boolean
  /** Shown between the title and the command. */
  note?: string
  command: string
}

const steps: Step[] = [
  { title: 'Install iii', command: installCommand },
  { title: 'Create a project', command: 'iii project init quickstart --template quickstart' },
  {
    title: 'Run it',
    note: 'You get a Python worker and a TypeScript worker calling each other through the engine, plus state and an HTTP route.',
    command: 'cd quickstart && iii compose --up',
  },
  {
    title: 'Add agents and the ADE',
    optional: true,
    command: 'iii trigger compose::add worker=harness worker=ade\nopen http://localhost:3113',
  },
]

function TerminalGlyph() {
  return (
    <span className="flex size-6 items-center justify-center rounded-md bg-faint text-muted-foreground">
      <IconTerminal className="size-3.5" />
    </span>
  )
}

/** Get started: the quickstart as a numbered rail, one terminal command per step. */
export function GetStarted() {
  return (
    // biome-ignore lint/correctness/useUniqueElementIds: This homepage section has one stable public anchor.
    <Section id="get-started" eyebrow="Get started" title="Run the quickstart." split>
      <div className="mt-12 lg:mt-0">
        <ol className="relative ml-4 flex flex-col gap-10 pl-8 sm:pl-10">
          {steps.map((step, i) => (
            <Reveal
              key={step.title}
              as="li"
              delay={0.1 + i * 0.05}
              className={cn(
                'relative flex min-w-0 flex-col gap-4',
                // Rail: from this badge down to the next one; the last step has none, so the rail ends there.
                i < steps.length - 1 &&
                  'before:absolute before:top-8 before:-left-8 before:h-[calc(100%-2rem+2.5rem)] before:w-px before:-translate-x-1/2 before:bg-border sm:before:-left-10',
              )}
            >
              <span
                aria-hidden
                className="absolute top-0 -left-8 flex size-8 translate-x-[calc(-50%-0.5px)] items-center justify-center rounded-full border bg-background font-mono text-[13px] tabular-nums sm:-left-10"
              >
                {i + 1}
              </span>
              <h3 className="flex h-8 flex-wrap items-center gap-x-3 gap-y-1 font-medium text-[17px] tracking-[-0.01em]">
                {step.title}
                {step.optional ? (
                  <span className="rounded-md border px-1.5 py-0.5 font-mono font-normal text-[11px] text-muted-foreground leading-none">
                    optional
                  </span>
                ) : null}
              </h3>
              {step.note ? (
                <p className="max-w-[560px] text-pretty text-[15px] text-muted-foreground leading-relaxed">
                  {step.note}
                </p>
              ) : null}
              <CodeBlock code={step.command} lang="bash" title="terminal" icon={<TerminalGlyph />} typed />
            </Reveal>
          ))}
        </ol>
        <Reveal delay={0.3} className="mt-10 ml-4 pl-8 sm:pl-10">
          <a
            href={links.quickstart}
            className={cn(buttonVariants(), 'group h-12 w-full gap-2.5 rounded-xl px-5 text-[15px] sm:h-11 sm:w-auto')}
          >
            Follow the quickstart
            <ArrowRightIcon
              aria-hidden
              strokeWidth={1.75}
              className="size-4 transition-transform duration-150 group-hover:translate-x-0.5 motion-reduce:transition-none"
            />
          </a>
        </Reveal>
      </div>
    </Section>
  )
}

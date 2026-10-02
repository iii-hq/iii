'use client'

import { ArrowRightIcon } from 'lucide-react'

import { IconCheckCircle } from '@/components/icons/iconly'
import type { CodeLine } from '@/lib/highlight'
import { cn } from '@/lib/utils'

import { story } from './content'
import { HOPS } from './graph/model'
import { TokenCode } from './token-code'

const card = 'rounded-xl border bg-card'
const head = 'border-b px-4 py-2.5 font-sans text-[13px] text-muted-foreground uppercase tracking-[0.08em]'

/** Pre-highlighted code the story needs; tokenized on the server and passed down. */
export type StoryCode = {
  files: CodeLine[][]
  chain: CodeLine[]
  extend: CodeLine[]
}
export type AsideProps = { step: number; code: StoryCode }

/** Composability: the two compose files as real YAML, then the functions composing as code, in step with the graph. */
export function ComposeAside({ step, code }: AsideProps) {
  return (
    <div className="flex flex-col gap-4">
      <ul className="grid grid-cols-2 gap-x-6 gap-y-1.5 font-sans text-[13px] sm:grid-cols-3">
        {story.compose.rows.map(([from, to]) => (
          <li key={`${from}${to}`} className="flex items-center gap-1.5 text-muted-foreground">
            <span className="text-foreground">{from}</span>
            <ArrowRightIcon aria-hidden className="size-3" />
            <span className="text-foreground">{to}</span>
          </li>
        ))}
      </ul>
      {story.compose.files.map((file, i) => (
        <TokenCode
          key={file.name}
          title={file.name}
          status={step === i ? 'composing' : step >= 2 ? 'running' : undefined}
          lines={code.files[i]}
          lit={step === i || step >= 2}
          highlight={step === i ? file.code.map((_, n) => n).slice(1) : []}
        />
      ))}
      <p className="font-sans text-[13px] text-muted-foreground">
        <span className="font-mono text-foreground">$ iii compose --up</span> · both files connect to the same engine
      </p>
      <TokenCode
        title="then the functions compose"
        status={step === 4 ? 'running across 4 workers' : undefined}
        lines={code.chain}
        lit={step === 4}
        highlight={step === 4 ? [0, 1, 2, 3] : []}
      />
    </div>
  )
}

/** Observability: the trace list that mirrors the spans drawing under the graph. */
export function ObserveAside({ step }: AsideProps) {
  return (
    <div className={card}>
      <p className={cn(head, 'flex justify-between')}>
        <span>trace 7f3a</span>
        <span>{HOPS.length} spans</span>
      </p>
      <ol className="divide-y">
        {[...HOPS, null].map((hop, i) => {
          const shown = step < 0 || step >= i
          const lit = step === i || (hop === null && step === 9)
          if (hop === null)
            return (
              <li
                key="otlp"
                className={cn(
                  'flex items-center gap-3 border-dashed px-4 py-2 font-sans text-[13px] transition-colors duration-300',
                  step < 0 || step >= 9 ? 'text-foreground' : 'text-muted-foreground/50',
                  lit ? 'bg-faint' : '',
                )}
              >
                <span className={cn('size-1.5 shrink-0 rounded-full', lit ? 'bg-ok' : 'bg-transparent')} />
                <span className="truncate">{story.observe.export}</span>
                <ArrowRightIcon aria-hidden className="ml-auto size-3 shrink-0 text-muted-foreground" />
              </li>
            )
          return (
            <li
              key={hop.fn}
              className={cn(
                'flex items-center gap-3 px-4 py-2 font-sans text-[13px] transition-colors duration-300',
                shown ? 'text-foreground' : 'text-muted-foreground/50',
                lit ? 'bg-faint' : '',
              )}
            >
              <span
                className={cn(
                  'size-1.5 shrink-0 rounded-full',
                  lit ? 'bg-ok' : shown ? 'bg-foreground/50' : 'bg-transparent',
                )}
              />
              <span className="truncate font-mono">{hop.fn}</span>
              <span className="hidden text-muted-foreground sm:inline">{hop.lang}</span>
              <span className="ml-auto shrink-0 text-muted-foreground tabular-nums">
                {hop.ms.toLocaleString('en-US')}ms
              </span>
            </li>
          )
        })}
      </ol>
    </div>
  )
}

/** Extensibility: the hand-off from Discoverability, the capability catalog, the build-your-own card, and the code. */
export function ExtendAside({ step, code }: AsideProps) {
  const { categories, build, handoff } = story.extend
  return (
    <div className="flex flex-col gap-4">
      <p
        className={cn(
          'rounded-xl border px-4 py-3 font-sans text-[13px] leading-relaxed transition-colors duration-300',
          step === 0 ? 'border-line-strong text-foreground' : 'text-muted-foreground',
        )}
      >
        {handoff}
      </p>
      <dl className="grid gap-x-6 gap-y-3 sm:grid-cols-2">
        {categories.map((c) => (
          <div key={c.label} className="min-w-0">
            <dt className="font-sans text-[12px] text-muted-foreground uppercase tracking-[0.08em]">{c.label}</dt>
            <dd className="mt-0.5 truncate text-[13px] text-foreground">{c.items.join(' · ')}</dd>
          </div>
        ))}
      </dl>
      <div className={cn(card, 'border-dashed')}>
        <p className={cn(head, 'text-foreground')}>{build.title}</p>
        <p className="px-4 py-3 text-[13.5px] text-muted-foreground leading-relaxed">{build.body}</p>
      </div>
      <TokenCode
        title="any worker, from anywhere"
        lines={code.extend}
        lit={step === 5}
        highlight={step === 5 ? [0] : []}
      />
    </div>
  )
}

const DISCOVER_STEPS = [
  { at: 0, text: `Read the whole prompt: it needs ${story.discover.needs.join(', ')}` },
  { at: 1, text: '1 · The running system already has github, browser, llm' },
  { at: 2, text: '2 · Nothing for slack here, so search workers.iii.dev' },
  { at: 3, text: 'slack@1.2.0 found, installed like a library, slack::post joins' },
  { at: 5, text: '3 · Nothing formats a digest, so the harness writes it (next section)' },
  { at: 6, text: 'The agent uses all of it, the same turn' },
]

/** Discoverability: the harness working through a prompt, checked off in step with the graph. */
export function DiscoverAside({ step }: AsideProps) {
  const idle = step < 0
  return (
    <div className="flex flex-col gap-4">
      <ol className={cn(card, 'divide-y')}>
        {DISCOVER_STEPS.map((s) => {
          const done = idle || step > s.at
          const now = !idle && step === s.at
          return (
            <li
              key={s.text}
              className={cn(
                'flex items-center gap-3 px-4 py-2.5 text-[13px] transition-colors duration-300',
                done || now ? 'text-foreground' : 'text-muted-foreground/60',
                now ? 'bg-faint' : '',
              )}
            >
              {done ? (
                <IconCheckCircle className="size-4 shrink-0 text-hero-accent" />
              ) : (
                <span
                  aria-hidden
                  className={cn('size-4 shrink-0 rounded-full border', now ? 'border-foreground' : 'border-border')}
                />
              )}
              <span className="font-sans text-[13px]">{s.text}</span>
            </li>
          )
        })}
      </ol>
      <BeforeAfter before={story.discover.before} after={story.discover.after} />
    </div>
  )
}

/** Reactivity: before and after, from the doc. */
export function ReactAside(_: AsideProps) {
  return <BeforeAfter before={story.react.before} after={story.react.after} />
}

/** Summary: the five words light up as the graph labels them. */
export function SummaryAside({ step }: AsideProps) {
  return (
    <ol className="flex flex-col gap-1.5">
      {story.summary.words.map((word, i) => {
        const lit = step < 0 || step >= i
        return (
          <li
            key={word}
            className={cn(
              'flex items-center gap-3 font-pixel text-[22px] transition-colors duration-300 sm:text-[26px]',
              lit ? 'text-foreground' : 'text-muted-foreground/40',
            )}
          >
            <span className={cn('font-sans text-[13px]', lit ? 'text-hero-accent' : 'text-muted-foreground/40')}>
              {word[0]}
            </span>
            {word}
          </li>
        )
      })}
    </ol>
  )
}

function BeforeAfter({ before, after }: { before: string; after: string }) {
  return (
    <div className="grid gap-3 sm:grid-cols-2">
      <div className={card}>
        <p className={head}>Before</p>
        <p className="px-4 py-3 font-sans text-[13px] text-muted-foreground leading-relaxed">{before}</p>
      </div>
      <div className={cn(card, 'border-line-strong')}>
        <p className={cn(head, 'text-foreground')}>After iii</p>
        <p className="px-4 py-3 font-sans text-[13px] text-foreground leading-relaxed">{after}</p>
      </div>
    </div>
  )
}

'use client'

import { Tabs } from '@base-ui/react/tabs'
import { ArrowUpRightIcon } from 'lucide-react'
import { motion } from 'motion/react'
import { useEffect, useRef, useState } from 'react'
import type { BundledLanguage } from 'shiki'

import { DemoPlayback } from '@/components/graphics/demo-playback'
import {
  OneFunctionConvergence,
  OneFunctionTrace,
  type ReachPath,
} from '@/components/graphics/one-function-convergence'
import { IconCopy, IconTickSquare } from '@/components/icons/iconly'
import { useDemoPlayback } from '@/hooks/use-demo-playback'
import { spring } from '@/lib/motion'
import { cn } from '@/lib/utils'

export type OneFunctionSnippet = {
  path: ReachPath
  file: string
  lang: BundledLanguage
  via: string
  title: string
  description: string
  code: string
  /** Shiki output for `code`, produced on the server. */
  html: string
}

export function OneFunctionDemo({ snippets }: { snippets: OneFunctionSnippet[] }) {
  const { ref, reduce, paused, setPaused, running } = useDemoPlayback()
  const [index, setIndex] = useState(0)
  const [run, setRun] = useState(0)
  const [hovered, setHovered] = useState(false)
  const [focused, setFocused] = useState(false)
  const [pinned, setPinned] = useState(false)
  const current = snippets[index]

  useEffect(() => {
    if (!running) return
    const timer = window.setInterval(() => {
      if (!pinned && !hovered && !focused) setIndex((value) => (value + 1) % snippets.length)
      setRun((value) => value + 1)
    }, 6500)
    return () => window.clearInterval(timer)
  }, [running, hovered, focused, pinned, snippets.length])

  /* Reading the code or the tabs holds the current entry point; the replay keeps running. */
  const hold = {
    onPointerEnter: (event: React.PointerEvent) => event.pointerType === 'mouse' && setHovered(true),
    onPointerLeave: () => setHovered(false),
    onFocusCapture: () => setFocused(true),
    onBlurCapture: (event: React.FocusEvent) => {
      if (!event.currentTarget.contains(event.relatedTarget)) setFocused(false)
    },
  }

  return (
    <div ref={ref} data-running={running} className="mt-12 min-w-0 md:mt-14">
      <Tabs.Root
        value={current.path}
        onValueChange={(value) => {
          setIndex(snippets.findIndex((snippet) => snippet.path === value))
          setPinned(true)
          setRun((value) => value + 1)
        }}
      >
        <div className="flex items-center justify-between gap-3 border-y py-1.5" {...hold}>
          <Tabs.List
            activateOnFocus
            className="relative inline-flex h-9 items-center gap-0.5 rounded-lg border bg-foreground/[0.03] p-0.5 dark:bg-foreground/[0.04]"
            aria-label="Ways to call a function"
          >
            {snippets.map((snippet) => {
              const selected = snippet.path === current.path
              return (
                <Tabs.Tab
                  key={snippet.path}
                  value={snippet.path}
                  className={cn(
                    'relative h-full min-w-[84px] rounded-[7px] px-4 font-medium text-[13px] outline-none transition-colors focus-visible:outline-2 focus-visible:outline-foreground focus-visible:outline-offset-2 sm:min-w-[96px]',
                    selected ? 'text-foreground' : 'text-muted-foreground hover:text-foreground',
                  )}
                >
                  {selected ? (
                    <motion.span
                      layoutId="one-function-thumb"
                      aria-hidden
                      className="absolute inset-0 rounded-[7px] border bg-background shadow-[0_1px_2px_rgb(0_0_0/0.12)] dark:bg-foreground/[0.1] dark:shadow-none"
                      transition={spring.snappy}
                    />
                  ) : null}
                  <span className="relative">{snippet.via}</span>
                </Tabs.Tab>
              )
            })}
          </Tabs.List>
          <DemoPlayback paused={paused} reduce={reduce} onToggle={() => setPaused(!paused)} />
        </div>

        <Tabs.Panel
          value={current.path}
          className="pt-8 outline-offset-4 focus-visible:outline-2 focus-visible:outline-foreground lg:pt-10"
        >
          <div key={current.path} className="swap-in max-w-xl">
            <h3 className="font-medium text-xl tracking-tight">{current.title}</h3>
            <p className="mt-2 text-pretty text-[14px] text-muted-foreground leading-relaxed">{current.description}</p>
          </div>

          <div className="graphic-stage mt-9 lg:mt-12">
            <OneFunctionConvergence path={current.path} run={run} active={running} />
          </div>

          <div
            className="mt-10 grid min-w-0 gap-8 lg:mt-14 lg:grid-cols-[minmax(0,7fr)_minmax(0,5fr)] lg:gap-12"
            {...hold}
          >
            <SnippetCode key={current.path} snippet={current} />
            <div className="min-w-0 self-start">
              <OneFunctionTrace path={current.path} run={run} active={running} />
              <p className="mt-4 font-mono text-[12px] text-muted-foreground">
                Different entry points. The same logic. One trace.
              </p>
            </div>
          </div>
        </Tabs.Panel>
      </Tabs.Root>
      <a
        href="https://iii.dev/docs"
        className="mt-6 inline-flex min-h-11 items-center gap-2 text-[13px] text-muted-foreground underline-offset-4 hover:underline focus-visible:outline-2 focus-visible:outline-foreground focus-visible:outline-offset-2"
      >
        Explore functions and triggers <ArrowUpRightIcon aria-hidden className="size-3.5" />
      </a>
    </div>
  )
}

function SnippetCode({ snippet }: { snippet: OneFunctionSnippet }) {
  const [status, setStatus] = useState<'idle' | 'copied' | 'error'>('idle')
  const reset = useRef<number | undefined>(undefined)
  useEffect(() => () => window.clearTimeout(reset.current), [])
  const copy = async () => {
    window.clearTimeout(reset.current)
    try {
      await navigator.clipboard.writeText(snippet.code)
      setStatus('copied')
    } catch {
      setStatus('error')
    }
    reset.current = window.setTimeout(() => setStatus('idle'), 2500)
  }
  return (
    <div className="min-w-0 self-start">
      <div className="swap-in overflow-hidden rounded-xl border bg-card">
        <div className="flex min-h-11 items-center justify-between border-b pr-1 pl-4">
          <span className="font-mono text-[13px] text-muted-foreground">{snippet.file}</span>
          <button
            type="button"
            onClick={copy}
            aria-label={`Copy ${snippet.file}`}
            className="inline-flex size-11 items-center justify-center rounded-md text-muted-foreground focus-visible:outline-2 focus-visible:outline-foreground focus-visible:outline-offset-2 [@media(hover:hover)]:hover:text-foreground"
          >
            {status === 'copied' ? <IconTickSquare className="size-4" /> : <IconCopy className="size-4" />}
          </button>
        </div>
        <section
          className="max-h-[480px] overflow-auto p-4 font-mono text-[13px] leading-[1.7] [scrollbar-width:thin]"
          // biome-ignore lint/a11y/noNoninteractiveTabindex: Keyboard users need to scroll this code region.
          tabIndex={0}
          aria-label={`${snippet.file} code`}
          // biome-ignore lint/security/noDangerouslySetInnerHtml: Shiki output is generated on the server from static strings.
          dangerouslySetInnerHTML={{ __html: snippet.html }}
        />
      </div>
      <output aria-live="polite" className="mt-2 block min-h-5 text-xs text-muted-foreground">
        {status === 'copied' ? 'Copied.' : status === 'error' ? 'Could not copy. Try selecting the code.' : ''}
      </output>
    </div>
  )
}

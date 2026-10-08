'use client'

import { AnimatePresence, motion } from 'motion/react'
import { type CSSProperties, useId, useState } from 'react'

import { IconCopy, IconTerminal, IconTickSquare } from '@/components/icons/iconly'
import type { CodeLine } from '@/lib/highlight'
import { duration, easeOut, spring } from '@/lib/motion'
import { cn } from '@/lib/utils'

export type InstallCommandLine = {
  /** The plain command, for the clipboard. */
  command: string
  /** The same command tokenized by Shiki (Vesper on dark), for the screen. */
  tokens: CodeLine
}

export type InstallTab = {
  id: string
  label: string
  commands: InstallCommandLine[]
}

type InstallCommandProps = {
  tabs: InstallTab[]
  className?: string
}

/**
 * The hero's call to action: one centred terminal card. A header row carries the terminal mark, a pill tab per
 * install path from iii.dev/docs/install, and one copy control; the selected path's commands sit underneath as
 * `$`-prompted, Shiki-highlighted lines. Nothing else: the command is the whole message.
 */
export function InstallCommand({ tabs, className }: InstallCommandProps) {
  const id = useId()
  const [activeId, setActiveId] = useState(tabs[0]?.id)
  const [copied, setCopied] = useState(false)
  const active = tabs.find((tab) => tab.id === activeId) ?? tabs[0]

  function select(tabId: string) {
    setActiveId(tabId)
    setCopied(false)
  }

  async function copy() {
    const text = active.commands.map((line) => line.command).join('\n')
    const written = await navigator.clipboard.writeText(text).then(
      () => true,
      () => false,
    )
    if (!written) return
    setCopied(true)
    window.setTimeout(() => setCopied(false), 2000)
  }

  return (
    <div className={cn('relative w-full min-w-0', className)}>
      {/* A faint pool of the hero accent under the card, so it reads as the lit object on the page. */}
      <div aria-hidden className="-inset-x-8 -top-6 -bottom-10 -z-10 pointer-events-none absolute" />
      <div className="overflow-hidden rounded-2xl border bg-card text-left shadow-[0_1px_0_0_oklch(1_0_0/6%)_inset,0_24px_60px_-28px_oklch(0_0_0/70%)]">
        {/* Header: terminal mark, install paths, copy */}
        <div className="flex h-11 items-center gap-2 border-b bg-faint px-2 sm:h-12 sm:px-3">
          <span
            aria-hidden
            className="flex size-7 shrink-0 items-center justify-center rounded-md border bg-background text-muted-foreground"
          >
            <IconTerminal className="size-3.5" />
          </span>
          <div
            role="tablist"
            aria-label="Install paths"
            className="flex min-w-0 flex-1 items-center gap-0.5 overflow-x-auto py-1 [scrollbar-width:none]"
          >
            {tabs.map((tab) => {
              const selected = tab.id === active.id
              return (
                <button
                  key={tab.id}
                  type="button"
                  role="tab"
                  id={`${id}-tab-${tab.id}`}
                  aria-selected={selected}
                  aria-controls={`${id}-panel-${tab.id}`}
                  tabIndex={selected ? 0 : -1}
                  onClick={() => select(tab.id)}
                  onKeyDown={(e) => {
                    if (e.key !== 'ArrowRight' && e.key !== 'ArrowLeft') return
                    e.preventDefault()
                    const index = tabs.findIndex((t) => t.id === active.id)
                    const next = tabs[(index + (e.key === 'ArrowRight' ? 1 : tabs.length - 1)) % tabs.length]
                    select(next.id)
                    document.getElementById(`${id}-tab-${next.id}`)?.focus()
                  }}
                  className={cn(
                    'relative flex h-8 shrink-0 items-center whitespace-nowrap rounded-lg px-3 font-sans text-[13.5px] outline-none transition-colors duration-150',
                    'focus-visible:text-foreground',
                    selected ? 'text-foreground' : 'text-muted-foreground hover:text-foreground',
                  )}
                >
                  {selected ? (
                    <motion.span
                      layoutId={`${id}-pill`}
                      aria-hidden
                      className="absolute inset-0 rounded-lg border bg-background shadow-[0_1px_2px_0_oklch(0_0_0/30%)]"
                      transition={spring.snappy}
                    />
                  ) : null}
                  <span className="relative">{tab.label}</span>
                </button>
              )
            })}
          </div>
          <button
            type="button"
            onClick={copy}
            aria-label={copied ? 'Copied' : 'Copy the commands'}
            className={cn(
              'pressable flex h-8 shrink-0 items-center gap-1.5 rounded-md px-2 font-sans text-[13px] outline-none',
              'focus-visible:outline-2 focus-visible:outline-foreground focus-visible:outline-offset-2',
              copied ? 'text-ok' : 'text-muted-foreground hover:bg-accent hover:text-foreground',
            )}
          >
            {/* The two states swap in place (scale 0.25 → 1, blur 4px → 0, a spring with no bounce) instead of
                waiting for one to leave before the other arrives. */}
            <AnimatePresence mode="popLayout" initial={false}>
              <motion.span
                key={copied ? 'done' : 'copy'}
                initial={{ opacity: 0, scale: 0.25, filter: 'blur(4px)' }}
                animate={{ opacity: 1, scale: 1, filter: 'blur(0px)' }}
                exit={{ opacity: 0, scale: 0.25, filter: 'blur(4px)' }}
                transition={{ type: 'spring', duration: 0.3, bounce: 0 }}
                className="flex items-center gap-1.5"
              >
                {copied ? <IconTickSquare className="size-4" /> : <IconCopy className="size-4" />}
                <span className="hidden sm:inline">{copied ? 'Copied' : 'Copy'}</span>
              </motion.span>
            </AnimatePresence>
          </button>
        </div>

        {/* From `sm` up every path's commands share one grid cell, so the card is always as tall as the longest path
            (the three-step `no llm`) and switching tabs never moves the page; the full command shows, wrapping if
            it must. Phones: only the selected path takes space, so a one-line `curl` gets a one-line card, and each
            command stays on one line while this area scrolls sideways under a fade on the right edge. */}
        <div className="grid overflow-x-auto overscroll-x-contain px-4 py-3 [mask-image:linear-gradient(to_right,black_calc(100%-40px),transparent)] [scrollbar-width:none] sm:overflow-visible sm:px-5 sm:py-5 sm:[mask-image:none] [&::-webkit-scrollbar]:hidden">
          {tabs.map((tab) => {
            const selected = tab.id === active.id
            return (
              <motion.ol
                key={tab.id}
                id={`${id}-panel-${tab.id}`}
                role="tabpanel"
                aria-labelledby={`${id}-tab-${tab.id}`}
                aria-hidden={!selected}
                inert={!selected}
                initial={false}
                animate={
                  selected
                    ? { opacity: 1, transform: 'translateY(0px)' }
                    : { opacity: 0, transform: 'translateY(-4px)' }
                }
                transition={{
                  duration: selected ? duration.base : duration.fast,
                  delay: selected ? duration.fast * 0.5 : 0,
                  ease: easeOut,
                }}
                className={cn(
                  'col-start-1 row-start-1 flex min-w-max flex-col gap-1.5 pr-10 sm:min-w-0 sm:gap-2.5 sm:pr-0',
                  !selected && 'pointer-events-none max-sm:hidden',
                )}
              >
                {tab.commands.map((line) => (
                  <li key={line.command} className="flex min-w-0 items-start gap-2.5 sm:gap-3">
                    <span
                      aria-hidden
                      className="select-none font-mono text-[13px] text-hero-accent leading-[1.7] sm:text-[15px]"
                    >
                      $
                    </span>
                    {/* Phones: one line, scrolled sideways. Wider: the full command, wrapping onto a second line if
                        it has to, never clipped. */}
                    <code className="code-tokens whitespace-pre font-mono text-[13px] text-foreground leading-[1.7] sm:min-w-0 sm:whitespace-pre-wrap sm:text-[15px] sm:[overflow-wrap:anywhere]">
                      {line.tokens.map((token, position) => (
                        // biome-ignore lint/suspicious/noArrayIndexKey: Tokens are static per command and never reorder.
                        <span key={position} style={token.style as CSSProperties}>
                          {token.text}
                        </span>
                      ))}
                    </code>
                  </li>
                ))}
              </motion.ol>
            )
          })}
        </div>
      </div>
    </div>
  )
}

import { Section } from '@lib/components/Section'
import { Button } from '@lib/components/schematic/Button'
import { Caret } from '@lib/components/schematic/Caret'
import { C, CodeBlock, K, M, S } from '@lib/components/schematic/CodeBlock'
import { Prompt } from '@lib/components/schematic/Prompt'
import { StatusDot } from '@lib/components/schematic/StatusDot'
import { StatusPanel } from '@lib/components/schematic/StatusPanel'
import { useEffect, useRef, useState } from 'react'

const INSTALL_LINES: Array<{ kind: 'cmd'; text: string; comment?: string } | { kind: 'out'; text: string }> = [
  { kind: 'cmd', text: 'iii worker add harness' },
  { kind: 'out', text: 'llm-router connected — providers self-register' },
  { kind: 'out', text: 'session-manager connected — 6 trigger types live' },
  { kind: 'out', text: 'context-manager connected' },
  { kind: 'out', text: 'harness connected — the loop is live' },
  { kind: 'cmd', text: 'iii worker add approval-gate', comment: '# optional' },
  { kind: 'out', text: 'approval-gate bound to harness::hook::pre_trigger' },
]

function InstallTerminal() {
  const [count, setCount] = useState(0)
  const [started, setStarted] = useState(false)
  const ref = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const el = ref.current
    if (!el) return
    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (entry.isIntersecting) {
            setStarted(true)
            observer.disconnect()
          }
        }
      },
      { rootMargin: '0px 0px -15% 0px' },
    )
    observer.observe(el)
    return () => observer.disconnect()
  }, [])

  useEffect(() => {
    if (!started || count >= INSTALL_LINES.length) return
    const line = INSTALL_LINES[count]
    const delay = line.kind === 'cmd' ? 620 : 340
    const t = setTimeout(() => setCount((c) => c + 1), delay)
    return () => clearTimeout(t)
  }, [started, count])

  const finished = count >= INSTALL_LINES.length

  return (
    <div ref={ref} className="border border-rule bg-bg">
      <div className="flex items-center justify-between bg-panel px-3.5 py-2 border-b border-rule">
        <span className="font-mono text-[11px] font-medium uppercase tracking-[0.06em] text-ink-faint">
          a complete agent backend — one command
        </span>
        {finished ? (
          <Button variant="ghost" size="sm" className="h-6 px-2 text-[11px]" onClick={() => setCount(0)}>
            replay
          </Button>
        ) : (
          <span className="flex items-center gap-x-1.5">
            <StatusDot pulse />
            <span className="font-mono text-[10px] uppercase tracking-[0.06em] text-ink-faint">installing</span>
          </span>
        )}
      </div>
      <div className="p-4 font-mono text-[13px] min-h-[220px]">
        <div className="flex flex-col gap-y-1.5">
          {INSTALL_LINES.slice(0, count).map((line, i) =>
            line.kind === 'cmd' ? (
              <div key={i} className="flex items-center gap-x-2">
                <Prompt symbol="$" />
                <span className="text-ink">{line.text}</span>
                {line.comment ? <span className="text-ink-ghost text-[12px]">{line.comment}</span> : null}
              </div>
            ) : (
              <div key={i} className="pl-4 text-[12.5px] text-ink-faint flex items-center gap-x-2">
                <span className="text-accent">✓</span>
                <span>{line.text}</span>
              </div>
            ),
          )}
          <div className="flex items-center gap-x-2">
            <Prompt symbol="$" />
            <Caret />
          </div>
        </div>
      </div>
    </div>
  )
}

export function OnboardingSection() {
  return (
    <Section
      id="install"
      index="10"
      eyebrow="onboarding"
      title="from zero to a live agent surface in three steps."
      lede="no scaffolding, no config sprawl, no services to babysit. each worker is one command; the set replays anywhere from the lockfile. and a fresh install is safe by default — it dispatches nothing until you allow it."
    >
      <div className="grid grid-cols-1 @4xl:grid-cols-2 gap-4 items-start">
        <div className="flex flex-col gap-4">
          <InstallTerminal />
          <StatusPanel
            variant="success"
            headline="replayable installs"
            detail="commit iii.lock, run `iii worker sync` on any engine — same workers, same versions, zero drift."
          />
        </div>

        <div className="flex flex-col gap-4 min-w-0">
          <CodeBlock title="step 2 — send (any language, any worker)">
            <K>await</K> iii.<K>trigger</K>(<M>{'{'}</M>
            {'\n'}
            {'  '}function_id: <S>"harness::send"</S>,{'\n'}
            {'  '}payload: <M>{'{'}</M>
            {'\n'}
            {'    '}message: <S>"summarise this repo's readme"</S>,{'\n'}
            {'    '}model: <S>"claude-sonnet-4"</S>,{'\n'}
            {'    '}options: <M>{'{'}</M> functions: <M>{'{'}</M> allow: [<S>"shell::*"</S>] <M>{'}'}</M> <M>{'}'}</M>
            {'\n'}
            {'  '}
            <M>{'}'}</M>
            {'\n'}
            <M>{'}'}</M>);
            {'\n'}
            <C>// → {'{ session_id, turn_id, accepted: true }'}</C>
          </CodeBlock>

          <CodeBlock title="step 3 — bind, and you are live">
            <C>// reactive: one binding renders every delta</C>
            {'\n'}
            iii.<K>registerFunction</K>(<S>"app::render"</S>, paintMessage);
            {'\n'}
            iii.<K>registerTrigger</K>(<M>{'{'}</M>
            {'\n'}
            {'  '}type: <S>"session::message-updated"</S>,{'\n'}
            {'  '}function_id: <S>"app::render"</S>,{'\n'}
            <M>{'}'}</M>);
          </CodeBlock>
        </div>
      </div>

      <div className="mt-6 grid grid-cols-1 @2xl:grid-cols-3 gap-px bg-rule border border-rule">
        {[
          {
            title: 'standalone by contract',
            body: 'need only provider routing? install llm-router alone. just a reactive transcript store? session-manager alone. the loop is a composition, not a prerequisite.',
          },
          {
            title: 'deny-all until told otherwise',
            body: 'a fresh harness refuses every dispatch with a readable error. capability is a deliberate allow-list, reviewed like code.',
          },
          {
            title: 'your stack, your languages',
            body: 'workers register from typescript, python, or rust over one socket — the presentation you are reading describes every one of them with the same five concepts.',
          },
        ].map((cell) => (
          <div key={cell.title} className="bg-bg p-6">
            <div className="font-mono text-[15px] font-semibold lowercase text-ink mb-2">{cell.title}</div>
            <p className="font-mono text-[12.5px] leading-[1.7] text-ink-faint lowercase">{cell.body}</p>
          </div>
        ))}
      </div>
    </Section>
  )
}

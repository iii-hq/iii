'use client'

import { ArrowRightIcon, ArrowUpIcon, ChevronDownIcon, MicIcon, PlusIcon } from 'lucide-react'
import { useEffect, useState } from 'react'

import { DemoPlayback } from '@/components/graphics/demo-playback'
import { IconBot, IconCheckCircle, IconSearch } from '@/components/icons/iconly'
import { Reveal } from '@/components/site/reveal'
import { buttonVariants } from '@/components/ui/button'
import { useDemoPlayback } from '@/hooks/use-demo-playback'
import { cn } from '@/lib/utils'
import { proof } from './content'
import styles from './proof.module.css'
import {
  BEAT_MS,
  CALL_AT,
  CALLS,
  CONTEXT_K,
  DONE_AT,
  ELAPSED,
  PROMPT,
  REPLY,
  REPLY_AT,
  REST_MS,
  RUN_SHARE,
  SEARCH,
  SEARCH_AT,
  TURN_MS,
} from './proof-session'
import { ProofTraces } from './proof-traces'
import { Section } from './section'
import { typed, useBeatClock } from './use-beat-clock'

/**
 * Proof: the harness in use, drawn as the ADE app (2026-10-05 sync: no graph, show the actual harness UI, bigger).
 * Left, the conversations; centre, the session: the prompt, the function search, each call as it runs, and the reply;
 * right, the inspector with the page's one trace view and the context window. The two claims in the copy are shown,
 * not told: discovery returns in milliseconds, and only the matched schemas enter the context.
 */

type CallState = 'waiting' | 'running' | 'done'

function useSession() {
  const { ref, running, paused, setPaused, reduce } = useDemoPlayback<HTMLDivElement>()
  const [beat, setBeat] = useState(0)
  useEffect(() => {
    if (!running) return
    const id = window.setTimeout(
      () => setBeat((current) => (current >= DONE_AT ? 0 : current + 1)),
      beat >= DONE_AT ? REST_MS : BEAT_MS[beat],
    )
    return () => window.clearTimeout(id)
  }, [running, beat])
  const at = reduce ? DONE_AT : beat
  const ms = useBeatClock(at, running && !reduce)
  return { ref, at, ms, running, paused, setPaused, reduce }
}

/** A search or call row: waiting before its beat, running for the first part of it, then done. */
function stepState(at: number, stepAt: number, ms: number): CallState {
  if (at < stepAt) return 'waiting'
  if (at > stepAt) return 'done'
  return ms < BEAT_MS[stepAt] * RUN_SHARE ? 'running' : 'done'
}

export function Proof() {
  const { ref, at, ms, running, paused, setPaused, reduce } = useSession()
  const search = stepState(at, SEARCH_AT, ms)
  const calls = CALLS.map((_, i) => stepState(at, CALL_AT + i, ms))
  const prompt = at === 0 ? typed(PROMPT, ms, 24) : ''
  const reply = at < REPLY_AT ? '' : at === REPLY_AT ? typed(REPLY, ms, 11) : REPLY
  const working = at >= 1 && at < DONE_AT
  const done = at >= DONE_AT || (at === REPLY_AT && reply.length === REPLY.length)
  const context = CONTEXT_K[at] ?? CONTEXT_K[CONTEXT_K.length - 1]
  const activeCall = at >= CALL_AT && at < REPLY_AT ? CALLS[at - CALL_AT].fn : undefined

  return (
    // biome-ignore lint/correctness/useUniqueElementIds: One stable anchor per section on this page.
    <Section id="proof" eyebrow={proof.eyebrow} title={proof.title} lede={proof.subtitle}>
      <Reveal className={styles.stage}>
        <div ref={ref} className={styles.app}>
          {/* Title bar: the app, the session, its status, and the one control. */}
          <div className={styles.bar}>
            <span className={styles.appMark}>
              <IconBot className="size-4" />
            </span>
            <span className={styles.appName}>ADE</span>
            <span aria-hidden className={styles.slash}>
              /
            </span>
            <span className={styles.sessionName}>Fix staging checkout</span>
            <span className={styles.status} data-state={done ? 'done' : working ? 'working' : 'idle'}>
              <span aria-hidden />
              {done ? 'Done' : working ? 'Working' : 'Ready'}
            </span>
            <DemoPlayback paused={paused} reduce={reduce} onToggle={() => setPaused(!paused)} />
          </div>

          <div className={styles.grid}>
            {/* Conversations */}
            <aside className={styles.sidebar} aria-label="Conversations">
              <p className={styles.newChat}>New chat</p>
              <p className={styles.sideLabel}>Conversations</p>
              <ul className={styles.conversations}>
                <li data-active>
                  <span>Fix staging checkout</span>
                  <span className={styles.live} data-on={working} aria-hidden />
                </li>
                <li>
                  <span>Release digest</span>
                </li>
                <li>
                  <span>Payments ledger</span>
                </li>
              </ul>
            </aside>

            {/* Session */}
            <section className={styles.session} aria-label="Harness session">
              <div className={styles.thread}>
                <div className={styles.userMessage} data-visible={at >= 1}>
                  <p>{PROMPT}</p>
                </div>

                <div className={styles.step} data-state={search}>
                  <span className={styles.stepIcon} aria-hidden>
                    <span className={styles.spinner} />
                    <IconSearch className={cn(styles.stepDone, 'size-3.5')} />
                  </span>
                  <p className={styles.stepTitle}>
                    {search === 'done' ? 'Found 5 functions' : 'Searching functions'}
                    <code>{SEARCH.fn}</code>
                  </p>
                  <span className={styles.stepMs}>{search === 'done' ? `${SEARCH.ms} ms` : ''}</span>
                  <ul className={styles.found} data-visible={search === 'done'}>
                    {CALLS.map((call, i) => (
                      <li key={call.fn} style={{ transitionDelay: at === SEARCH_AT ? `${i * 50}ms` : undefined }}>
                        <code>{call.fn}</code>
                      </li>
                    ))}
                  </ul>
                </div>

                <ol className={styles.calls} data-visible={at >= CALL_AT}>
                  {CALLS.map((call, i) => (
                    <li key={call.fn} className={styles.call} data-state={calls[i]}>
                      <span className={styles.stepIcon} aria-hidden>
                        <span className={styles.spinner} />
                        <IconCheckCircle className={cn(styles.stepDone, styles.callCheck, 'size-4')} />
                      </span>
                      <p className={styles.callHead}>
                        <code>{call.fn}</code>
                        <span>{call.where}</span>
                      </p>
                      <span className={styles.stepMs}>
                        {calls[i] === 'done' ? `${call.ms.toLocaleString()} ms` : ''}
                      </span>
                      <p className={styles.callResult}>{call.result}</p>
                    </li>
                  ))}
                </ol>

                <div className={styles.reply} data-visible={at >= REPLY_AT}>
                  <span className={styles.replyMark} aria-hidden>
                    <IconBot className="size-3.5" />
                  </span>
                  <p>
                    <span aria-hidden className={styles.replyGhost}>
                      {REPLY}
                    </span>
                    <span className={styles.replyText}>
                      {reply}
                      {at === REPLY_AT && reply.length < REPLY.length ? (
                        <span aria-hidden className={styles.caret} />
                      ) : null}
                    </span>
                  </p>
                </div>
              </div>

              {/* The composer, as ADE draws it: a raised card, the text on top, the model and actions below. */}
              <div className={styles.composer} aria-hidden>
                <p className={styles.composerText} data-empty={!prompt}>
                  {prompt || (working ? 'streaming response…' : 'Ask to make changes, @mention files, run /commands')}
                  {prompt ? <span className={styles.caret} /> : null}
                </p>
                <div className={styles.composerBar}>
                  <span className={styles.modelPicker}>
                    Claude Sonnet 4.5
                    <span className={styles.effort}>High</span>
                    <ChevronDownIcon strokeWidth={1.75} className="size-3.5" />
                  </span>
                  <span className={styles.composerActions}>
                    <span className={styles.toolButton}>
                      <PlusIcon strokeWidth={1.75} className="size-4" />
                    </span>
                    <span className={styles.toolButton}>
                      <MicIcon strokeWidth={1.75} className="size-4" />
                    </span>
                    <span className={styles.send} data-ready={Boolean(prompt)}>
                      <ArrowUpIcon strokeWidth={2.25} className="size-4" />
                    </span>
                  </span>
                </div>
              </div>
            </section>

            {/* Inspector */}
            <aside className={styles.inspector} aria-label="Inspector">
              <ProofTraces
                elapsed={ELAPSED[at] ?? TURN_MS}
                active={activeCall}
                running={running && !reduce}
                stepMs={BEAT_MS[at] ?? 0}
                discovery={search === 'done' ? SEARCH.ms : undefined}
                context={context}
              />
            </aside>
          </div>
        </div>
        <p className="sr-only">
          Sample session: the prompt “{PROMPT}” runs directory::search_functions, which finds five functions in 3
          milliseconds. The harness then calls browser::navigate, sandbox::exec, router::chat, state::set and
          github::pr::create through iii, and replies: {REPLY}
        </p>

        <div className={styles.after}>
          <p>{proof.solution}</p>
          <a
            href={proof.cta.href}
            className={cn(buttonVariants(), 'group h-11 w-fit gap-2.5 rounded-xl px-5 pr-4 text-[15px]')}
          >
            {proof.cta.label}
            <ArrowRightIcon
              aria-hidden
              strokeWidth={1.75}
              className="size-4 transition-transform duration-150 group-hover:translate-x-0.5 motion-reduce:transition-none"
            />
          </a>
        </div>
      </Reveal>
    </Section>
  )
}

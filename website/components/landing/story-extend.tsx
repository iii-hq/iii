'use client'

import { IconCheckCircle, IconPackage, IconProfile } from '@/components/icons/iconly'
import { cn } from '@/lib/utils'
import shared from './story.module.css'
import { CommandLine, commandShown, commandTypeMs } from './story-command'
import styles from './story-extend.module.css'
import { STORY_BEATS } from './story-model'
import { typed, useBeatClock } from './use-beat-clock'

/**
 * Extensibility in one column, read top to bottom: someone asks for what the system is missing, Compose answers,
 * one `compose::add` line runs, and the workers join as a dependency tree (the way a package manager prints a
 * resolve). storage arrives alone; harness pulls in its dependencies, and llm-router pulls in the providers it
 * routes to. The header counts the graph as it grows. Names and dependencies are the real registry manifests.
 */

const ASK = 'Add storage, and give this system an agent harness.'
const REPLY = 'Adding both. harness depends on ten other workers, so they join with it.'
const COMMAND = 'iii trigger compose::add worker=storage worker=harness'
const ASK_CHAR_MS = 16
/** Workers already in the graph from the chapters before (http, api, database, iii-directory). */
const BASE_WORKERS = 4

const PROVIDERS = ['provider-anthropic', 'provider-openai', 'provider-openai-codex'] as const
const SUPPORT = ['state', 'queue', 'session-manager', 'context-manager', 'judge', 'configuration'] as const

/** Beat on which each part of the tree joins. */
const AT = { storage: 3, harness: 4, deps: 5, providers: 6 } as const
/** Stagger inside a beat (30–80ms per Emil): the dependency chips, then the providers. */
const CHIP_STAGGER = 40
const PROVIDER_STAGGER = 50

export function ExtendPreview({ beat, running }: { beat: number; running: boolean }) {
  const ms = useBeatClock(beat, running)
  const ask = beat === 0 ? typed(ASK, ms, ASK_CHAR_MS) : ASK
  const accepted = beat > 2 || (beat === 2 && ms >= commandTypeMs(COMMAND) + 120)
  /* Whether something that joins on beat `at`, `delay` ms into it, has landed. Drives the rows and the count
     together, so the number ticks up exactly as each worker appears. */
  const arrived = (at: number, delay = 0) => beat > at || (beat === at && ms >= delay)
  const count =
    BASE_WORKERS +
    Number(arrived(AT.storage)) +
    Number(arrived(AT.harness)) +
    Number(arrived(AT.deps)) +
    SUPPORT.filter((_, i) => arrived(AT.deps, (i + 1) * CHIP_STAGGER)).length +
    PROVIDERS.filter((_, i) => arrived(AT.providers, i * PROVIDER_STAGGER)).length
  const caption = STORY_BEATS.extend[beat].caption

  return (
    <figure className={cn(shared.activity, styles.preview)} data-beat={beat}>
      <figcaption className={shared.activityHeader}>
        <span className={shared.activityIcon}>
          <IconPackage className="size-4" />
        </span>
        <span className={shared.activityLabel}>Extend</span>
        <span className={shared.activityStatus}>
          Graph ·{' '}
          <span key={count} className={cn(styles.count, 'swap-in')}>
            {count}
          </span>{' '}
          workers
        </span>
      </figcaption>

      <div className={styles.body}>
        {/* 1 · The ask and the answer. */}
        <div className={styles.chat}>
          <div className={styles.message}>
            <span className={styles.avatar}>
              <IconProfile className="size-3.5" />
            </span>
            <p className={styles.bubble}>
              <span aria-hidden className={styles.ghost}>
                {ASK}
              </span>
              <span className={styles.typedText}>
                {ask}
                {ask.length < ASK.length ? <span aria-hidden className={styles.caret} /> : null}
              </span>
            </p>
          </div>
          <div className={styles.message} data-from="compose" data-visible={beat >= 1}>
            <span className={cn(styles.avatar, styles.avatarIii)} aria-hidden>
              iii
            </span>
            <p className={styles.bubble}>
              <span className={styles.sender}>Compose</span>
              {REPLY}
            </p>
          </div>
        </div>

        {/* 2 · The one command it runs. */}
        <div className={styles.run} data-visible={beat >= 2}>
          <CommandLine command={COMMAND} shown={beat < 2 ? 0 : beat === 2 ? commandShown(ms) : undefined} />
          <p className={styles.response} data-visible={accepted}>
            status: accepted · requested: 2
          </p>
        </div>

        {/* 3 · Who joins: a resolve tree. Every row is laid out from the start so nothing shifts as it fills. */}
        <ul className={styles.tree} aria-label="Workers joining the graph">
          <TreeRow joined={arrived(AT.storage)} name="storage" note="No dependencies" />
          <TreeRow joined={arrived(AT.harness)} name="harness" note="Agent runtime · 10 dependencies" />
          <TreeRow joined={arrived(AT.deps)} name="llm-router" note="Routes model calls" depth={1} branch="mid" />
          {PROVIDERS.map((name, i) => (
            <TreeRow
              key={name}
              joined={arrived(AT.providers, i * PROVIDER_STAGGER)}
              name={name}
              depth={2}
              branch={i === PROVIDERS.length - 1 ? 'last' : 'mid'}
              rail
            />
          ))}
          <li className={styles.row} data-depth={1} data-branch="last" data-joined={arrived(AT.deps)}>
            <span className={styles.chips}>
              {SUPPORT.map((name, i) => (
                <code key={name} className={styles.chip} data-joined={arrived(AT.deps, (i + 1) * CHIP_STAGGER)}>
                  {name}
                </code>
              ))}
            </span>
          </li>
        </ul>
      </div>

      <div className={shared.activityFooter}>
        <span key={caption} className="swap-in">
          {caption}
        </span>
      </div>
    </figure>
  )
}

function TreeRow({
  joined,
  name,
  note,
  depth = 0,
  branch,
  rail = false,
}: {
  joined: boolean
  name: string
  note?: string
  depth?: number
  branch?: 'mid' | 'last'
  /** Draws the parent's vertical line through this row (a grandchild under a branch that continues). */
  rail?: boolean
}) {
  return (
    <li className={styles.row} data-depth={depth} data-branch={branch} data-rail={rail} data-joined={joined}>
      <code className={styles.name}>{name}</code>
      <span className={styles.note}>{note}</span>
      <span className={styles.state} aria-hidden>
        <IconCheckCircle className="size-4" />
      </span>
      <span className="sr-only">{joined ? 'joined' : 'waiting'}</span>
    </li>
  )
}

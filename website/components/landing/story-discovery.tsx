'use client'

import { IconCheckCircle, IconCode, IconPackage, IconSearch } from '@/components/icons/iconly'
import { cn } from '@/lib/utils'
import shared from './story.module.css'
import styles from './story-discovery.module.css'
import { STORY_BEATS } from './story-model'
import { typed, useBeatClock } from './use-beat-clock'

/**
 * Discoverability as a search the reader can follow, in the order iii's harness runs it: the prompt is split into
 * capabilities by `directory::search_functions`, each capability is searched against what is already running, then
 * the worker registry, and when neither has it the answer is to write it. Three needs, three outcomes: call one,
 * install one, write one. Colour stays neutral; the accent only marks something ready.
 */

type Scope = 'system' | 'registry'
type Row = { id: string; detail: string; owner: string }

const PROMPT = 'Save this order, archive its receipt, and apply our loyalty discount.'
const SYSTEM: Row[] = [
  { id: 'orders::create', detail: 'Create an order', owner: 'api · Python' },
  { id: 'database::execute', detail: 'Run SQL against the database', owner: 'database · Rust' },
  { id: 'directory::search_functions', detail: 'Search registered functions', owner: 'iii-directory' },
]
const REGISTRY: Row[] = [
  { id: 'document', detail: 'Read and write documents', owner: 'workers.iii.dev' },
  { id: 'storage', detail: 'Store and archive objects', owner: 'workers.iii.dev' },
  { id: 'browser', detail: 'Drive a real browser', owner: 'workers.iii.dev' },
]
const NEEDS = ['Save an order', 'Archive a receipt', 'Apply loyalty discount'] as const
const OUTCOMES = [
  { need: NEEDS[0], result: 'database::execute', note: 'Installed · call it now', icon: IconCheckCircle, at: 2 },
  { need: NEEDS[1], result: 'storage', note: 'Install it with compose::add', icon: IconPackage, at: 4 },
  { need: NEEDS[2], result: 'loyalty::apply', note: 'Write it as your own worker', icon: IconCode, at: 7 },
] as const

/** Per beat: the need being searched, where, whether its query types in, the rows walked, and the match (null: none). */
type Search = { need: number; scope: Scope; type: boolean; path: number[]; match: number | null }
const SEARCHES: Record<number, Search> = {
  2: { need: 0, scope: 'system', type: true, path: [0, 1], match: 1 },
  3: { need: 1, scope: 'system', type: true, path: [0, 1, 2], match: null },
  4: { need: 1, scope: 'registry', type: false, path: [0, 1], match: 1 },
  5: { need: 2, scope: 'system', type: true, path: [0, 1, 2], match: null },
  6: { need: 2, scope: 'registry', type: false, path: [0, 1, 2], match: null },
}
const PROMPT_CHAR_MS = 22
const QUERY_CHAR_MS = 34
const SCAN_PAUSE_MS = 160
const SCAN_STEP_MS = 230

/** The beat's own timeline: type the query, pause, walk the rows one at a time, then settle on the outcome. */
function searchFrame(search: Search | undefined, ms: number) {
  if (!search) return { query: '', typing: false, row: null as number | null, settled: false }
  const query = NEEDS[search.need].toLowerCase()
  const typeMs = search.type ? query.length * QUERY_CHAR_MS : 0
  const step = Math.floor((ms - typeMs - SCAN_PAUSE_MS) / SCAN_STEP_MS)
  const settled = step > search.path.length - 1
  const row = step < 0 ? null : settled ? search.match : search.path[step]
  return { query: search.type ? typed(query, ms, QUERY_CHAR_MS) : query, typing: ms < typeMs, row, settled }
}

export function DiscoverPreview({ beat, running }: { beat: number; running: boolean }) {
  const ms = useBeatClock(beat, running)
  const search = SEARCHES[beat]
  const frame = searchFrame(search, ms)
  const writing = beat >= 7
  const scope: Scope = search?.scope ?? (writing ? 'registry' : 'system')
  const rows = scope === 'system' ? SYSTEM : REGISTRY
  const matched = frame.settled && search?.match != null
  const missed = (frame.settled && search?.match === null) || writing
  const prompt = beat === 0 ? typed(PROMPT, ms, PROMPT_CHAR_MS) : PROMPT
  const idle = !search && !writing
  const caption = STORY_BEATS.discover[beat].caption

  return (
    <figure className={cn(shared.activity, styles.preview)} data-beat={beat}>
      <figcaption className={shared.activityHeader}>
        <span className={shared.activityIcon}>
          <IconSearch className="size-4" />
        </span>
        <span className={shared.activityLabel}>Discover</span>
        <code className={styles.fn}>directory::search_functions</code>
      </figcaption>

      <div className={styles.body}>
        {/* 1 · The prompt, then the capabilities the search splits it into. */}
        <div>
          <p className={styles.label}>Prompt to your harness</p>
          <p className={styles.prompt}>
            {/* The full prompt holds the space while the typed copy draws over it, so nothing below moves. */}
            <span aria-hidden className={styles.promptGhost}>
              {PROMPT}
            </span>
            <span className={styles.promptTyped}>
              {prompt}
              {prompt.length < PROMPT.length ? <span aria-hidden className={styles.caret} /> : null}
            </span>
          </p>
          <ol className={styles.needs} data-visible={beat >= 1}>
            {NEEDS.map((need, i) => (
              <li
                key={need}
                data-current={search?.need === i}
                style={{ transitionDelay: beat === 1 ? `${i * 70}ms` : undefined }}
              >
                <span className={styles.needIndex}>{i + 1}</span>
                {need}
              </li>
            ))}
          </ol>
        </div>

        {/* 2 · The search: the query types in, then the highlight walks the list it is searching. */}
        <div className={styles.search}>
          <div className={styles.searchBar}>
            <IconSearch className="size-4 shrink-0 text-muted-foreground" />
            <span className={styles.query} data-idle={idle}>
              {search ? frame.query : writing ? NEEDS[2].toLowerCase() : 'Waiting for a prompt'}
              {frame.typing ? <span aria-hidden className={styles.caret} /> : null}
            </span>
            <span className={styles.scopes} data-scope={scope} data-idle={idle}>
              <span aria-hidden className={styles.scopePill} />
              <span data-on={scope === 'system'}>Your system</span>
              <span data-on={scope === 'registry'}>Registry</span>
            </span>
          </div>
          <div className={styles.results}>
            <div key={scope} className={cn(styles.rowsFrame, 'swap-in')}>
              <span
                aria-hidden
                className={styles.cursor}
                data-visible={frame.row != null}
                data-matched={matched}
                style={{ transform: `translateY(${(frame.row ?? 0) * 100}%)` }}
              />
              <ul className={styles.rows}>
                {rows.map((item, i) => {
                  const isMatch = matched && search?.match === i
                  return (
                    <li key={item.id} className={styles.row} data-dim={matched && !isMatch}>
                      <code className={styles.rowId}>{item.id}</code>
                      <span className={styles.rowDetail}>{item.detail}</span>
                      <span className={styles.rowOwner}>
                        {isMatch ? (
                          <span key="match" className={cn(styles.matchTag, 'swap-in')}>
                            {scope === 'system' ? 'Installed' : 'Installable'}
                          </span>
                        ) : (
                          item.owner
                        )}
                      </span>
                    </li>
                  )
                })}
              </ul>
            </div>
            <p className={styles.noMatch} data-visible={missed}>
              {scope === 'system' ? 'Nothing running matches. Checking the registry.' : 'No worker does this yet.'}
            </p>
          </div>
        </div>

        {/* 3 · What the search produced: call one, install one, write one. */}
        <ol className={styles.plan}>
          {OUTCOMES.map((item, i) => {
            const done = beat >= item.at
            const Icon = item.icon
            return (
              <li key={item.need} data-ready={done}>
                <span className={styles.planState} aria-hidden>
                  <span className={styles.needIndex}>{i + 1}</span>
                  <Icon className={cn(styles.planIcon, i === 0 && styles.planIconReady, 'size-[18px]')} />
                </span>
                <span className={styles.planWhat}>
                  {done ? (
                    <code key="result" className="swap-in">
                      {item.result}
                    </code>
                  ) : (
                    <span>{item.need}</span>
                  )}
                </span>
                <span
                  key={done ? 'done' : search?.need === i ? 'search' : 'wait'}
                  className={cn(styles.planNote, 'swap-in')}
                >
                  {done ? item.note : search?.need === i ? 'Searching' : 'Waiting'}
                </span>
              </li>
            )
          })}
        </ol>
      </div>

      <div className={shared.activityFooter}>
        <span key={caption} className="swap-in">
          {caption}
        </span>
      </div>
      <p className="sr-only">
        Example: the prompt “{PROMPT}” needs three capabilities. Searching the running system finds database::execute,
        ready to call. Nothing running archives files, so the search checks the worker registry and finds storage, which
        Compose can install. Nothing anywhere applies a loyalty discount, so you write it as your own worker.
      </p>
    </figure>
  )
}

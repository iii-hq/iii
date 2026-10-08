'use client'

import { IconCheckCircle, IconGitHub } from '@/components/icons/iconly'
import { cn } from '@/lib/utils'
import shared from './story.module.css'
import { STORY_BEATS } from './story-model'
import styles from './story-react.module.css'

/**
 * Reactivity as triggers, drawn as one fan-out read top to bottom: a pull request opens, the github worker emits
 * `github::pr::event` into iii, iii delivers it to every subscriber at once, and each handler reacts on its own.
 * Then a fourth handler subscribes to the same event without anything upstream changing.
 */

const HANDLERS = [
  { id: 'review', name: 'Review bot', fn: 'review::start', result: 'Review started' },
  { id: 'notify', name: 'Notify team', fn: 'notify::team', result: '#checkout pinged' },
  { id: 'ci', name: 'CI checks', fn: 'ci::run', result: '48 tests running' },
  { id: 'deploy', name: 'Preview deploy', fn: 'deploy::preview', result: 'Subscribed' },
] as const

/** Beats: 1 PR opens, 2 event reaches iii, 3 delivered to subscribers, 4 handlers react, 5 a fourth subscribes. */
const AT = { opened: 1, emitted: 2, delivered: 3, reacted: 4, added: 5 } as const

export function ReactPreview({ beat }: { beat: number }) {
  const caption = STORY_BEATS.react[beat].caption
  const opened = beat >= AT.opened
  const subscribed = (i: number) => i < 3 || beat >= AT.added
  const delivered = (i: number) => beat >= AT.delivered && i < 3
  const reacted = (i: number) => beat >= AT.reacted && i < 3

  return (
    <figure className={cn(shared.activity, styles.preview)} data-beat={beat}>
      <figcaption className={shared.activityHeader}>
        <span className={shared.activityIcon}>
          <IconGitHub className="size-4" />
        </span>
        <span className={shared.activityLabel}>Triggers</span>
        <code className={styles.fn}>github::pr::event</code>
      </figcaption>

      <div className={styles.body}>
        {/* 1 · The event source: a pull request on GitHub. */}
        <div className={styles.source} data-opened={opened}>
          <IconGitHub className="size-5 shrink-0" />
          <div className="min-w-0">
            <p className={styles.sourceTitle}>Add retry to checkout</p>
            <p className={styles.sourceMeta}>iii-hq/shop · pull request #482</p>
          </div>
          <span key={opened ? 'open' : 'draft'} className={cn(styles.badge, 'swap-in')} data-opened={opened}>
            {opened ? 'Opened' : 'Draft'}
          </span>
        </div>

        {/* 2 · The github worker emits the event into iii. */}
        <div className={styles.stem} data-lit={beat >= AT.emitted} aria-hidden>
          <span />
        </div>
        <div className={styles.hub} data-lit={beat >= AT.emitted}>
          <span className={styles.hubMark} aria-hidden>
            iii
          </span>
          <code>github::pr::event</code>
          <span className={styles.hubNote}>{beat >= AT.added ? '4 subscribers' : '3 subscribers'}</span>
        </div>

        {/* 3 · iii delivers it to every subscriber at once. */}
        <div className={styles.fan} aria-hidden data-added={beat >= AT.added}>
          <span className={styles.fanStem} data-lit={beat >= AT.delivered} />
          <span className={styles.rail} />
          <span className={styles.railNext} />
          <span className={cn(styles.railLit, styles.railLeft)} data-lit={beat >= AT.delivered} />
          <span className={cn(styles.railLit, styles.railRight)} data-lit={beat >= AT.delivered} />
          {HANDLERS.map((handler, i) => (
            <span
              key={handler.id}
              className={styles.drop}
              data-subscribed={subscribed(i)}
              data-lit={delivered(i)}
              style={{ left: `${12.5 + i * 25}%` }}
            />
          ))}
        </div>

        {/* 4 · Each handler reacts on its own. The fourth slot fills when a new handler subscribes. */}
        <ol className={styles.handlers}>
          {HANDLERS.map((handler, i) => {
            const on = subscribed(i)
            const done = reacted(i)
            return (
              <li
                key={handler.id}
                className={styles.handler}
                data-subscribed={on}
                data-reacted={done}
                style={{ transitionDelay: beat === AT.reacted ? `${i * 60}ms` : undefined }}
              >
                {on ? (
                  <>
                    <p className={styles.handlerName}>{handler.name}</p>
                    <code className={styles.handlerFn}>{handler.fn}</code>
                    <p className={styles.handlerState}>
                      <span className={styles.stateIcon} aria-hidden>
                        <span className={styles.listening} />
                        <IconCheckCircle className={cn(styles.check, 'size-3.5')} />
                      </span>
                      <span key={done ? 'done' : 'wait'} className="swap-in">
                        {done ? handler.result : i === 3 ? 'Subscribed' : 'Listening'}
                      </span>
                    </p>
                  </>
                ) : (
                  <p className={styles.empty}>
                    <span aria-hidden>+</span> Add a handler
                  </p>
                )}
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
        Example: a pull request opens and the github worker emits github::pr::event through iii. A review bot, a team
        notifier and CI checks each subscribe to that event and react independently. A fourth handler, a preview deploy,
        subscribes later without any change to the event source.
      </p>
    </figure>
  )
}

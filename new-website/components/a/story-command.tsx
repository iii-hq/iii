import styles from './story.module.css'

/** Milliseconds per character when a command types in. */
export const COMMAND_CHAR_MS = 14

type Segment = { text: string; tone: 'binary' | 'muted' | 'value' }

/** `iii trigger compose::add worker=x` → coloured segments: the binary and function bright, flags muted, values warm. */
function segments(command: string): Segment[] {
  const [binary, subcommand, fn, ...args] = command.split(' ')
  const out: Segment[] = [
    { text: binary, tone: 'binary' },
    { text: ` ${subcommand}`, tone: 'muted' },
    { text: ` ${fn}`, tone: 'binary' },
  ]
  for (const arg of args) {
    const [key, value] = arg.split('=')
    out.push({ text: ` ${key}=`, tone: 'muted' }, { text: value ?? '', tone: 'value' })
  }
  return out
}

const TONE = { binary: styles.cmdBinary, muted: styles.cmdMuted, value: styles.cmdValue } as const

/**
 * One shell line in a recessed code block, typed in character by character. `shown` is how many characters are
 * visible (Infinity for all). A caret blinks after the prompt while the line is empty and rides the end while typing.
 */
export function CommandLine({ command, shown = Number.POSITIVE_INFINITY }: { command: string; shown?: number }) {
  let budget = shown
  const typing = shown < command.length
  return (
    <p className={styles.command}>
      <span aria-hidden className={styles.prompt}>
        $
      </span>
      <code className={styles.commandText}>
        <span className="sr-only">{command}</span>
        {segments(command).map((segment, i) => {
          const visible = segment.text.slice(0, Math.max(0, budget))
          budget -= segment.text.length
          return visible ? (
            // biome-ignore lint/suspicious/noArrayIndexKey: Segments are positional and never reorder.
            <span key={i} className={TONE[segment.tone]} aria-hidden>
              {visible}
            </span>
          ) : null
        })}
        {typing ? <span aria-hidden className={styles.caret} data-idle={shown <= 0} /> : null}
      </code>
    </p>
  )
}

/** How many characters of a command are visible `ms` into the beat it types on. */
export const commandShown = (ms: number) => Math.floor(ms / COMMAND_CHAR_MS)
export const commandTypeMs = (command: string) => command.length * COMMAND_CHAR_MS

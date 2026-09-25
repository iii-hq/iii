import { type CSSProperties, Fragment } from "react"

/**
 * A line of display text, one animated span per word, for the opening
 * cascade (`.intro-word` in globals.css: each word rises out of a blur, 60ms
 * apart). `from` is the index of the first word across the whole headline, so
 * a second line keeps counting instead of starting over.
 */
export function Words({ text, from = 0 }: { text: string; from?: number }) {
  // The space sits between the spans as its own text node: inside an
  // inline-block it would collapse and the words would run together.
  return text.split(" ").map((word, i) => (
    <Fragment key={`${from + i}-${word}`}>
      {i > 0 && " "}
      <span className="intro-word" style={{ "--w": from + i } as CSSProperties}>
        {word}
      </span>
    </Fragment>
  ))
}

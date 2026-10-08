'use client'

import { motion } from 'motion/react'
import type { CSSProperties } from 'react'

import type { CodeLine } from '@/lib/highlight'
import { cn } from '@/lib/utils'

const line = {
  hidden: { clipPath: 'inset(0px 100% 0px 0px)' },
  shown: { clipPath: 'inset(0px 0% 0px 0px)' },
}

/**
 * Shiki tokens rendered line by line; each line is revealed left to right the first time the block scrolls into
 * view, like the hero's code. The `<pre>` is observed, the lines are clipped (see PixelHeading for why).
 */
export function CodeLines({ lines, className }: { lines: CodeLine[]; className?: string }) {
  return (
    <motion.pre
      className={cn('outline-none', className)}
      initial="hidden"
      whileInView="shown"
      viewport={{ once: true, amount: 0.4 }}
    >
      <code className="code-tokens">
        {lines.map((tokens, index) => (
          <motion.span
            // biome-ignore lint/suspicious/noArrayIndexKey: Lines are static per block and never reorder.
            key={index}
            className="print block whitespace-pre"
            variants={line}
            transition={{ duration: 0.35, delay: 0.1 + index * 0.09, ease: 'linear' }}
          >
            {tokens.map((token, position) => (
              // biome-ignore lint/suspicious/noArrayIndexKey: Tokens are static per line and never reorder.
              <span key={position} style={token.style as CSSProperties}>
                {token.text}
              </span>
            ))}
            {'\n'}
          </motion.span>
        ))}
      </code>
    </motion.pre>
  )
}

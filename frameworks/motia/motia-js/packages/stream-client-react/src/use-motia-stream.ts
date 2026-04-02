import React from 'react'
import { MotiaStreamContext } from './motia-stream-context'

/**
 * A hook to get the stream context.
 *
 * @example
 * ```tsx
 * const { stream } = useMotiaStream()
 * ```
 */
export const useMotiaStream = () => {
  const context = React.useContext(MotiaStreamContext)

  if (!context) {
    throw new Error('useMotiaStream must be used within a MotiaStreamProvider')
  }

  return context
}

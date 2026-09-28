"use client"

import { useCallback, useEffect, useRef, useState } from "react"

/** Copy text to the clipboard and expose a `copied` flag that resets after `resetMs`. */
export function useCopy(resetMs = 2000) {
  const [copied, setCopied] = useState(false)
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)

  useEffect(() => () => clearTimeout(timer.current), [])

  const copy = useCallback(
    async (text: string) => {
      try {
        await navigator.clipboard?.writeText(text)
      } catch {
        // clipboard blocked: still show the confirmation
      }
      // Flash the confirmation even if the clipboard is blocked, matching the old site.
      setCopied(true)
      clearTimeout(timer.current)
      timer.current = setTimeout(() => setCopied(false), resetMs)
    },
    [resetMs],
  )

  return { copied, copy }
}

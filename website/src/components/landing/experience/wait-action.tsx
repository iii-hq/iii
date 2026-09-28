"use client"

import { useEffect, useEffectEvent } from "react"
import { buttonVariants } from "@/components/ui/button-variants"
import { cn } from "@/lib/utils"
import styles from "./experience.module.css"

/** How long a wait point holds for a click before the flow carries on by itself. */
export const AUTO_MS = 6000

/** Fires `action` after AUTO_MS while `enabled`. Cleared if the component leaves first. */
export function useAutoAction(enabled: boolean, action: () => void) {
  const fire = useEffectEvent(() => action())
  useEffect(() => {
    if (!enabled) return
    const t = setTimeout(fire, AUTO_MS)
    return () => clearTimeout(t)
  }, [enabled])
}

type WaitActionProps = {
  label: string
  /** called with the label on click, with nothing when the flow carries on by itself */
  onAction: (label?: string) => void
  variant?: "default" | "outline"
  size?: "xs" | "sm"
  /** also fire on Enter anywhere on the page (the Send button) */
  enterKey?: boolean
  className?: string
}

/**
 * A button where the flow is waiting for you. It breathes a ring so it reads
 * as the next thing to click, and a sweep along its base counts down to the
 * flow continuing on its own, so nobody is ever stuck.
 */
export function WaitAction({
  label,
  onAction,
  variant = "default",
  size = "xs",
  enterKey,
  className,
}: WaitActionProps) {
  useAutoAction(true, onAction)

  const onKey = useEffectEvent((e: KeyboardEvent) => {
    if (e.key === "Enter" && !e.metaKey && !e.ctrlKey) onAction(label)
  })
  useEffect(() => {
    if (!enterKey) return
    window.addEventListener("keydown", onKey)
    return () => window.removeEventListener("keydown", onKey)
  }, [enterKey])

  return (
    <span className={cn(styles.nudge, size === "xs" && "rounded-[8px]", className)}>
      <button
        type="button"
        className={cn(buttonVariants({ variant, size }), "relative overflow-hidden")}
        onClick={() => onAction(label)}
      >
        {label}
        <span aria-hidden="true" className={styles.sweep} style={{ animationDuration: `${AUTO_MS}ms` }} />
      </button>
    </span>
  )
}

"use client"

import { m, useReducedMotion } from "motion/react"
import { type KeyboardEvent, useRef } from "react"
import { MonitorIcon, MoonIcon, SunIcon } from "@/components/site/iconly"
import { useTheme } from "@/hooks/use-theme"
import { THEME_PREFERENCES, type ThemePreference } from "@/lib/theme"
import { cn } from "@/lib/utils"

const OPTIONS: { value: ThemePreference; label: string; Icon: typeof SunIcon }[] = [
  { value: "light", label: "Light", Icon: SunIcon },
  { value: "system", label: "System", Icon: MonitorIcon },
  { value: "dark", label: "Dark", Icon: MoonIcon },
]

/**
 * Light / System / Dark as a pill of three icons. One raised disc slides to
 * the chosen option (shared layoutId). Behaves as a radio group: Tab lands on
 * the chosen one, arrow keys move and select.
 */
export function ThemeToggle({ className }: { className?: string }) {
  const { preference, setPreference } = useTheme()
  const reduce = useReducedMotion()
  const refs = useRef(new Map<ThemePreference, HTMLButtonElement>())

  function onKeyDown(e: KeyboardEvent<HTMLDivElement>) {
    const step =
      e.key === "ArrowRight" || e.key === "ArrowDown" ? 1 : e.key === "ArrowLeft" || e.key === "ArrowUp" ? -1 : 0
    if (!step) return
    e.preventDefault()
    const i = THEME_PREFERENCES.indexOf(preference)
    const next = THEME_PREFERENCES[(i + step + THEME_PREFERENCES.length) % THEME_PREFERENCES.length]
    setPreference(next)
    refs.current.get(next)?.focus()
  }

  return (
    <div
      role="radiogroup"
      aria-label="Theme"
      onKeyDown={onKeyDown}
      className={cn(
        "flex h-8 items-center gap-0.5 rounded-full bg-gray-3 p-0.5 shadow-[inset_0_0_0_1px_var(--gray-5)]",
        className,
      )}
    >
      {OPTIONS.map(({ value, label, Icon }) => {
        const checked = preference === value
        return (
          // biome-ignore lint/a11y/useSemanticElements: APG radio-group pattern on buttons; a native radio can't carry the sliding disc
          <button
            key={value}
            ref={(el) => {
              if (el) refs.current.set(value, el)
            }}
            type="button"
            role="radio"
            aria-checked={checked}
            aria-label={label}
            tabIndex={checked ? 0 : -1}
            onClick={() => setPreference(value)}
            className={cn(
              "relative flex size-7 cursor-pointer items-center justify-center rounded-full outline-none transition-colors duration-150 ease-[ease] focus-visible:ring-2 focus-visible:ring-gray-8 focus-visible:ring-offset-2 focus-visible:ring-offset-gray-1",
              checked ? "text-gray-12" : "text-gray-9 hover:text-gray-11",
            )}
          >
            {checked && (
              <m.span
                layoutId="theme-disc"
                aria-hidden="true"
                className="absolute inset-0 rounded-full bg-gray-5 shadow-[inset_0_0_0_1px_var(--gray-7)]"
                transition={reduce ? { duration: 0 } : { type: "spring", duration: 0.3, bounce: 0 }}
              />
            )}
            <Icon className="relative size-[15px]" />
          </button>
        )
      })}
    </div>
  )
}

"use client"

import { useInView, useReducedMotion } from "motion/react"
import { useEffect, useRef, useState } from "react"
import { GlyphMatrix } from "@/components/ui/glyph-matrix"
import { useTheme } from "@/hooks/use-theme"
import { cn } from "@/lib/utils"

/**
 * The hero's decorative texture: Magic UI's glyph matrix, colored from the
 * theme's neutral scale (gray-11, so the glyphs read clearly against the page)
 * and set in Inter like everything else in the hero.
 * Canvas can't read CSS variables, so the color is resolved on the client
 * and refreshed when the theme flips. Under reduced motion the field is
 * drawn once and never mutates.
 */
export function GlyphField({ className }: { className?: string }) {
  const { theme } = useTheme()
  const reduce = useReducedMotion()
  // Two of these live on the page; only the one on screen should keep mutating.
  const ref = useRef<HTMLDivElement>(null)
  const inView = useInView(ref, { margin: "20% 0px" })
  const [color, setColor] = useState<string | null>(null)
  const [font, setFont] = useState("ui-sans-serif, system-ui, sans-serif")

  useEffect(() => {
    const styles = getComputedStyle(document.documentElement)
    // Dark needs the brighter step to read; on a light page the same step is too heavy.
    setColor(styles.getPropertyValue(theme === "dark" ? "--gray-11" : "--gray-9").trim() || null)
    const inter = styles.getPropertyValue("--font-inter").trim()
    if (inter) setFont(`${inter}, ui-sans-serif, system-ui, sans-serif`)
  }, [theme])

  const live = inView && !reduce

  return (
    <div ref={ref} className={cn("select-none", className)}>
      {/* Don't draw the default grey before the theme color is known. */}
      {color && (
        <GlyphMatrix
          color={color}
          fontFamily={font}
          cellSize={16}
          mutationRate={0.045}
          interval={live ? 110 : Number.MAX_SAFE_INTEGER}
          fadeBottom={0.7}
        />
      )}
    </div>
  )
}

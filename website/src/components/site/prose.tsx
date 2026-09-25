import type { ReactNode } from "react"
import { cn } from "@/lib/utils"

/**
 * Long-form article body. Takes the React nodes `renderMarkdown` produces from
 * our own markdown files and gives them the `.prose` styles from globals.css:
 * 17px Inter at a 68ch measure, headings on the type scale, Geist Mono code in
 * Shiki's two themes.
 */
export function Prose({ children, className }: { children: ReactNode; className?: string }) {
  return <div className={cn("prose", className)}>{children}</div>
}

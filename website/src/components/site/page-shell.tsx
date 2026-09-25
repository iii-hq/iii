import type { ReactNode } from "react"
import { cn } from "@/lib/utils"

/**
 * The main region for every page that isn't the landing page: room beneath
 * the fixed header (which the root layout renders), then the content.
 */
export function PageShell({ children, className }: { children: ReactNode; className?: string }) {
  return <main className={cn("pt-16", className)}>{children}</main>
}

import { cn } from "@/lib/utils"

/** The iii mark: three bars, the middle one in brand color on hover-capable surfaces. */
export function LogoMark({ className, accent = false }: { className?: string; accent?: boolean }) {
  return (
    <svg
      viewBox="0 0 1075.74 1075.74"
      xmlns="http://www.w3.org/2000/svg"
      aria-hidden="true"
      className={cn("fill-current", className)}
    >
      <rect x="0" y="0.05" width="268.94" height="268.94" />
      <rect x="0" y="403.45" width="268.94" height="672.24" />
      <g className={accent ? "fill-brand" : undefined}>
        <rect x="403.4" y="0.05" width="268.94" height="268.94" />
        <rect x="403.4" y="403.45" width="268.94" height="672.24" />
      </g>
      <rect x="806.81" y="0.05" width="268.94" height="268.94" />
      <rect x="806.81" y="403.45" width="268.94" height="672.24" />
    </svg>
  )
}

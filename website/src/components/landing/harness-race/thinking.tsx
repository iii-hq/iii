/** "Thinking..." with dots cycling off the scene clock (`t` in seconds). */
export function Thinking({ t, label = "Thinking" }: { t: number; label?: string }) {
  const dots = 1 + (Math.floor(t * 2.6) % 3)
  return (
    <div className="text-gray-10">
      {label}
      <span aria-hidden="true">{".".repeat(dots)}</span>
    </div>
  )
}

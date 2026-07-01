import { cn } from '@lib/lib/utils'

interface CaretProps {
  className?: string
}

export function Caret({ className }: CaretProps) {
  return <span aria-hidden className={cn('blink inline-block w-[6px] h-[13px] bg-ink align-middle', className)} />
}

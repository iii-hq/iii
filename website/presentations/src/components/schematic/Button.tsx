import { cn } from '@lib/lib/utils'
import * as React from 'react'

type ButtonVariant = 'primary' | 'ghost' | 'pill' | 'icon'
type ButtonSize = 'sm' | 'md' | 'icon'

const variantClasses: Record<ButtonVariant, string> = {
  primary: 'bg-ink text-bg border border-ink hover:bg-bg hover:text-ink',
  ghost: 'bg-transparent text-ink border border-transparent hover:bg-ink hover:text-bg',
  pill: 'bg-bg text-ink border border-ink hover:bg-ink hover:text-bg',
  icon: 'bg-bg text-ink-faint border border-rule hover:text-ink',
}

const sizeClasses: Record<ButtonSize, string> = {
  sm: 'h-8 px-3 text-[13px]',
  md: 'h-9 px-5 text-[13px]',
  icon: 'size-[30px] p-0',
}

export interface ButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant
  size?: ButtonSize
}

export const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant = 'primary', size = 'md', children, ...props }, ref) => {
    return (
      <button
        ref={ref}
        className={cn(
          'inline-flex items-center justify-center gap-x-2 whitespace-nowrap font-mono lowercase rounded-none transition-[background-color,color,border-color] duration-150 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent disabled:pointer-events-none disabled:opacity-40 select-none cursor-pointer',
          variantClasses[variant],
          sizeClasses[size],
          className,
        )}
        {...props}
      >
        {children}
      </button>
    )
  },
)
Button.displayName = 'Button'

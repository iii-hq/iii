'use client'

import { cva, type VariantProps } from 'class-variance-authority'
import { AnimatePresence, motion } from 'motion/react'
import * as React from 'react'

import {
  Button as ButtonPrimitive,
  type ButtonProps as ButtonPrimitiveProps,
} from '@/components/animate-ui/primitives/buttons/button'
import { IconCopy, IconTickSquare } from '@/components/icons/iconly'
import { useControlledState } from '@/hooks/use-controlled-state'
import { duration, easeOut } from '@/lib/motion'
import { cn } from '@/lib/utils'

const buttonVariants = cva(
  "flex items-center justify-center rounded-md transition-[box-shadow,_color,_background-color,_border-color,_outline-color,_text-decoration-color,_fill,_stroke] disabled:pointer-events-none disabled:opacity-50 [&_svg]:pointer-events-none [&_svg:not([class*='size-'])]:size-4 shrink-0 [&_svg]:shrink-0 outline-none focus-visible:outline-2 focus-visible:outline-foreground focus-visible:outline-offset-2 aria-invalid:ring-destructive/20 dark:aria-invalid:ring-destructive/40 aria-invalid:border-destructive",
  {
    variants: {
      variant: {
        default: 'bg-primary text-primary-foreground shadow-xs hover:bg-primary/90',
        accent: 'bg-accent text-accent-foreground shadow-xs hover:bg-accent/90',
        destructive:
          'bg-destructive text-white shadow-xs hover:bg-destructive/90 focus-visible:ring-destructive/20 dark:focus-visible:ring-destructive/40 dark:bg-destructive/60',
        outline:
          'border bg-background shadow-xs hover:bg-accent hover:text-accent-foreground dark:bg-input/30 dark:border-input dark:hover:bg-input/50',
        secondary: 'bg-secondary text-secondary-foreground shadow-xs hover:bg-secondary/80',
        ghost: 'hover:bg-accent hover:text-accent-foreground dark:hover:bg-accent/50',
        link: 'text-primary underline-offset-4 hover:underline',
      },
      size: {
        default: 'size-9',
        xs: "size-7 [&_svg:not([class*='size-'])]:size-3.5 rounded-md",
        sm: 'size-8 rounded-md',
        lg: 'size-10 rounded-md',
      },
    },
    defaultVariants: {
      variant: 'default',
      size: 'default',
    },
  },
)

type CopyButtonProps = Omit<ButtonPrimitiveProps, 'children'> &
  VariantProps<typeof buttonVariants> & {
    content: string
    copied?: boolean
    onCopiedChange?: (copied: boolean, content?: string) => void
    delay?: number
  }

function CopyButton({
  className,
  content,
  copied,
  onCopiedChange,
  onClick,
  variant,
  size,
  delay = 3000,
  ...props
}: CopyButtonProps) {
  const [isCopied, setIsCopied] = useControlledState({
    value: copied,
    onChange: onCopiedChange,
  })

  const handleCopy = React.useCallback(
    (e: React.MouseEvent<HTMLButtonElement>) => {
      onClick?.(e)
      if (copied) return
      if (content) {
        navigator.clipboard
          .writeText(content)
          .then(() => {
            setIsCopied(true)
            onCopiedChange?.(true, content)
            setTimeout(() => {
              setIsCopied(false)
              onCopiedChange?.(false)
            }, delay)
          })
          .catch((error) => {
            console.error('Error copying command', error)
          })
      }
    },
    [onClick, copied, content, setIsCopied, onCopiedChange, delay],
  )

  const Icon = isCopied ? IconTickSquare : IconCopy

  return (
    <ButtonPrimitive
      data-slot="copy-button"
      className={cn(buttonVariants({ variant, size, className }))}
      onClick={handleCopy}
      hoverScale={1}
      tapScale={0.97}
      {...props}
    >
      <AnimatePresence mode="popLayout">
        <motion.span
          key={isCopied ? 'check' : 'copy'}
          data-slot="copy-button-icon"
          initial={{ scale: 0.5, opacity: 0, filter: 'blur(4px)' }}
          animate={{ scale: 1, opacity: 1, filter: 'blur(0px)' }}
          exit={{ scale: 0.5, opacity: 0, filter: 'blur(4px)' }}
          transition={{ duration: duration.base, ease: easeOut }}
        >
          <Icon />
        </motion.span>
      </AnimatePresence>
    </ButtonPrimitive>
  )
}

export { CopyButton, buttonVariants, type CopyButtonProps }

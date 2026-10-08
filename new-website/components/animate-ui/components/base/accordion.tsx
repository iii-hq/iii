import type * as React from 'react'
import {
  AccordionHeader as AccordionHeaderPrimitive,
  AccordionItem as AccordionItemPrimitive,
  type AccordionItemProps as AccordionItemPrimitiveProps,
  AccordionPanel as AccordionPanelPrimitive,
  type AccordionPanelProps as AccordionPanelPrimitiveProps,
  Accordion as AccordionPrimitive,
  type AccordionProps as AccordionPrimitiveProps,
  AccordionTrigger as AccordionTriggerPrimitive,
  type AccordionTriggerProps as AccordionTriggerPrimitiveProps,
} from '@/components/animate-ui/primitives/base/accordion'
import { IconArrowDown } from '@/components/icons/iconly'
import { cn } from '@/lib/utils'

type AccordionProps = AccordionPrimitiveProps

function Accordion(props: AccordionProps) {
  return <AccordionPrimitive {...props} />
}

type AccordionItemProps = AccordionItemPrimitiveProps

function AccordionItem({ className, ...props }: AccordionItemProps) {
  return <AccordionItemPrimitive className={cn('border-b last:border-b-0', className)} {...props} />
}

type AccordionTriggerProps = AccordionTriggerPrimitiveProps & {
  showArrow?: boolean
}

function AccordionTrigger({ className, children, showArrow = true, ...props }: AccordionTriggerProps) {
  return (
    <AccordionHeaderPrimitive className="flex">
      <AccordionTriggerPrimitive
        className={cn(
          'flex flex-1 items-start justify-between gap-4 rounded-md py-4 text-left text-sm font-medium transition-colors outline-none hover:underline focus-visible:outline-2 focus-visible:outline-foreground focus-visible:outline-offset-2 disabled:pointer-events-none disabled:opacity-50 [&[data-panel-open]>svg]:rotate-180',
          className,
        )}
        {...props}
      >
        {children}
        {showArrow && (
          <IconArrowDown className="pointer-events-none size-5 shrink-0 text-muted-foreground transition-transform duration-300 ease-out" />
        )}
      </AccordionTriggerPrimitive>
    </AccordionHeaderPrimitive>
  )
}

type AccordionPanelProps = AccordionPanelPrimitiveProps & {
  children: React.ReactNode
}

function AccordionPanel({ className, children, ...props }: AccordionPanelProps) {
  return (
    <AccordionPanelPrimitive {...props}>
      <div className={cn('text-sm pt-0 pb-4', className)}>{children}</div>
    </AccordionPanelPrimitive>
  )
}

export {
  Accordion,
  AccordionItem,
  AccordionTrigger,
  AccordionPanel,
  type AccordionProps,
  type AccordionItemProps,
  type AccordionTriggerProps,
  type AccordionPanelProps,
}

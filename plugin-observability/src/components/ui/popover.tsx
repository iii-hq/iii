import { cn } from '@motiadev/ui'
import * as PopoverPrimitive from '@radix-ui/react-popover'
import type { ComponentProps, FC } from 'react'

export const Popover: FC<ComponentProps<typeof PopoverPrimitive.Root>> = ({ ...props }) => {
  return <PopoverPrimitive.Root data-slot="popover" {...props} />
}

export const PopoverTrigger: FC<ComponentProps<typeof PopoverPrimitive.Trigger>> = ({ ...props }) => {
  return <PopoverPrimitive.Trigger data-slot="popover-trigger" {...props} />
}

export const PopoverContent: FC<ComponentProps<typeof PopoverPrimitive.Content>> = ({
  className,
  align = 'center',
  sideOffset = 4,
  ...props
}) => {
  return (
    <PopoverPrimitive.Portal>
      <PopoverPrimitive.Content
        data-slot="popover-content"
        align={align}
        sideOffset={sideOffset}
        className={cn(
          'bg-popover text-popover-foreground data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0 data-[state=closed]:zoom-out-95 data-[state=open]:zoom-in-95 data-[side=bottom]:slide-in-from-top-2 data-[side=left]:slide-in-from-right-2 data-[side=right]:slide-in-from-left-2 data-[side=top]:slide-in-from-bottom-2 z-50 w-72 origin-(--radix-popover-content-transform-origin) rounded-md border p-4 shadow-md outline-hidden',
          className,
        )}
        {...props}
      />
    </PopoverPrimitive.Portal>
  )
}

export const PopoverAnchor: FC<ComponentProps<typeof PopoverPrimitive.Anchor>> = ({ ...props }) => {
  return <PopoverPrimitive.Anchor data-slot="popover-anchor" {...props} />
}

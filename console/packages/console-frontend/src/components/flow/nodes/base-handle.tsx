import { Handle, type HandleProps, Position } from '@xyflow/react'
import { clsx } from 'clsx'

type Props = HandleProps & {
  isHidden?: boolean
}

export function BaseHandle({ isHidden, position, ...rest }: Props) {
  const isHorizontal = position === Position.Left || position === Position.Right

  return (
    <div
      className={clsx(
        'absolute w-1.5 h-1.5',
        position === Position.Top && '-top-1',
        position === Position.Bottom && '-bottom-1',
        position === Position.Left && '-left-1',
        position === Position.Right && '-right-1',
        isHorizontal ? 'top-1/2 -mt-0.5' : 'left-1/2 -ml-0.5',
        isHidden && 'hidden',
      )}
    >
      <Handle
        {...rest}
        position={position}
        className="!static !w-1.5 !h-1.5 !min-w-0 !min-h-0 !p-0 !border-none !transform-none !rounded-full !outline-none !shadow-none !bg-[#F3F724]"
      />
    </div>
  )
}

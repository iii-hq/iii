import { cn } from '@motiadev/ui'
import { ChevronDown } from 'lucide-react'
import type { FC } from 'react'
import type { ApiEndpoint } from '../types/endpoint'
import { EndpointItem } from './endpoint-item'

type FlowGroupProps = {
  flow: string
  endpoints: ApiEndpoint[]
  isOpen: boolean
  isSelected: boolean
  onToggle: (flow: string) => void
  onClearSelection: () => void
  selectedEndpointId?: string
  onSelectEndpoint: (id: string) => void
}

export const FlowGroup: FC<FlowGroupProps> = ({
  flow,
  endpoints,
  isOpen,
  isSelected,
  onToggle,
  onClearSelection,
  selectedEndpointId,
  onSelectEndpoint,
}: FlowGroupProps) => {
  return (
    <div className="pt-2">
      <button
        data-testid={`flow-group-${flow}`}
        className="w-full grid grid-cols-[auto_1fr] items-center gap-3 hover:bg-muted/40 cursor-pointer min-h-8.5 select-none hover:bg-muted-foreground/10 px-4"
        onClick={() => {
          if (isSelected) {
            onClearSelection()
          }
          onToggle(flow)
        }}
      >
        <div className="grid grid-cols-1">
          <ChevronDown
            className={cn('w-4 h-4 transition-transform duration-300 text-[#555]', { 'rotate-180 mt-2.5': isOpen })}
            strokeWidth={1.5}
          />
          <svg
            xmlns="http://www.w3.org/2000/svg"
            width="2"
            height="10"
            viewBox="0 0 2 10"
            fill="none"
            className={cn('ml-[7px]', { hidden: !isOpen })}
          >
            <path
              d="M1.5 1C1.5 0.723858 1.27614 0.5 1 0.5C0.723858 0.5 0.5 0.723858 0.5 1H1H1.5ZM1 1H0.5V10H1H1.5V1H1Z"
              className="fill-[#555]"
            />
          </svg>
        </div>
        <h3 className="text-sm font-medium text-left">{flow}</h3>
      </button>

      <div className={cn('grid grid-cols-1 items-center justify-center', { hidden: !isOpen })}>
        {endpoints.map((endpoint: ApiEndpoint, index: number) => (
          <EndpointItem
            key={endpoint.id}
            endpoint={endpoint}
            isSelected={selectedEndpointId === endpoint.id}
            isLast={index === endpoints.length - 1}
            onSelect={onSelectEndpoint}
          />
        ))}
      </div>
    </div>
  )
}

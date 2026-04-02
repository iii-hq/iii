import { ChevronDown } from 'lucide-react'
import { useCallback, useEffect, useRef, useState } from 'react'
import type { FlowResponse } from '../../api/flows/types'

type Props = {
  flows: FlowResponse[]
  selectedFlowId: string
  onSelectFlow: (flowId: string) => void
}

export function FlowSelector({ flows, selectedFlowId, onSelectFlow }: Props) {
  const [open, setOpen] = useState(false)
  const ref = useRef<HTMLDivElement>(null)

  const activeFlow = flows.find((f) => f.id === selectedFlowId)

  const handleSelect = useCallback(
    (flowId: string) => {
      onSelectFlow(flowId)
      setOpen(false)
    },
    [onSelectFlow],
  )

  useEffect(() => {
    function onClickOutside(e: MouseEvent) {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        setOpen(false)
      }
    }
    if (open) {
      document.addEventListener('mousedown', onClickOutside)
      return () => document.removeEventListener('mousedown', onClickOutside)
    }
  }, [open])

  return (
    <div ref={ref} className="relative">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="flex items-center gap-1.5 px-2 py-1 rounded hover:bg-[#1D1D1D] transition-colors text-[#F4F4F4] text-xs md:text-sm font-medium tracking-wide"
      >
        {activeFlow?.name ?? 'Select flow'}
        <ChevronDown
          className={`w-3 h-3 text-[#5B5B5B] transition-transform ${open ? 'rotate-180' : ''}`}
        />
      </button>

      {open && (
        <div className="absolute top-full left-0 mt-1 min-w-[180px] bg-[#141414] border border-[#1D1D1D] rounded shadow-lg shadow-black/40 z-50 py-1">
          {flows.map((flow) => (
            <button
              key={flow.id}
              type="button"
              onClick={() => handleSelect(flow.id)}
              className={`w-full text-left px-3 py-1.5 text-xs tracking-wide transition-colors ${
                flow.id === selectedFlowId
                  ? 'text-[#F3F724] bg-[#F3F724]/5'
                  : 'text-[#9CA3AF] hover:text-[#F4F4F4] hover:bg-[#1D1D1D]'
              }`}
            >
              {flow.name}
            </button>
          ))}
        </div>
      )}
    </div>
  )
}

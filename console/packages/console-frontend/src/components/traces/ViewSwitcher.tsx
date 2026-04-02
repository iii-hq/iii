import { Flame, GitBranch, List, Network } from 'lucide-react'

export type ViewType = 'waterfall' | 'flamegraph' | 'map' | 'flow'

interface ViewSwitcherProps {
  currentView: ViewType
  onViewChange: (view: ViewType) => void
}

export function ViewSwitcher({ currentView, onViewChange }: ViewSwitcherProps) {
  const views: Array<{ id: ViewType; label: string; icon: typeof List }> = [
    { id: 'waterfall', label: 'Waterfall', icon: List },
    { id: 'flamegraph', label: 'Flame Graph', icon: Flame },
    { id: 'map', label: 'Trace Map', icon: Network },
    { id: 'flow', label: 'Flow', icon: GitBranch },
  ]

  return (
    <div className="inline-flex items-center gap-0.5 bg-[#0A0A0A] rounded-md p-0.5 border border-[#1D1D1D]">
      {views.map(({ id, label, icon: Icon }) => {
        const isActive = currentView === id
        return (
          <button
            key={id}
            type="button"
            onClick={() => onViewChange(id)}
            className={`
              flex items-center gap-1.5 px-3 py-1.5 rounded-[5px] transition-all duration-150
              ${
                isActive
                  ? 'bg-[#F3F724] text-black font-semibold shadow-[0_0_8px_rgba(243,247,36,0.15)]'
                  : 'text-gray-500 hover:text-gray-300 hover:bg-[#141414]'
              }
            `}
          >
            <Icon className="w-3.5 h-3.5" />
            <span className="text-[11px]">{label}</span>
          </button>
        )
      })}
    </div>
  )
}

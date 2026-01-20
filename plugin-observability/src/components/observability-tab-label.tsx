import { GanttChartSquare } from 'lucide-react'
import { memo } from 'react'

export const ObservabilityTabLabel = memo(() => (
  <div data-testid="observability-link">
    <GanttChartSquare aria-hidden="true" />
    <span>Tracing</span>
  </div>
))
ObservabilityTabLabel.displayName = 'ObservabilityTabLabel'

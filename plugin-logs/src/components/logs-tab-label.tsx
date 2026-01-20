import { ClipboardList } from 'lucide-react'
import { memo } from 'react'

export const LogsTabLabel = memo(() => (
  <>
    <ClipboardList aria-hidden="true" />
    <span>Logs</span>
  </>
))
LogsTabLabel.displayName = 'LogsTabLabel'

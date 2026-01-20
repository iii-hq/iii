import { FileText } from 'lucide-react'
import { memo } from 'react'

export const StatesTabLabel = memo(() => (
  <>
    <FileText aria-hidden="true" />
    <span>States</span>
  </>
))
StatesTabLabel.displayName = 'StatesTabLabel'

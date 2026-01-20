import { Sidebar } from '@motiadev/ui'
import { X } from 'lucide-react'
import type React from 'react'
import type { StateItem } from '../types/state'
import { StateDetails } from './state-details'
import { StateEditor } from './state-editor'

type Props = {
  state: StateItem
  onClose: () => void
}

export const StateSidebar: React.FC<Props> = ({ state, onClose }) => {
  return (
    <Sidebar
      onClose={onClose}
      title="State Details"
      initialWidth={500}
      tabs={[
        {
          label: 'Overview',
          content: <StateDetails state={state} />,
        },
        {
          label: 'Editor',
          content: <StateEditor state={state} />,
        },
      ]}
      actions={[{ icon: <X />, onClick: onClose, label: 'Close' }]}
    />
  )
}

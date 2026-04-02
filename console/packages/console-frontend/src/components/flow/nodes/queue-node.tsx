import { ListOrdered } from 'lucide-react'
import type { NodeData } from '../../../api/flows/types'
import { BaseNode } from './base-node'

export function QueueFlowNode({ data }: { data: NodeData }) {
  const queueTrigger = data.triggers?.find((t) => t.type === 'queue')

  return (
    <BaseNode
      data={data}
      variant="queue"
      title={data.name}
      subtitle={data.description}
      disableSourceHandle={!data.emits?.length && !data.virtualEmits?.length}
      disableTargetHandle={!data.subscribes?.length && !data.virtualSubscribes?.length}
    >
      {queueTrigger?.topic && (
        <div className="text-[10px] text-[#9CA3AF] flex items-center gap-1.5 font-mono">
          <ListOrdered className="w-3 h-3" /> {queueTrigger.topic}
        </div>
      )}
    </BaseNode>
  )
}

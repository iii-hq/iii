import { Clock } from 'lucide-react'
import type { NodeData } from '../../../api/flows/types'
import { BaseNode } from './base-node'
import { TriggerList } from './trigger-list'

export function CronFlowNode({ data }: { data: NodeData }) {
  const hasMultiple = data.triggers && data.triggers.length > 1

  return (
    <BaseNode
      data={data}
      variant="cron"
      title={data.name}
      subtitle={data.description}
      disableTargetHandle={!data.virtualSubscribes?.length}
      disableSourceHandle={!data.virtualEmits?.length && !data.emits?.length}
    >
      <TriggerList triggers={data.triggers} />
      {!hasMultiple && data.cronExpression && (
        <div className="text-[10px] text-[#9CA3AF] flex items-center gap-1.5 font-mono">
          <Clock className="w-3 h-3" /> {data.cronExpression}
        </div>
      )}
    </BaseNode>
  )
}

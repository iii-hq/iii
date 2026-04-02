import type { NodeData } from '../../../api/flows/types'
import { BaseNode } from './base-node'
import { TriggerList } from './trigger-list'

export function ApiFlowNode({ data }: { data: NodeData }) {
  const hasMultiple = data.triggers && data.triggers.length > 1

  return (
    <BaseNode
      data={data}
      variant="http"
      title={data.name}
      subtitle={data.description}
      disableSourceHandle={!data.emits?.length && !data.virtualEmits?.length}
      disableTargetHandle={!data.subscribes?.length && !data.virtualSubscribes?.length}
    >
      <TriggerList triggers={data.triggers} />
      {!hasMultiple && data.webhookUrl && (
        <div className="text-[10px] text-[#9CA3AF] font-mono">{data.webhookUrl}</div>
      )}
    </BaseNode>
  )
}

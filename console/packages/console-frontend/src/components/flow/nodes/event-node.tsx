import type { NodeData } from '../../../api/flows/types'
import { BaseNode } from './base-node'
import { TriggerList } from './trigger-list'

export function EventFlowNode({ data }: { data: NodeData }) {
  return (
    <BaseNode
      data={data}
      variant="event"
      title={data.name}
      subtitle={data.description}
      disableSourceHandle={!data.emits?.length && !data.virtualEmits?.length}
      disableTargetHandle={!data.subscribes?.length && !data.virtualSubscribes?.length}
    >
      <TriggerList triggers={data.triggers} />
    </BaseNode>
  )
}

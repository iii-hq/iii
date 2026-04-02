import type { NodeData } from '../../../api/flows/types'
import { BaseNode } from './base-node'

export function StateFlowNode({ data }: { data: NodeData }) {
  return (
    <BaseNode
      data={data}
      variant="state"
      title={data.name}
      subtitle={data.description}
      disableSourceHandle={!data.emits?.length && !data.virtualEmits?.length}
      disableTargetHandle={!data.subscribes?.length && !data.virtualSubscribes?.length}
    />
  )
}

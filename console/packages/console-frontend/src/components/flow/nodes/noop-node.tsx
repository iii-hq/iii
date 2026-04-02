import type { NodeData } from '../../../api/flows/types'
import { BaseNode } from './base-node'

export function NoopFlowNode({ data }: { data: NodeData }) {
  return (
    <BaseNode
      data={data}
      variant="noop"
      title={data.name}
      subtitle={data.description}
      disableSourceHandle={!data.virtualEmits?.length}
      disableTargetHandle={!data.subscribes?.length}
    />
  )
}

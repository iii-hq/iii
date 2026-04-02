export type Emit = string | { topic: string; label?: string; conditional?: boolean }

export type TriggerData = {
  type: 'event' | 'noop' | 'http' | 'cron' | 'queue' | 'state'
  topic?: string
  path?: string
  method?: string
  cronExpression?: string
  bodySchema?: unknown
  input?: unknown
  conditionFunctionId?: string
}

export type FlowStep = {
  id: string
  name: string
  type: 'event' | 'http' | 'noop' | 'cron' | 'queue' | 'state'
  triggers: TriggerData[]
  description?: string
  subscribes?: string[]
  emits: Emit[]
  virtualEmits?: Emit[]
  virtualSubscribes?: string[]
  action?: 'webhook'
  webhookUrl?: string
  webhookUrls?: string[]
  cronExpression?: string
  cronExpressions?: string[]
  language?: string
  filePath?: string
}

export type EdgeData = {
  variant: 'event' | 'virtual'
  topic: string
  label?: string
  labelVariant?: 'default' | 'conditional'
}

export type FlowEdge = {
  id: string
  source: string
  target: string
  data: EdgeData
}

export type FlowResponse = {
  id: string
  name: string
  steps: FlowStep[]
  edges: FlowEdge[]
}

export type NodeConfig = {
  x: number
  y: number
  sourceHandlePosition?: 'bottom' | 'right'
  targetHandlePosition?: 'top' | 'left'
}

export type FlowConfigResponse = {
  id: string
  config: Record<string, NodeConfig>
}

export type NodeData = FlowStep & {
  nodeConfig?: NodeConfig
}

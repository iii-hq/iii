import { fetchWithFallback } from '../utils'
import type { Emit, FlowConfigResponse, FlowEdge, FlowResponse, FlowStep } from './types'

// --- Flow transformation logic (ported from workbench server) ---

// biome-ignore lint/suspicious/noExplicitAny: snake_case keys come from the engine API
function normalizeMetadata(raw: Record<string, any>): FunctionMetadata {
  // Engine API uses "enqueues" instead of "emits"
  const emits: Emit[] = raw.emits ?? raw.Emits ?? raw.enqueues ?? raw.Enqueues ?? []
  const triggers = raw.triggers ?? raw.Triggers ?? []

  // Some engines store emits inside trigger configs
  const triggerEmits: Emit[] = []
  if (Array.isArray(triggers)) {
    for (const t of triggers) {
      if (t.emits && Array.isArray(t.emits)) {
        triggerEmits.push(...t.emits)
      }
      if (t.enqueues && Array.isArray(t.enqueues)) {
        triggerEmits.push(...t.enqueues)
      }
    }
  }

  return {
    name: raw.name ?? raw.Name,
    description: raw.description ?? raw.Description,
    filePath: raw.filePath ?? raw.file_path ?? raw.filepath,
    triggers,
    emits: emits.length > 0 ? emits : triggerEmits,
    virtualEmits:
      raw.virtualEmits ?? raw.virtual_emits ?? raw.virtualenqueues ?? raw.virtualEnqueues,
    virtualSubscribes: raw.virtualSubscribes ?? raw.virtual_subscribes,
    flows: raw.flows ?? raw.Flows,
  }
}

interface FunctionMetadata {
  name?: string
  description?: string
  filePath?: string
  triggers?: Array<{
    type: string
    topic?: string
    path?: string
    method?: string
    expression?: string
    bodySchema?: unknown
  }>
  emits?: Emit[]
  virtualEmits?: Emit[]
  virtualSubscribes?: string[]
  flows?: string[]
}

interface FunctionInfo {
  name?: string
  function_id?: string
  functionPath?: string
  metadata?: Record<string, unknown>
}

function generateStepId(filePath: string): string {
  if (!filePath) return 'unknown-step'
  // Deterministic ID from file path - use full path to avoid collisions
  return filePath
    .replace(/\.[^.]+$/, '') // Remove extension
    .replace(/[^a-zA-Z0-9-_]/g, '-') // Sanitize
    .replace(/^-+|-+$/g, '') // Trim leading/trailing dashes
    .toLowerCase()
}

function getStepLanguage(filePath?: string): string | undefined {
  if (!filePath) return undefined
  if (filePath.endsWith('.ts')) return 'typescript'
  if (filePath.endsWith('.js')) return 'javascript'
  if (filePath.endsWith('.py')) return 'python'
  if (filePath.endsWith('.go')) return 'go'
  if (filePath.endsWith('.rb')) return 'ruby'
  return undefined
}

function processEmit(emit: Emit): { topic: string; label?: string; conditional?: boolean } {
  if (typeof emit === 'string') {
    return { topic: emit }
  }
  return {
    topic: emit.topic,
    label: emit.label,
    conditional:
      'conditional' in emit ? (emit as { conditional?: boolean }).conditional : undefined,
  }
}

function createStep(func: FunctionInfo): FlowStep {
  const funcName = func.name ?? func.function_id ?? 'unknown'
  const meta = func.metadata as FunctionMetadata | undefined
  if (!meta?.filePath) {
    return {
      id: funcName,
      name: meta?.name || funcName,
      type: 'noop',
      triggers: [],
      emits: [],
    }
  }

  const id = generateStepId(meta.filePath)
  const triggers = (meta.triggers || []).map((t) => {
    if (t.type === 'event') return { type: 'event' as const, topic: t.topic }
    if (t.type === 'http')
      return { type: 'http' as const, path: t.path, method: t.method, bodySchema: t.bodySchema }
    if (t.type === 'cron') return { type: 'cron' as const, cronExpression: t.expression }
    if (t.type === 'durable:subscriber')
      return { type: 'durable:subscriber' as const, topic: t.topic }
    if (t.type === 'state') return { type: 'state' as const }
    return { type: t.type as FlowStep['type'] }
  })

  const eventTriggers = (meta.triggers || []).filter((t) => t.type === 'event')
  const httpTriggers = (meta.triggers || []).filter((t) => t.type === 'http')
  const cronTriggers = (meta.triggers || []).filter((t) => t.type === 'cron')
  const queueTriggers = (meta.triggers || []).filter((t) => t.type === 'durable:subscriber')

  const knownTypes: Set<FlowStep['type']> = new Set([
    'event',
    'http',
    'cron',
    'durable:subscriber',
    'state',
  ])
  const rawType = triggers.length > 0 ? triggers[0].type : undefined
  const stepType: FlowStep['type'] =
    rawType && knownTypes.has(rawType as FlowStep['type']) ? (rawType as FlowStep['type']) : 'noop'

  const topicTriggers = [...eventTriggers, ...queueTriggers]
  const subscribes =
    topicTriggers.length > 0
      ? topicTriggers.map((t) => t.topic ?? '').filter(Boolean)
      : meta.virtualSubscribes
        ? [...meta.virtualSubscribes]
        : undefined

  const webhookUrls =
    httpTriggers.length > 0 ? httpTriggers.map((t) => `${t.method} ${t.path}`) : undefined

  const cronExpressions =
    cronTriggers.length > 0 ? cronTriggers.map((t) => t.expression ?? '') : undefined

  const firstHttp = httpTriggers[0]
  const firstCron = cronTriggers[0]

  return {
    id,
    name: meta.name || funcName,
    description: meta.description,
    type: stepType,
    triggers,
    filePath: meta.filePath,
    language: getStepLanguage(meta.filePath),
    emits: meta.emits || [],
    virtualEmits: meta.virtualEmits,
    virtualSubscribes: meta.virtualSubscribes,
    subscribes,
    webhookUrls,
    cronExpressions,
    action: httpTriggers.length > 0 ? 'webhook' : undefined,
    webhookUrl: firstHttp ? `${firstHttp.method} ${firstHttp.path}` : undefined,
    cronExpression: firstCron?.expression,
  }
}

function buildTopicMap(steps: FlowStep[]): Map<string, FlowStep[]> {
  const map = new Map<string, FlowStep[]>()
  for (const step of steps) {
    for (const topic of step.subscribes ?? []) {
      const list = map.get(topic) ?? []
      list.push(step)
      map.set(topic, list)
    }
    for (const topic of step.virtualSubscribes ?? []) {
      const list = map.get(topic) ?? []
      list.push(step)
      map.set(topic, list)
    }
  }
  return map
}

function createEdges(sourceStep: FlowStep, topicMap: Map<string, FlowStep[]>): FlowEdge[] {
  const edges: FlowEdge[] = []

  const addEdgesForEmits = (emits: Emit[], variant: 'event' | 'virtual') => {
    for (const emit of emits) {
      const { topic, label, conditional } = processEmit(emit)
      if (!topic) continue
      for (const target of topicMap.get(topic) ?? []) {
        edges.push({
          id: `${sourceStep.id}-${target.id}`,
          source: sourceStep.id,
          target: target.id,
          data: {
            variant,
            topic,
            label,
            labelVariant: conditional ? 'conditional' : 'default',
          },
        })
      }
    }
  }

  addEdgesForEmits(sourceStep.emits, 'event')
  if (sourceStep.virtualEmits) {
    addEdgesForEmits(sourceStep.virtualEmits, 'virtual')
  }

  return edges
}

interface NormalizedFunction {
  name: string
  functionPath: string
  metadata?: FunctionMetadata
}

function normalizeFunction(func: FunctionInfo): NormalizedFunction {
  return {
    name: func.name ?? func.function_id ?? 'unknown',
    functionPath: func.functionPath ?? '',
    metadata: func.metadata
      ? normalizeMetadata(func.metadata as Record<string, unknown>)
      : undefined,
  }
}

function groupByFlow(functions: NormalizedFunction[]): Map<string, NormalizedFunction[]> {
  const map = new Map<string, NormalizedFunction[]>()
  for (const func of functions) {
    const flows = (func.metadata?.flows as string[]) ?? ['default']
    for (const flowId of flows) {
      const existing = map.get(flowId) ?? []
      existing.push(func)
      map.set(flowId, existing)
    }
  }
  return map
}

function buildFlow(flowId: string, functions: NormalizedFunction[]): FlowResponse | null {
  const valid = functions.filter((f) => f.metadata && typeof f.metadata.filePath === 'string')

  const deduped = new Map<string, NormalizedFunction>()
  for (const f of valid) {
    const key = f.metadata?.filePath ?? ''
    if (!deduped.has(key)) deduped.set(key, f)
  }

  const steps = [...deduped.values()].map((f) => createStep(f as FunctionInfo))
  if (steps.length === 0) return null

  const topicMap = buildTopicMap(steps)
  const edges = steps.flatMap((step) => createEdges(step, topicMap))
  return { id: flowId, name: flowId, steps, edges }
}

function transformFunctionsToFlows(functions: FunctionInfo[]): FlowResponse[] {
  const normalized = functions.map(normalizeFunction)
  const byFlow = groupByFlow(normalized)
  const result: FlowResponse[] = []
  for (const [flowId, funcs] of byFlow) {
    const flow = buildFlow(flowId, funcs)
    if (flow) result.push(flow)
  }
  return result
}

// --- API fetch functions ---

interface FunctionsApiResponse {
  functions: FunctionInfo[]
}

interface TriggersApiResponse {
  triggers: Array<{
    id: string
    trigger_type: string
    function_id: string
    config: Record<string, unknown>
  }>
}

export async function fetchFlows(): Promise<FlowResponse[]> {
  // Fetch functions and triggers in parallel
  const [functionsData, triggersData] = await Promise.all([
    fetchWithFallback<FunctionsApiResponse>('/functions?include_internal=true'),
    fetchWithFallback<TriggersApiResponse>('/triggers?include_internal=true').catch((e) => {
      console.warn('Failed to fetch triggers, continuing without trigger data:', e)
      return { triggers: [] as TriggersApiResponse['triggers'] }
    }),
  ])

  const functions = functionsData.functions || []

  // Build a map of function_id -> triggers, normalizing engine config field names
  // to the metadata format that createStep expects (e.g. api_path -> path)
  const triggersByFunction = new Map<string, Array<Record<string, unknown>>>()
  for (const trigger of triggersData.triggers || []) {
    const funcId = trigger.function_id
    const existing = triggersByFunction.get(funcId) ?? []
    const config = trigger.config as Record<string, unknown>
    const normalized: Record<string, unknown> = {
      ...config,
      type: trigger.trigger_type,
    }

    if (trigger.trigger_type === 'http') {
      const rawPath = config.api_path as string | undefined
      normalized.path = rawPath ? (rawPath.startsWith('/') ? rawPath : `/${rawPath}`) : config.path
      normalized.method = config.http_method ?? config.method
    } else if (trigger.trigger_type === 'cron') {
      normalized.expression = config.expression
    } else if (
      trigger.trigger_type === 'event' ||
      trigger.trigger_type === 'queue' ||
      trigger.trigger_type === 'durable:subscriber'
    ) {
      normalized.topic = config.topic
    }

    existing.push(normalized)
    triggersByFunction.set(funcId, existing)
  }

  // Merge triggers into function metadata
  for (const func of functions) {
    const funcId = func.function_id ?? func.name
    const externalTriggers = triggersByFunction.get(funcId ?? '')

    if (externalTriggers && func.metadata) {
      const meta = func.metadata as Record<string, unknown>
      if (!meta.triggers || (Array.isArray(meta.triggers) && meta.triggers.length === 0)) {
        meta.triggers = externalTriggers
      }
    }
  }

  return transformFunctionsToFlows(functions)
}

export async function fetchFlowConfig(flowId: string): Promise<FlowConfigResponse> {
  try {
    return await fetchWithFallback<FlowConfigResponse>(
      `/flows/config/${encodeURIComponent(flowId)}`,
    )
  } catch (e) {
    console.warn('Failed to fetch flow config for', flowId, ':', e)
    return { id: flowId, config: {} }
  }
}

export async function saveFlowConfig(flowId: string, config: FlowConfigResponse): Promise<void> {
  await fetchWithFallback<{ message: string }>(
    `/flows/config/${encodeURIComponent(flowId)}`,
    undefined,
    {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(config),
    },
  )
}

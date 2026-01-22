export enum MessageType {
  RegisterFunction = 'registerfunction',
  RegisterService = 'registerservice',
  InvokeFunction = 'invokefunction',
  InvocationResult = 'invocationresult',
  RegisterTriggerType = 'registertriggertype',
  RegisterTrigger = 'registertrigger',
  UnregisterTrigger = 'unregistertrigger',
  UnregisterTriggerType = 'unregistertriggertype',
  TriggerRegistrationResult = 'triggerregistrationresult',
}

export type RegisterTriggerTypeMessage = {
  type: MessageType.RegisterTriggerType
  id: string
  description: string
}

export type UnregisterTriggerTypeMessage = {
  type: MessageType.UnregisterTriggerType
  id: string
}

export type UnregisterTriggerMessage = {
  type: MessageType.UnregisterTrigger
  id: string
}

export type TriggerRegistrationResultMessage = {
  type: MessageType.TriggerRegistrationResult
  id: string
  trigger_type: string
  function_path: string
  result?: any
  error?: any
}

export type RegisterTriggerMessage = {
  type: MessageType.RegisterTrigger

  id: string
  /**
   * The type of trigger. Can be 'cron', 'event', 'http', etc.
   */
  trigger_type: string
  /**
   * Engine path for the function, including the service and function name
   * Example: software.engineering.code.rust
   * Where software, engineering, and code are the service ids
   */
  function_path: string
  config: any
}

export type RegisterServiceMessage = {
  type: MessageType.RegisterService
  id: string
  description?: string
  parent_service_id?: string
}

export type RegisterFunctionFormat = {
  name: string
  /**
   * The description of the parameter
   */
  description?: string
  /**
   * The type of the parameter
   */
  type: 'string' | 'number' | 'boolean' | 'object' | 'array' | 'null' | 'map'
  /**
   * The body of the parameter
   */
  body?: RegisterFunctionFormat[]
  /**
   * The items of the parameter
   */
  items?: RegisterFunctionFormat
  /**
   * Whether the parameter is required
   */
  required?: boolean
}

export type RegisterFunctionMessage = {
  type: MessageType.RegisterFunction
  /**
   * The path of the function
   */
  function_path: string
  /**
   * The description of the function
   */
  description?: string
  /**
   * The request format of the function
   */
  request_format?: RegisterFunctionFormat
  /**
   * The response format of the function
   */
  response_format?: RegisterFunctionFormat
  metadata?: Record<string, unknown>
}

export type InvokeFunctionMessage = {
  type: MessageType.InvokeFunction
  /**
   * This is optional for async invocations
   */
  invocation_id?: string
  /**
   * The path of the function
   */
  function_path: string
  /**
   * The data to pass to the function
   */
  data: any
}

export type InvocationResultMessage = {
  type: MessageType.InvocationResult
  /**
   * The id of the invocation
   */
  invocation_id: string
  /**
   * The path of the function
   */
  function_path: string
  result?: any
  error?: any
}

export type FunctionInfo = {
  function_path: string
  description?: string
  request_format?: RegisterFunctionFormat
  response_format?: RegisterFunctionFormat
  metadata?: Record<string, unknown>
}

export type WorkerStatus = 'connected' | 'available' | 'busy' | 'disconnected'

export type WorkerInfo = {
  id: string
  name?: string
  runtime?: string
  version?: string
  os?: string
  ip_address?: string
  status: WorkerStatus
  connected_at_ms: number
  function_count: number
  functions: string[]
  active_invocations: number
}

export type ProcessMetrics = {
  cpu_percent?: number
  memory_used_bytes?: number
  memory_total_bytes?: number
  process_uptime_secs?: number
}

export type PerformanceMetrics = {
  thread_count?: number
  open_connections?: number
  invocations_per_sec?: number
  avg_latency_ms?: number
}

export type ExtendedMetrics = {
  disk_read_bytes?: number
  disk_write_bytes?: number
  network_rx_bytes?: number
  network_tx_bytes?: number
  open_file_descriptors?: number
  error_count?: number
}

export type KubernetesIdentifiers = {
  cluster?: string
  namespace?: string
  pod_name?: string
  container_name?: string
  node_name?: string
  pod_uid?: string
}

export type KubernetesCoreMetrics = {
  cpu_usage_cores?: number
  memory_working_set_bytes?: number
  pod_phase?: string
  pod_ready?: boolean
  container_restarts_total?: number
  last_termination_reason?: string
  uptime_seconds?: number
}

export type KubernetesResourceMetrics = {
  cpu_requests_cores?: number
  cpu_limits_cores?: number
  memory_requests_bytes?: number
  memory_limits_bytes?: number
  cpu_throttled_seconds_total?: number
  pod_pending_seconds?: number
}

export type KubernetesExtendedMetrics = {
  network_rx_bytes_total?: number
  network_tx_bytes_total?: number
  fs_usage_bytes?: number
  node_memory_pressure?: boolean
  node_disk_pressure?: boolean
  node_pid_pressure?: boolean
}

export type WorkerMetrics = {
  collected_at_ms: number
  process?: ProcessMetrics
  performance?: PerformanceMetrics
  extended?: ExtendedMetrics
  k8s_identifiers?: KubernetesIdentifiers
  k8s_core?: KubernetesCoreMetrics
  k8s_resources?: KubernetesResourceMetrics
  k8s_extended?: KubernetesExtendedMetrics
}

export type WorkerMetricsInfo = {
  worker_id: string
  worker_name?: string
  metrics: WorkerMetrics
}

export type BridgeMessage =
  | RegisterFunctionMessage
  | InvokeFunctionMessage
  | InvocationResultMessage
  | RegisterServiceMessage
  | RegisterTriggerMessage
  | RegisterTriggerTypeMessage
  | UnregisterTriggerMessage
  | UnregisterTriggerTypeMessage
  | TriggerRegistrationResultMessage

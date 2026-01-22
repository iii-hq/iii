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

/** Process-level metrics */
export type ProcessMetrics = {
  /** Current CPU usage percentage (0-100) */
  cpu_percent?: number
  /** Process memory usage in bytes */
  memory_used_bytes?: number
  /** Total available memory in bytes */
  memory_total_bytes?: number
  /** Time since worker started in seconds */
  process_uptime_secs?: number
}

/** Performance metrics */
export type PerformanceMetrics = {
  /** Number of active threads */
  thread_count?: number
  /** Number of open network connections */
  open_connections?: number
  /** Invocations processed per second */
  invocations_per_sec?: number
  /** Average invocation latency in milliseconds */
  avg_latency_ms?: number
}

/** Extended metrics */
export type ExtendedMetrics = {
  /** Disk read bytes */
  disk_read_bytes?: number
  /** Disk write bytes */
  disk_write_bytes?: number
  /** Network bytes received */
  network_rx_bytes?: number
  /** Network bytes transmitted */
  network_tx_bytes?: number
  /** Number of open file descriptors */
  open_file_descriptors?: number
  /** Total failed invocations count */
  error_count?: number
}

/** Kubernetes/EKS-specific identifiers for correlation */
export type KubernetesIdentifiers = {
  /** Cluster name */
  cluster?: string
  /** Kubernetes namespace */
  namespace?: string
  /** Pod name */
  pod_name?: string
  /** Container name */
  container_name?: string
  /** Node name */
  node_name?: string
  /** Pod UID for unique identification */
  pod_uid?: string
}

/** Kubernetes core metrics */
export type KubernetesCoreMetrics = {
  /** CPU usage in cores (or millicores) */
  cpu_usage_cores?: number
  /** Memory working set bytes */
  memory_working_set_bytes?: number
  /** Pod phase (Pending, Running, Succeeded, Failed, Unknown) */
  pod_phase?: string
  /** Whether pod is ready to accept traffic */
  pod_ready?: boolean
  /** Total container restarts */
  container_restarts_total?: number
  /** Last termination reason (e.g., OOMKilled, Error) */
  last_termination_reason?: string
  /** Container uptime in seconds */
  uptime_seconds?: number
}

/** Kubernetes resource metrics */
export type KubernetesResourceMetrics = {
  /** CPU requests in cores */
  cpu_requests_cores?: number
  /** CPU limits in cores */
  cpu_limits_cores?: number
  /** Memory requests in bytes */
  memory_requests_bytes?: number
  /** Memory limits in bytes */
  memory_limits_bytes?: number
  /** CPU throttled time in seconds */
  cpu_throttled_seconds_total?: number
  /** Time pod spent in pending state */
  pod_pending_seconds?: number
}

/** Kubernetes extended metrics */
export type KubernetesExtendedMetrics = {
  /** Network received bytes (pod/container) */
  network_rx_bytes_total?: number
  /** Network transmitted bytes (pod/container) */
  network_tx_bytes_total?: number
  /** Filesystem usage in bytes */
  fs_usage_bytes?: number
  /** Node memory pressure */
  node_memory_pressure?: boolean
  /** Node disk pressure */
  node_disk_pressure?: boolean
  /** Node PID pressure */
  node_pid_pressure?: boolean
}

/** Complete worker metrics payload */
export type WorkerMetrics = {
  /** Timestamp when metrics were collected (Unix epoch ms) */
  collected_at_ms: number

  process?: ProcessMetrics
  performance?: PerformanceMetrics
  extended?: ExtendedMetrics
  k8s_identifiers?: KubernetesIdentifiers
  k8s_core?: KubernetesCoreMetrics
  k8s_resources?: KubernetesResourceMetrics
  k8s_extended?: KubernetesExtendedMetrics
}

/** Worker metrics response with worker info */
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

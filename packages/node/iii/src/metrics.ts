/**
 * Real system metrics collector for iii-sdk
 * Collects CPU, memory, network, disk metrics and K8s info when available
 */

import * as os from 'os'
import * as fs from 'fs'
import * as path from 'path'
import type {
  WorkerMetrics,
  ProcessMetrics,
  PerformanceMetrics,
  ExtendedMetrics,
  KubernetesIdentifiers,
  KubernetesCoreMetrics,
  KubernetesResourceMetrics,
  KubernetesExtendedMetrics,
} from './bridge-types'

// Track metrics over time for calculations
let lastCpuUsage: NodeJS.CpuUsage | null = null
let lastCpuTime: [number, number] | null = null
let lastNetworkStats: { rx: number; tx: number; time: number } | null = null
let lastDiskStats: { read: number; write: number; time: number } | null = null

// Invocation tracking
let invocationCount = 0
let totalLatencyMs = 0
let errorCount = 0
let lastInvocationWindow = Date.now()

// Process start time
const processStartTime = Date.now()

/**
 * Record an invocation for throughput/latency calculations
 */
export function recordInvocation(latencyMs: number, isError: boolean = false): void {
  invocationCount++
  totalLatencyMs += latencyMs
  if (isError) {
    errorCount++
  }
}

/**
 * Get real CPU usage percentage using process.cpuUsage()
 * Returns percentage of CPU used since last call (0-100 per core)
 */
function getCpuPercent(): number {
  const currentUsage = process.cpuUsage(lastCpuUsage ?? undefined)
  const currentTime = process.hrtime()

  if (lastCpuUsage === null || lastCpuTime === null) {
    lastCpuUsage = process.cpuUsage()
    lastCpuTime = process.hrtime()
    return 0 // First call, no delta available
  }

  // Calculate elapsed time in microseconds
  const elapsedTime = (currentTime[0] - lastCpuTime[0]) * 1e6 + (currentTime[1] - lastCpuTime[1]) / 1000

  if (elapsedTime <= 0) {
    return 0
  }

  // CPU time used in microseconds (user + system)
  const cpuTimeUsed = currentUsage.user + currentUsage.system

  // Calculate percentage (can exceed 100% on multi-core)
  const cpuPercent = (cpuTimeUsed / elapsedTime) * 100

  // Update for next call
  lastCpuUsage = process.cpuUsage()
  lastCpuTime = process.hrtime()

  return Math.min(cpuPercent, 100 * os.cpus().length)
}

/**
 * Get process memory metrics
 */
function getMemoryMetrics(): { used: number; total: number } {
  const memUsage = process.memoryUsage()
  return {
    used: memUsage.heapUsed + memUsage.external,
    total: memUsage.heapTotal + memUsage.external,
  }
}

/**
 * Get thread count (Node.js uses libuv thread pool)
 * UV_THREADPOOL_SIZE defaults to 4, can be set via env var
 */
function getThreadCount(): number {
  // Node.js main thread + libuv thread pool
  const uvThreadPoolSize = parseInt(process.env.UV_THREADPOOL_SIZE || '4', 10)
  return 1 + uvThreadPoolSize
}

/**
 * Check if running inside Kubernetes
 */
function isKubernetes(): boolean {
  return !!process.env.KUBERNETES_SERVICE_HOST
}

/**
 * Get Kubernetes identifiers from environment/downward API
 */
function getKubernetesIdentifiers(): KubernetesIdentifiers | undefined {
  if (!isKubernetes()) {
    return undefined
  }

  return {
    cluster: process.env.CLUSTER_NAME,
    namespace: process.env.POD_NAMESPACE || readK8sFile('namespace'),
    pod_name: process.env.POD_NAME || process.env.HOSTNAME,
    container_name: process.env.CONTAINER_NAME,
    node_name: process.env.NODE_NAME,
    pod_uid: process.env.POD_UID || readK8sFile('pod-uid'),
  }
}

/**
 * Read Kubernetes downward API files
 */
function readK8sFile(filename: string): string | undefined {
  const basePath = '/var/run/secrets/kubernetes.io/serviceaccount'
  const filePath = path.join(basePath, filename)
  try {
    if (fs.existsSync(filePath)) {
      return fs.readFileSync(filePath, 'utf-8').trim()
    }
  } catch {
    // Ignore errors
  }
  return undefined
}

/**
 * Read a cgroup file and return its numeric value
 * Supports both cgroup v1 and v2 paths
 */
function readCgroupValue(v1Path: string, v2Path: string): number | undefined {
  const paths = [v2Path, v1Path]
  for (const p of paths) {
    try {
      if (fs.existsSync(p)) {
        const content = fs.readFileSync(p, 'utf-8').trim()
        const value = parseInt(content, 10)
        if (!isNaN(value) && value !== -1) {
          return value
        }
      }
    } catch {
      // Continue to next path
    }
  }
  return undefined
}

/**
 * Get CPU usage in cores from cgroup
 */
function getCgroupCpuUsageCores(): number | undefined {
  // cgroup v2: /sys/fs/cgroup/cpu.stat contains usage_usec
  // cgroup v1: /sys/fs/cgroup/cpu/cpuacct.usage (nanoseconds)
  try {
    // Try cgroup v2 first
    const v2Path = '/sys/fs/cgroup/cpu.stat'
    if (fs.existsSync(v2Path)) {
      const content = fs.readFileSync(v2Path, 'utf-8')
      const match = content.match(/usage_usec\s+(\d+)/)
      if (match) {
        const usec = parseInt(match[1], 10)
        // Convert microseconds to cores (assuming 1 second window)
        return usec / 1_000_000
      }
    }

    // Try cgroup v1
    const v1Path = '/sys/fs/cgroup/cpu/cpuacct.usage'
    if (fs.existsSync(v1Path)) {
      const nsec = parseInt(fs.readFileSync(v1Path, 'utf-8').trim(), 10)
      // Convert nanoseconds to cores
      return nsec / 1_000_000_000
    }
  } catch {
    // Ignore errors
  }
  return undefined
}

/**
 * Get memory working set bytes from cgroup
 */
function getCgroupMemoryWorkingSet(): number | undefined {
  // cgroup v2: /sys/fs/cgroup/memory.current
  // cgroup v1: /sys/fs/cgroup/memory/memory.usage_in_bytes
  return readCgroupValue(
    '/sys/fs/cgroup/memory/memory.usage_in_bytes',
    '/sys/fs/cgroup/memory.current'
  )
}

/**
 * Get memory limit from cgroup
 */
function getCgroupMemoryLimit(): number | undefined {
  // cgroup v2: /sys/fs/cgroup/memory.max
  // cgroup v1: /sys/fs/cgroup/memory/memory.limit_in_bytes
  const value = readCgroupValue(
    '/sys/fs/cgroup/memory/memory.limit_in_bytes',
    '/sys/fs/cgroup/memory.max'
  )
  // "max" in cgroup v2 or very large values mean no limit
  if (value && value < 9223372036854771712) {
    return value
  }
  return undefined
}

/**
 * Get CPU limit in cores from cgroup
 */
function getCgroupCpuLimitCores(): number | undefined {
  try {
    // cgroup v2: /sys/fs/cgroup/cpu.max (format: "quota period")
    const v2Path = '/sys/fs/cgroup/cpu.max'
    if (fs.existsSync(v2Path)) {
      const content = fs.readFileSync(v2Path, 'utf-8').trim()
      if (content !== 'max') {
        const [quota, period] = content.split(' ').map((s) => parseInt(s, 10))
        if (quota > 0 && period > 0) {
          return quota / period
        }
      }
    }

    // cgroup v1: quota and period are separate files
    const quotaPath = '/sys/fs/cgroup/cpu/cpu.cfs_quota_us'
    const periodPath = '/sys/fs/cgroup/cpu/cpu.cfs_period_us'
    if (fs.existsSync(quotaPath) && fs.existsSync(periodPath)) {
      const quota = parseInt(fs.readFileSync(quotaPath, 'utf-8').trim(), 10)
      const period = parseInt(fs.readFileSync(periodPath, 'utf-8').trim(), 10)
      if (quota > 0 && period > 0) {
        return quota / period
      }
    }
  } catch {
    // Ignore errors
  }
  return undefined
}

/**
 * Get CPU throttled time from cgroup
 */
function getCgroupCpuThrottledSeconds(): number | undefined {
  try {
    // cgroup v2: /sys/fs/cgroup/cpu.stat contains throttled_usec
    const v2Path = '/sys/fs/cgroup/cpu.stat'
    if (fs.existsSync(v2Path)) {
      const content = fs.readFileSync(v2Path, 'utf-8')
      const match = content.match(/throttled_usec\s+(\d+)/)
      if (match) {
        return parseInt(match[1], 10) / 1_000_000
      }
    }

    // cgroup v1: /sys/fs/cgroup/cpu/cpu.stat contains throttled_time (ns)
    const v1Path = '/sys/fs/cgroup/cpu/cpu.stat'
    if (fs.existsSync(v1Path)) {
      const content = fs.readFileSync(v1Path, 'utf-8')
      const match = content.match(/throttled_time\s+(\d+)/)
      if (match) {
        return parseInt(match[1], 10) / 1_000_000_000
      }
    }
  } catch {
    // Ignore errors
  }
  return undefined
}

/**
 * Get filesystem usage from statvfs (root filesystem)
 */
function getFilesystemUsage(): number | undefined {
  if (process.platform !== 'linux' && process.platform !== 'darwin') {
    return undefined
  }

  try {
    // Use df command to get filesystem usage (works on Linux and macOS)
    const { execSync } = require('child_process')
    const output = execSync('df -B1 / 2>/dev/null', { encoding: 'utf-8' })
    const lines = output.trim().split('\n')
    if (lines.length >= 2) {
      const parts = lines[1].split(/\s+/)
      if (parts.length >= 4) {
        const used = parseInt(parts[2], 10)
        if (!isNaN(used)) {
          return used
        }
      }
    }
  } catch {
    // Ignore errors
  }
  return undefined
}

/**
 * Get node pressure conditions from cgroup pressure files
 */
function getNodePressure(): {
  memory: boolean | undefined
  disk: boolean | undefined
  pid: boolean | undefined
} {
  const result = { memory: undefined as boolean | undefined, disk: undefined as boolean | undefined, pid: undefined as boolean | undefined }

  if (process.platform !== 'linux') {
    return result
  }

  const extractAvg10 = (line: string): number | undefined => {
    const match = line.match(/avg10=(\d+\.?\d*)/)
    return match ? parseFloat(match[1]) : undefined
  }

  // Memory pressure
  try {
    const content = fs.readFileSync('/sys/fs/cgroup/memory.pressure', 'utf-8')
    for (const line of content.split('\n')) {
      if (line.startsWith('some')) {
        const avg10 = extractAvg10(line)
        if (avg10 !== undefined) {
          result.memory = avg10 > 10.0
        }
      }
    }
  } catch {
    // Not available
  }

  // IO/Disk pressure
  try {
    const content = fs.readFileSync('/sys/fs/cgroup/io.pressure', 'utf-8')
    for (const line of content.split('\n')) {
      if (line.startsWith('some')) {
        const avg10 = extractAvg10(line)
        if (avg10 !== undefined) {
          result.disk = avg10 > 10.0
        }
      }
    }
  } catch {
    // Not available
  }

  // CPU/PID pressure
  try {
    const content = fs.readFileSync('/sys/fs/cgroup/cpu.pressure', 'utf-8')
    for (const line of content.split('\n')) {
      if (line.startsWith('some')) {
        const avg10 = extractAvg10(line)
        if (avg10 !== undefined) {
          result.pid = avg10 > 10.0
        }
      }
    }
  } catch {
    // Not available
  }

  return result
}

/**
 * Get Kubernetes core metrics from downward API / cgroup
 */
function getKubernetesCoreMetrics(): KubernetesCoreMetrics | undefined {
  if (!isKubernetes()) {
    return undefined
  }

  const metrics: KubernetesCoreMetrics = {
    uptime_seconds: Math.floor((Date.now() - processStartTime) / 1000),
    pod_phase: 'Running', // If we're executing, we're running
    pod_ready: true,
    cpu_usage_cores: getCgroupCpuUsageCores(),
    memory_working_set_bytes: getCgroupMemoryWorkingSet(),
  }

  // Try to read container restarts from downward API env var
  const restartsEnv = process.env.CONTAINER_RESTARTS
  if (restartsEnv) {
    metrics.container_restarts_total = parseInt(restartsEnv, 10)
  }

  // Last termination reason (set by K8s via downward API)
  metrics.last_termination_reason = process.env.LAST_TERMINATION_REASON

  return metrics
}

/**
 * Get Kubernetes resource metrics (limits, requests, throttling)
 */
function getKubernetesResourceMetrics(): KubernetesResourceMetrics | undefined {
  if (!isKubernetes()) {
    return undefined
  }

  const metrics: KubernetesResourceMetrics = {}

  // CPU limits from cgroup
  metrics.cpu_limits_cores = getCgroupCpuLimitCores()

  // Memory limits from cgroup
  metrics.memory_limits_bytes = getCgroupMemoryLimit()

  // CPU throttling
  metrics.cpu_throttled_seconds_total = getCgroupCpuThrottledSeconds()

  // Requests come from downward API (env vars set by K8s)
  const cpuRequest = process.env.CPU_REQUEST
  if (cpuRequest) {
    // Parse "100m" (millicores) or "0.5" (cores)
    if (cpuRequest.endsWith('m')) {
      metrics.cpu_requests_cores = parseInt(cpuRequest, 10) / 1000
    } else {
      metrics.cpu_requests_cores = parseFloat(cpuRequest)
    }
  }

  const memRequest = process.env.MEMORY_REQUEST
  if (memRequest) {
    // Parse "256Mi", "1Gi", etc.
    metrics.memory_requests_bytes = parseK8sMemory(memRequest)
  }

  return Object.keys(metrics).length > 0 ? metrics : undefined
}

/**
 * Parse Kubernetes memory strings (e.g., "256Mi", "1Gi", "1024")
 */
function parseK8sMemory(value: string): number | undefined {
  const match = value.match(/^(\d+(?:\.\d+)?)(Ki|Mi|Gi|Ti|Pi|Ei|K|M|G|T|P|E)?$/)
  if (!match) return undefined

  const num = parseFloat(match[1])
  const unit = match[2] || ''

  const multipliers: Record<string, number> = {
    '': 1,
    K: 1000,
    M: 1000 ** 2,
    G: 1000 ** 3,
    T: 1000 ** 4,
    P: 1000 ** 5,
    E: 1000 ** 6,
    Ki: 1024,
    Mi: 1024 ** 2,
    Gi: 1024 ** 3,
    Ti: 1024 ** 4,
    Pi: 1024 ** 5,
    Ei: 1024 ** 6,
  }

  return num * (multipliers[unit] || 1)
}

/**
 * Get Kubernetes extended metrics
 */
function getKubernetesExtendedMetrics(): KubernetesExtendedMetrics | undefined {
  if (!isKubernetes()) {
    return undefined
  }

  const metrics: KubernetesExtendedMetrics = {}

  // Network stats from /proc/net/dev (container namespace)
  const netStats = getNetworkStats()
  if (netStats) {
    metrics.network_rx_bytes_total = netStats.rx
    metrics.network_tx_bytes_total = netStats.tx
  }

  // Filesystem usage
  metrics.fs_usage_bytes = getFilesystemUsage()

  // Node pressure conditions from cgroup v2 pressure files
  const pressure = getNodePressure()
  metrics.node_memory_pressure = pressure.memory
  metrics.node_disk_pressure = pressure.disk
  metrics.node_pid_pressure = pressure.pid

  return Object.keys(metrics).some((k) => metrics[k as keyof KubernetesExtendedMetrics] !== undefined)
    ? metrics
    : undefined
}

/**
 * Try to get network I/O stats (Linux only via /proc)
 */
function getNetworkStats(): { rx: number; tx: number } | undefined {
  if (process.platform !== 'linux') {
    return undefined
  }

  try {
    const netDev = fs.readFileSync('/proc/net/dev', 'utf-8')
    const lines = netDev.split('\n').slice(2) // Skip headers

    let totalRx = 0
    let totalTx = 0

    for (const line of lines) {
      const parts = line.trim().split(/\s+/)
      if (parts.length >= 10 && !parts[0].startsWith('lo:')) {
        // Skip loopback
        totalRx += parseInt(parts[1], 10) || 0
        totalTx += parseInt(parts[9], 10) || 0
      }
    }

    return { rx: totalRx, tx: totalTx }
  } catch {
    return undefined
  }
}

/**
 * Try to get disk I/O stats (Linux only via /proc)
 */
function getDiskStats(): { read: number; write: number } | undefined {
  if (process.platform !== 'linux') {
    return undefined
  }

  try {
    const diskStats = fs.readFileSync('/proc/diskstats', 'utf-8')
    const lines = diskStats.split('\n')

    let totalRead = 0
    let totalWrite = 0

    for (const line of lines) {
      const parts = line.trim().split(/\s+/)
      if (parts.length >= 14) {
        // Sectors read (index 5) and written (index 9), 512 bytes per sector
        totalRead += (parseInt(parts[5], 10) || 0) * 512
        totalWrite += (parseInt(parts[9], 10) || 0) * 512
      }
    }

    return { read: totalRead, write: totalWrite }
  } catch {
    return undefined
  }
}

/**
 * Get open file descriptor count (Linux/macOS)
 */
function getOpenFileDescriptors(): number | undefined {
  if (process.platform === 'linux') {
    try {
      const fdDir = `/proc/${process.pid}/fd`
      const fds = fs.readdirSync(fdDir)
      return fds.length
    } catch {
      return undefined
    }
  }

  // macOS: would need native module, skip for now
  return undefined
}

/**
 * Collect all metrics
 */
export function collectMetrics(): WorkerMetrics {
  const now = Date.now()
  const uptimeSecs = Math.floor((now - processStartTime) / 1000)

  // Calculate invocation metrics for the window
  const windowDurationSecs = (now - lastInvocationWindow) / 1000
  const invocationsPerSec = windowDurationSecs > 0 ? invocationCount / windowDurationSecs : 0
  const avgLatencyMs = invocationCount > 0 ? totalLatencyMs / invocationCount : 0

  // Prepare to collect extended metrics
  let networkRx: number | undefined
  let networkTx: number | undefined
  let diskRead: number | undefined
  let diskWrite: number | undefined

  // Get network stats delta
  const currentNetStats = getNetworkStats()
  if (currentNetStats && lastNetworkStats) {
    networkRx = currentNetStats.rx - lastNetworkStats.rx
    networkTx = currentNetStats.tx - lastNetworkStats.tx
  }
  if (currentNetStats) {
    lastNetworkStats = { ...currentNetStats, time: now }
  }

  // Get disk stats delta
  const currentDiskStats = getDiskStats()
  if (currentDiskStats && lastDiskStats) {
    diskRead = currentDiskStats.read - lastDiskStats.read
    diskWrite = currentDiskStats.write - lastDiskStats.write
  }
  if (currentDiskStats) {
    lastDiskStats = { ...currentDiskStats, time: now }
  }

  const memMetrics = getMemoryMetrics()

  const process_metrics: ProcessMetrics = {
    cpu_percent: getCpuPercent(),
    memory_used_bytes: memMetrics.used,
    memory_total_bytes: memMetrics.total,
    process_uptime_secs: uptimeSecs,
  }

  const performance: PerformanceMetrics = {
    thread_count: getThreadCount(),
    open_connections: 1, // WebSocket connection to engine
    invocations_per_sec: invocationsPerSec,
    avg_latency_ms: avgLatencyMs,
  }

  const extended: ExtendedMetrics = {
    disk_read_bytes: diskRead,
    disk_write_bytes: diskWrite,
    network_rx_bytes: networkRx,
    network_tx_bytes: networkTx,
    open_file_descriptors: getOpenFileDescriptors(),
    error_count: errorCount,
  }

  // Reset invocation counters for next window
  invocationCount = 0
  totalLatencyMs = 0
  lastInvocationWindow = now

  const metrics: WorkerMetrics = {
    collected_at_ms: now,
    process: process_metrics,
    performance,
    extended,
  }

  // Add K8s metrics if running in Kubernetes
  if (isKubernetes()) {
    metrics.k8s_identifiers = getKubernetesIdentifiers()
    metrics.k8s_core = getKubernetesCoreMetrics()
    metrics.k8s_resources = getKubernetesResourceMetrics()
    metrics.k8s_extended = getKubernetesExtendedMetrics()
  }

  return metrics
}

/**
 * Create a metrics collector that auto-reports to the bridge
 */
export function createMetricsReporter(
  reportFn: (metrics: Omit<WorkerMetrics, 'collected_at_ms'>) => void,
  intervalMs: number = 5000
): { start: () => void; stop: () => void } {
  let timer: NodeJS.Timeout | null = null

  return {
    start: () => {
      if (timer) return

      // Initial warmup call to establish baselines
      collectMetrics()

      timer = setInterval(() => {
        const metrics = collectMetrics()
        reportFn(metrics)
      }, intervalMs)
    },
    stop: () => {
      if (timer) {
        clearInterval(timer)
        timer = null
      }
    },
  }
}

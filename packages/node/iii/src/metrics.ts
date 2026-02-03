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

let lastCpuUsage: NodeJS.CpuUsage | null = null
let lastCpuTime: [number, number] | null = null
let lastNetworkStats: { rx: number; tx: number; time: number } | null = null
let lastDiskStats: { read: number; write: number; time: number } | null = null

let invocationCount = 0
let totalLatencyMs = 0
let errorCount = 0
let lastInvocationWindow = Date.now()

const processStartTime = Date.now()

export function recordInvocation(latencyMs: number, isError: boolean = false): void {
  invocationCount++
  totalLatencyMs += latencyMs
  if (isError) {
    errorCount++
  }
}

function getCpuPercent(): number {
  const currentUsage = process.cpuUsage(lastCpuUsage ?? undefined)
  const currentTime = process.hrtime()

  if (lastCpuUsage === null || lastCpuTime === null) {
    lastCpuUsage = process.cpuUsage()
    lastCpuTime = process.hrtime()
    return 0
  }

  const elapsedTime = (currentTime[0] - lastCpuTime[0]) * 1e6 + (currentTime[1] - lastCpuTime[1]) / 1000

  if (elapsedTime <= 0) {
    return 0
  }

  const cpuTimeUsed = currentUsage.user + currentUsage.system
  const cpuPercent = (cpuTimeUsed / elapsedTime) * 100

  lastCpuUsage = process.cpuUsage()
  lastCpuTime = process.hrtime()

  return Math.min(cpuPercent, 100 * os.cpus().length)
}

function getMemoryMetrics(): { used: number; total: number } {
  const memUsage = process.memoryUsage()
  return {
    used: memUsage.rss,
    total: os.totalmem(),
  }
}

function getThreadCount(): number {
  const uvThreadPoolSize = parseInt(process.env.UV_THREADPOOL_SIZE || '4', 10)
  return 1 + uvThreadPoolSize
}

function isKubernetes(): boolean {
  return !!process.env.KUBERNETES_SERVICE_HOST
}

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

function readK8sFile(filename: string): string | undefined {
  const basePath = '/var/run/secrets/kubernetes.io/serviceaccount'
  const filePath = path.join(basePath, filename)
  try {
    if (fs.existsSync(filePath)) {
      return fs.readFileSync(filePath, 'utf-8').trim()
    }
  } catch {
  }
  return undefined
}

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
    }
  }
  return undefined
}

function getCgroupCpuUsageCores(): number | undefined {
  try {
    const v2Path = '/sys/fs/cgroup/cpu.stat'
    if (fs.existsSync(v2Path)) {
      const content = fs.readFileSync(v2Path, 'utf-8')
      const match = content.match(/usage_usec\s+(\d+)/)
      if (match) {
        const usec = parseInt(match[1], 10)
        return usec / 1_000_000
      }
    }

    const v1Path = '/sys/fs/cgroup/cpu/cpuacct.usage'
    if (fs.existsSync(v1Path)) {
      const nsec = parseInt(fs.readFileSync(v1Path, 'utf-8').trim(), 10)
      return nsec / 1_000_000_000
    }
  } catch {
  }
  return undefined
}

function getCgroupMemoryWorkingSet(): number | undefined {
  return readCgroupValue(
    '/sys/fs/cgroup/memory/memory.usage_in_bytes',
    '/sys/fs/cgroup/memory.current'
  )
}

function getCgroupMemoryLimit(): number | undefined {
  const value = readCgroupValue(
    '/sys/fs/cgroup/memory/memory.limit_in_bytes',
    '/sys/fs/cgroup/memory.max'
  )
  if (value && value < Number.MAX_SAFE_INTEGER) {
    return value
  }
  return undefined
}

function getCgroupCpuLimitCores(): number | undefined {
  try {
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
  }
  return undefined
}

function getCgroupCpuThrottledSeconds(): number | undefined {
  try {
    const v2Path = '/sys/fs/cgroup/cpu.stat'
    if (fs.existsSync(v2Path)) {
      const content = fs.readFileSync(v2Path, 'utf-8')
      const match = content.match(/throttled_usec\s+(\d+)/)
      if (match) {
        return parseInt(match[1], 10) / 1_000_000
      }
    }

    const v1Path = '/sys/fs/cgroup/cpu/cpu.stat'
    if (fs.existsSync(v1Path)) {
      const content = fs.readFileSync(v1Path, 'utf-8')
      const match = content.match(/throttled_time\s+(\d+)/)
      if (match) {
        return parseInt(match[1], 10) / 1_000_000_000
      }
    }
  } catch {
  }
  return undefined
}

function getFilesystemUsage(): number | undefined {
  if (process.platform !== 'linux' && process.platform !== 'darwin') {
    return undefined
  }

  try {
    const { execSync } = require('child_process')
    const output = execSync('df -B1 / 2>/dev/null', { encoding: 'utf-8', timeout: 1000 })
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
  }
  return undefined
}

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
  }

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
  }

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
  }

  return result
}

function getKubernetesCoreMetrics(): KubernetesCoreMetrics | undefined {
  if (!isKubernetes()) {
    return undefined
  }

  const metrics: KubernetesCoreMetrics = {
    uptime_seconds: Math.floor((Date.now() - processStartTime) / 1000),
    pod_phase: 'Running',
    pod_ready: true,
    cpu_usage_cores: getCgroupCpuUsageCores(),
    memory_working_set_bytes: getCgroupMemoryWorkingSet(),
  }

  const restartsEnv = process.env.CONTAINER_RESTARTS
  if (restartsEnv) {
    metrics.container_restarts_total = parseInt(restartsEnv, 10)
  }

  metrics.last_termination_reason = process.env.LAST_TERMINATION_REASON

  return metrics
}

function getKubernetesResourceMetrics(): KubernetesResourceMetrics | undefined {
  if (!isKubernetes()) {
    return undefined
  }

  const metrics: KubernetesResourceMetrics = {}

  metrics.cpu_limits_cores = getCgroupCpuLimitCores()
  metrics.memory_limits_bytes = getCgroupMemoryLimit()
  metrics.cpu_throttled_seconds_total = getCgroupCpuThrottledSeconds()

  const cpuRequest = process.env.CPU_REQUEST
  if (cpuRequest) {
    if (cpuRequest.endsWith('m')) {
      metrics.cpu_requests_cores = parseInt(cpuRequest, 10) / 1000
    } else {
      metrics.cpu_requests_cores = parseFloat(cpuRequest)
    }
  }

  const memRequest = process.env.MEMORY_REQUEST
  if (memRequest) {
    metrics.memory_requests_bytes = parseK8sMemory(memRequest)
  }

  return Object.keys(metrics).length > 0 ? metrics : undefined
}

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

function getKubernetesExtendedMetrics(): KubernetesExtendedMetrics | undefined {
  if (!isKubernetes()) {
    return undefined
  }

  const metrics: KubernetesExtendedMetrics = {}

  const netStats = getNetworkStats()
  if (netStats) {
    metrics.network_rx_bytes_total = netStats.rx
    metrics.network_tx_bytes_total = netStats.tx
  }

  metrics.fs_usage_bytes = getFilesystemUsage()

  const pressure = getNodePressure()
  metrics.node_memory_pressure = pressure.memory
  metrics.node_disk_pressure = pressure.disk
  metrics.node_pid_pressure = pressure.pid

  return Object.keys(metrics).some((k) => metrics[k as keyof KubernetesExtendedMetrics] !== undefined)
    ? metrics
    : undefined
}

function getNetworkStats(): { rx: number; tx: number } | undefined {
  if (process.platform !== 'linux') {
    return undefined
  }

  try {
    const netDev = fs.readFileSync('/proc/net/dev', 'utf-8')
    const lines = netDev.split('\n').slice(2)

    let totalRx = 0
    let totalTx = 0

    for (const line of lines) {
      const parts = line.trim().split(/\s+/)
      if (parts.length >= 10 && !parts[0].startsWith('lo:')) {
        totalRx += parseInt(parts[1], 10) || 0
        totalTx += parseInt(parts[9], 10) || 0
      }
    }

    return { rx: totalRx, tx: totalTx }
  } catch {
    return undefined
  }
}

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
        totalRead += (parseInt(parts[5], 10) || 0) * 512
        totalWrite += (parseInt(parts[9], 10) || 0) * 512
      }
    }

    return { read: totalRead, write: totalWrite }
  } catch {
    return undefined
  }
}

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

  return undefined
}

export function collectMetrics(): WorkerMetrics {
  const now = Date.now()
  const uptimeSecs = Math.floor((now - processStartTime) / 1000)

  const windowDurationSecs = (now - lastInvocationWindow) / 1000
  const invocationsPerSec = windowDurationSecs > 0 ? invocationCount / windowDurationSecs : 0
  const avgLatencyMs = invocationCount > 0 ? totalLatencyMs / invocationCount : 0

  let networkRx: number | undefined
  let networkTx: number | undefined
  let diskRead: number | undefined
  let diskWrite: number | undefined

  const currentNetStats = getNetworkStats()
  if (currentNetStats && lastNetworkStats) {
    networkRx = currentNetStats.rx - lastNetworkStats.rx
    networkTx = currentNetStats.tx - lastNetworkStats.tx
  }
  if (currentNetStats) {
    lastNetworkStats = { ...currentNetStats, time: now }
  }

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

  invocationCount = 0
  totalLatencyMs = 0
  errorCount = 0
  lastInvocationWindow = now

  const metrics: WorkerMetrics = {
    collected_at_ms: now,
    process: process_metrics,
    performance,
    extended,
  }

  if (isKubernetes()) {
    metrics.k8s_identifiers = getKubernetesIdentifiers()
    metrics.k8s_core = getKubernetesCoreMetrics()
    metrics.k8s_resources = getKubernetesResourceMetrics()
    metrics.k8s_extended = getKubernetesExtendedMetrics()
  }

  return metrics
}

export function createMetricsReporter(
  reportFn: (metrics: Omit<WorkerMetrics, 'collected_at_ms'>) => void,
  intervalMs: number = 5000
): { start: () => void; stop: () => void } {
  let timer: NodeJS.Timeout | null = null

  return {
    start: () => {
      if (timer) return

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

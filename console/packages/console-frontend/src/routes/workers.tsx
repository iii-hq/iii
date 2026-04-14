import { useQuery } from '@tanstack/react-query'
import { createFileRoute, useNavigate } from '@tanstack/react-router'
import {
  Activity,
  Check,
  ChevronRight,
  Clock,
  Copy,
  Cpu,
  Eye,
  EyeOff,
  HardDrive,
  Monitor,
  RefreshCw,
  X,
  Zap,
} from 'lucide-react'
import { useEffect, useState } from 'react'
import { z } from 'zod'
import type { WorkerInfo } from '@/api'
import { workersQuery } from '@/api'
import { Badge, Button } from '@/components/ui/card'
import { EmptyState } from '@/components/ui/empty-state'
import { PageHeader } from '@/components/ui/page-header'
import { SearchBar } from '@/components/ui/search-bar'
import { Skeleton } from '@/components/ui/skeleton'

const workersSearchSchema = z.object({
  worker: z
    .string()
    .optional()
    .transform((s) => (s && s.trim() !== '' ? s : undefined)),
  q: z
    .string()
    .optional()
    .transform((s) => (s && s.trim() !== '' ? s : undefined)),
  runtime: z
    .string()
    .optional()
    .transform((s) => (s && s.trim() !== '' ? s : undefined)),
  internal: z
    .union([z.boolean(), z.string()])
    .optional()
    .transform((v) => v === true || v === 'true'),
})

export const Route = createFileRoute('/workers')({
  validateSearch: workersSearchSchema,
  component: WorkersPage,
  loader: ({ context: { queryClient } }) => {
    queryClient.prefetchQuery(workersQuery)
  },
})

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

function formatUptime(seconds: number): string {
  if (seconds < 60) return `${Math.floor(seconds)}s`
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m`
  if (seconds < 86400) return `${Math.floor(seconds / 3600)}h ${Math.floor((seconds % 3600) / 60)}m`
  return `${Math.floor(seconds / 86400)}d ${Math.floor((seconds % 86400) / 3600)}h`
}

function formatConnectedAt(ms: number): string {
  const now = Date.now()
  const diff = now - ms
  const seconds = Math.floor(diff / 1000)
  if (seconds < 60) return `${seconds}s ago`
  const minutes = Math.floor(seconds / 60)
  if (minutes < 60) return `${minutes}m ago`
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return `${hours}h ago`
  const days = Math.floor(hours / 24)
  return `${days}d ago`
}

function getWorkerDisplayName(worker: WorkerInfo): string {
  if (worker.name) return worker.name
  return `${worker.id.slice(0, 8)}...`
}

function getRuntimeBadgeClass(runtime: string | null): string {
  switch (runtime?.toLowerCase()) {
    case 'node':
    case 'nodejs':
      return 'bg-green-500/10 text-green-400 border-green-500/30'
    case 'python':
      return 'bg-blue-500/10 text-blue-400 border-blue-500/30'
    case 'rust':
      return 'bg-orange-500/10 text-orange-400 border-orange-500/30'
    default:
      return 'bg-white/5 text-muted border-border'
  }
}

function getIsolationBadgeClass(isolation: string | null): string {
  switch (isolation?.toLowerCase()) {
    case 'libkrun':
      return 'bg-purple-500/10 text-purple-400 border-purple-500/30'
    case 'docker':
      return 'bg-blue-500/10 text-blue-400 border-blue-500/30'
    case 'kubernetes':
    case 'k8s':
      return 'bg-sky-500/10 text-sky-400 border-sky-500/30'
    case 'firecracker':
      return 'bg-amber-500/10 text-amber-400 border-amber-500/30'
    default:
      return 'bg-white/5 text-muted border-border'
  }
}

function getStatusDotClass(status: string): string {
  switch (status) {
    case 'connected':
    case 'available':
      return 'bg-success shadow-[0_0_4px_var(--success)]'
    case 'busy':
      return 'bg-warning shadow-[0_0_4px_var(--warning)]'
    default:
      return 'bg-error shadow-[0_0_4px_var(--error)]'
  }
}

function WorkersPage() {
  const navigate = useNavigate({ from: '/workers' })
  const search = Route.useSearch()
  const [copied, setCopied] = useState<string | null>(null)

  const searchQuery = search.q ?? ''
  const showInternal = search.internal
  const filterRuntime = search.runtime ?? null
  const workerParam = search.worker

  const {
    data: workersData,
    isLoading,
    refetch,
  } = useQuery({ ...workersQuery, refetchInterval: 5000 })

  const allWorkers = workersData?.workers || []
  const userWorkers = allWorkers.filter((w) => !w.internal)

  const selectedWorker =
    workerParam != null ? (allWorkers.find((w) => w.id === workerParam) ?? null) : null

  useEffect(() => {
    if (!workersData || isLoading || !workerParam) return
    const exists = workersData.workers.some((w) => w.id === workerParam)
    if (!exists) {
      navigate({
        search: (prev) => ({ ...prev, worker: undefined }),
        replace: true,
      })
    }
  }, [workersData, isLoading, workerParam, navigate])

  const runtimes = Array.from(
    new Set(allWorkers.filter((w) => !w.internal && w.runtime).map((w) => w.runtime as string)),
  )

  const filteredWorkers = allWorkers.filter((w) => {
    if (!showInternal && w.internal) return false
    if (filterRuntime && w.runtime?.toLowerCase() !== filterRuntime.toLowerCase()) return false
    if (searchQuery) {
      const q = searchQuery.toLowerCase()
      const name = w.name?.toLowerCase() || ''
      const id = w.id.toLowerCase()
      const ip = w.ip_address?.toLowerCase() || ''
      const rt = w.runtime?.toLowerCase() || ''
      if (!name.includes(q) && !id.includes(q) && !ip.includes(q) && !rt.includes(q)) return false
    }
    return true
  })

  const copyToClipboard = (text: string, key: string) => {
    navigator.clipboard.writeText(text)
    setCopied(key)
    setTimeout(() => setCopied(null), 2000)
  }

  const handleSelectWorker = (worker: WorkerInfo) => {
    navigate({
      search: (prev) => ({
        ...prev,
        worker: prev.worker === worker.id ? undefined : worker.id,
      }),
    })
  }

  return (
    <div className="flex flex-col h-full bg-background text-foreground">
      <PageHeader
        icon={Monitor}
        title="Workers"
        actions={
          <>
            <Button
              variant={showInternal ? 'accent' : 'ghost'}
              size="sm"
              onClick={() =>
                navigate({
                  search: (prev) => ({
                    ...prev,
                    internal: prev.internal ? undefined : true,
                  }),
                })
              }
              className="h-6 md:h-7 text-[10px] md:text-xs px-2"
            >
              {showInternal ? (
                <Eye className="w-3 h-3 md:mr-1.5" />
              ) : (
                <EyeOff className="w-3 h-3 md:mr-1.5" />
              )}
              <span className={`hidden md:inline ${showInternal ? '' : 'line-through opacity-60'}`}>
                Internal
              </span>
            </Button>
            <Button
              variant="ghost"
              size="sm"
              onClick={() => refetch()}
              disabled={isLoading}
              className="h-7 text-xs"
            >
              <RefreshCw className={`w-3.5 h-3.5 mr-1.5 ${isLoading ? 'animate-spin' : ''}`} />
              Refresh
            </Button>
          </>
        }
      >
        <Badge variant="success" className="gap-1 text-[10px] md:text-xs">
          <Activity className="w-2.5 h-2.5 md:w-3 md:h-3" />
          {userWorkers.length}
        </Badge>
      </PageHeader>

      <SearchBar
        value={searchQuery}
        onChange={(value) =>
          navigate({
            search: (prev) => ({
              ...prev,
              q: value.trim() ? value : undefined,
            }),
            replace: true,
          })
        }
        placeholder="Search workers by name, ID, runtime, IP..."
      />

      {runtimes.length > 0 && (
        <div className="flex items-center gap-1 px-3 py-2 border-b border-border bg-dark-gray/20">
          <button
            type="button"
            onClick={() =>
              navigate({
                search: (prev) => ({ ...prev, runtime: undefined }),
                replace: true,
              })
            }
            className={`flex items-center gap-1 px-2 py-1 rounded text-[10px] font-medium tracking-wider uppercase transition-colors ${
              !filterRuntime
                ? 'bg-white/10 text-foreground'
                : 'text-muted hover:text-foreground hover:bg-white/5'
            }`}
          >
            All
            <span className="tabular-nums">
              {showInternal ? allWorkers.length : userWorkers.length}
            </span>
          </button>
          {runtimes.map((rt) => {
            const count = allWorkers.filter(
              (w) => (showInternal || !w.internal) && w.runtime?.toLowerCase() === rt.toLowerCase(),
            ).length
            const isActive = filterRuntime === rt
            return (
              <button
                key={rt}
                type="button"
                onClick={() =>
                  navigate({
                    search: (prev) => ({
                      ...prev,
                      runtime: isActive ? undefined : rt,
                    }),
                    replace: true,
                  })
                }
                className={`flex items-center gap-1 px-2 py-1 rounded text-[10px] font-medium tracking-wider uppercase transition-colors ${isActive ? getRuntimeBadgeClass(rt) : 'text-muted hover:text-foreground hover:bg-white/5'}`}
              >
                <span className="hidden lg:inline">{rt}</span>
                <span className="tabular-nums">{count}</span>
              </button>
            )
          })}
        </div>
      )}

      <div className={`flex-1 flex overflow-hidden ${workerParam ? 'divide-x divide-border' : ''}`}>
        <div className="flex-1 overflow-y-auto p-5 space-y-2">
          {isLoading ? (
            <div className="space-y-3 py-4">
              <Skeleton className="h-16 w-full" />
              <Skeleton className="h-16 w-full" />
              <Skeleton className="h-16 w-full" />
            </div>
          ) : filteredWorkers.length === 0 ? (
            searchQuery || filterRuntime ? (
              <div className="flex flex-col items-center justify-center py-12">
                <Monitor className="w-12 h-12 text-muted/30 mb-4" />
                <div className="font-sans font-semibold text-base text-foreground mb-1">
                  No workers found
                </div>
                <div className="font-sans text-[13px] text-secondary">
                  Try a different search or filter
                </div>
              </div>
            ) : (
              <EmptyState
                icon={Monitor}
                title="No workers connected"
                description="Start a worker using the SDK to see it here"
              />
            )
          ) : (
            filteredWorkers.map((worker) => {
              const isSelected = workerParam === worker.id
              return (
                <button
                  key={worker.id}
                  type="button"
                  onClick={() => handleSelectWorker(worker)}
                  className={`group flex items-center gap-3 px-3 py-3 rounded-[var(--radius-lg)] cursor-pointer transition-all w-full text-left ${
                    isSelected
                      ? 'bg-primary/10 border border-primary/30 ring-1 ring-primary/20'
                      : 'bg-elevated border border-transparent hover:bg-hover hover:border-border'
                  }`}
                >
                  <div className="shrink-0 relative">
                    <Monitor className="w-4 h-4 text-muted" />
                    <div
                      className={`absolute -top-0.5 -right-0.5 w-2 h-2 rounded-full ${getStatusDotClass(worker.status)}`}
                    />
                  </div>

                  <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2 flex-wrap">
                      <span
                        className={`font-mono text-[13px] font-medium ${isSelected ? 'text-primary' : 'text-foreground'}`}
                      >
                        {getWorkerDisplayName(worker)}
                      </span>
                      {worker.telemetry?.project_name && (
                        <span className="text-[11px] text-secondary font-sans truncate">
                          {worker.telemetry.project_name}
                        </span>
                      )}
                      {worker.runtime && (
                        <Badge
                          variant="outline"
                          className={`text-[9px] uppercase ${getRuntimeBadgeClass(worker.runtime)}`}
                        >
                          {worker.runtime}
                          {worker.version ? ` ${worker.version}` : ''}
                        </Badge>
                      )}
                      {worker.isolation && (
                        <Badge
                          variant="outline"
                          className={`text-[9px] uppercase max-w-[8rem] truncate ${getIsolationBadgeClass(worker.isolation)}`}
                        >
                          {worker.isolation.slice(0, 64)}
                        </Badge>
                      )}
                    </div>
                    <div className="flex items-center gap-3 mt-0.5 text-[10px] text-muted font-mono">
                      <span>{worker.ip_address}</span>
                      {worker.pid != null && <span>pid {worker.pid}</span>}
                      <span className="flex items-center gap-1">
                        <Zap className="w-2.5 h-2.5" />
                        {worker.function_count} fn
                      </span>
                      {worker.active_invocations > 0 && (
                        <span className="flex items-center gap-1 text-amber-400">
                          <Activity className="w-2.5 h-2.5" />
                          {worker.active_invocations} active
                        </span>
                      )}
                      <span className="flex items-center gap-1">
                        <Clock className="w-2.5 h-2.5" />
                        {formatConnectedAt(worker.connected_at_ms)}
                      </span>
                    </div>
                  </div>

                  <ChevronRight
                    className={`w-4 h-4 text-muted shrink-0 transition-transform ${isSelected ? 'rotate-90' : ''}`}
                  />
                </button>
              )
            })
          )}
        </div>

        {selectedWorker && workerParam && (
          <div className="fixed inset-0 z-50 md:relative md:inset-auto w-full md:w-[360px] lg:w-[480px] shrink-0 flex flex-col h-full overflow-hidden bg-background md:bg-dark-gray/20 border-l border-border">
            <div className="px-3 md:px-4 py-2 md:py-3 border-b border-border bg-dark-gray/30 space-y-1.5">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2 min-w-0">
                  <div className="relative shrink-0">
                    <Monitor className="w-4 h-4 text-muted" />
                    <div
                      className={`absolute -top-0.5 -right-0.5 w-2 h-2 rounded-full ${getStatusDotClass(selectedWorker.status)}`}
                    />
                  </div>
                  <h2 className="font-medium text-xs md:text-sm truncate">
                    {getWorkerDisplayName(selectedWorker)}
                  </h2>
                </div>
                <div className="flex items-center gap-1 shrink-0">
                  <button
                    type="button"
                    onClick={() => copyToClipboard(selectedWorker.id, 'id')}
                    className="p-1.5 hover:bg-dark-gray rounded transition-colors"
                    title="Copy worker ID"
                  >
                    {copied === 'id' ? (
                      <Check className="w-3.5 h-3.5 text-success" />
                    ) : (
                      <Copy className="w-3.5 h-3.5 text-muted" />
                    )}
                  </button>
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() =>
                      navigate({
                        search: (prev) => ({ ...prev, worker: undefined }),
                      })
                    }
                    className="h-7 w-7 md:h-6 md:w-6 p-0"
                  >
                    <X className="w-4 h-4" />
                  </Button>
                </div>
              </div>
              <div className="text-[10px] text-muted font-mono truncate">{selectedWorker.id}</div>
            </div>

            <div className="flex-1 overflow-y-auto p-4 space-y-5">
              <div>
                <div className="font-sans font-semibold text-xs text-muted uppercase tracking-[0.04em] mb-3">
                  Info
                </div>
                <div className="grid grid-cols-2 gap-2">
                  {[
                    { label: 'Runtime', value: selectedWorker.runtime },
                    { label: 'Version', value: selectedWorker.version },
                    { label: 'OS', value: selectedWorker.os },
                    { label: 'IP', value: selectedWorker.ip_address },
                    {
                      label: 'PID',
                      value: selectedWorker.pid != null ? String(selectedWorker.pid) : null,
                    },
                    { label: 'Isolation', value: selectedWorker.isolation },
                    { label: 'Status', value: selectedWorker.status },
                    {
                      label: 'Connected',
                      value: new Date(selectedWorker.connected_at_ms).toLocaleString(),
                    },
                  ]
                    .filter((item) => item.value)
                    .map((item) => (
                      <div key={item.label} className="bg-dark-gray/50 rounded-lg p-2.5">
                        <div className="font-sans font-semibold text-[9px] text-muted uppercase tracking-[0.08em] mb-1">
                          {item.label}
                        </div>
                        <div className="font-mono text-[11px] text-foreground truncate">
                          {item.value}
                        </div>
                      </div>
                    ))}
                </div>
              </div>

              {selectedWorker.telemetry &&
                (selectedWorker.telemetry.project_name ||
                  selectedWorker.telemetry.language ||
                  selectedWorker.telemetry.framework) && (
                  <div>
                    <div className="font-sans font-semibold text-xs text-muted uppercase tracking-[0.04em] mb-3">
                      Telemetry
                    </div>
                    <div className="grid grid-cols-2 gap-2">
                      {[
                        { label: 'Project', value: selectedWorker.telemetry.project_name },
                        { label: 'Framework', value: selectedWorker.telemetry.framework },
                        { label: 'Language', value: selectedWorker.telemetry.language },
                      ]
                        .filter((item) => item.value)
                        .map((item) => (
                          <div key={item.label} className="bg-dark-gray/50 rounded-lg p-2.5">
                            <div className="font-sans font-semibold text-[9px] text-muted uppercase tracking-[0.08em] mb-1">
                              {item.label}
                            </div>
                            <div className="font-mono text-[11px] text-foreground truncate">
                              {item.value}
                            </div>
                          </div>
                        ))}
                    </div>
                  </div>
                )}

              {selectedWorker.latest_metrics && (
                <div>
                  <div className="font-sans font-semibold text-xs text-muted uppercase tracking-[0.04em] mb-3 flex items-center gap-2">
                    <Activity className="w-3 h-3" />
                    Metrics
                  </div>

                  <div className="space-y-2">
                    <div className="bg-dark-gray/50 rounded-lg p-3">
                      <div className="flex items-center gap-2 mb-2">
                        <HardDrive className="w-3 h-3 text-blue-400" />
                        <span className="font-sans font-semibold text-[10px] text-muted uppercase tracking-wider">
                          Memory
                        </span>
                      </div>
                      <div className="grid grid-cols-2 gap-1.5">
                        {[
                          {
                            label: 'Heap Used',
                            value: formatBytes(selectedWorker.latest_metrics.memory_heap_used),
                          },
                          {
                            label: 'Heap Total',
                            value: formatBytes(selectedWorker.latest_metrics.memory_heap_total),
                          },
                          {
                            label: 'RSS',
                            value: formatBytes(selectedWorker.latest_metrics.memory_rss),
                          },
                          {
                            label: 'External',
                            value: formatBytes(selectedWorker.latest_metrics.memory_external),
                          },
                        ].map((m) => (
                          <div key={m.label}>
                            <div className="text-[9px] text-muted uppercase tracking-wider">
                              {m.label}
                            </div>
                            <div className="font-mono text-[11px] text-blue-400">{m.value}</div>
                          </div>
                        ))}
                      </div>
                    </div>

                    <div className="bg-dark-gray/50 rounded-lg p-3">
                      <div className="flex items-center gap-2 mb-2">
                        <Cpu className="w-3 h-3 text-orange-400" />
                        <span className="font-sans font-semibold text-[10px] text-muted uppercase tracking-wider">
                          CPU
                        </span>
                      </div>
                      <div className="grid grid-cols-2 gap-1.5">
                        {[
                          {
                            label: 'CPU %',
                            value: `${selectedWorker.latest_metrics.cpu_percent.toFixed(1)}%`,
                          },
                          {
                            label: 'Event Loop Lag',
                            value: `${selectedWorker.latest_metrics.event_loop_lag_ms.toFixed(1)}ms`,
                          },
                          {
                            label: 'Uptime',
                            value: formatUptime(selectedWorker.latest_metrics.uptime_seconds),
                          },
                        ].map((m) => (
                          <div key={m.label}>
                            <div className="text-[9px] text-muted uppercase tracking-wider">
                              {m.label}
                            </div>
                            <div className="font-mono text-[11px] text-orange-400">{m.value}</div>
                          </div>
                        ))}
                      </div>
                    </div>
                  </div>
                </div>
              )}

              {selectedWorker.functions.length > 0 && (
                <div>
                  <div className="font-sans font-semibold text-xs text-muted uppercase tracking-[0.04em] mb-3 flex items-center gap-2">
                    <Zap className="w-3 h-3" />
                    Functions ({selectedWorker.functions.length})
                  </div>
                  <div className="space-y-1">
                    {selectedWorker.functions.map((fnId) => (
                      <button
                        key={fnId}
                        type="button"
                        onClick={() => navigate({ to: '/functions', search: { q: fnId } })}
                        className="w-full flex items-center justify-between px-3 py-2 rounded-[var(--radius-md)] bg-dark-gray/40 hover:bg-hover transition-colors text-left"
                      >
                        <span className="font-mono text-[11px] text-yellow truncate">{fnId}</span>
                        <ChevronRight className="w-3 h-3 text-muted shrink-0 ml-2" />
                      </button>
                    ))}
                  </div>
                </div>
              )}
            </div>
          </div>
        )}
      </div>
    </div>
  )
}

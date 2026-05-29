import { useQuery, useQueryClient } from '@tanstack/react-query'
import { createFileRoute, Link, redirect } from '@tanstack/react-router'
import {
  Activity,
  ArrowRight,
  Calendar,
  ChevronRight,
  Database,
  Globe,
  MessageSquare,
  Users,
  Wifi,
  Zap,
} from 'lucide-react'
import { useEffect } from 'react'
import { createMetricsSubscription } from '@/api'
import { useConfig } from '@/api/config-provider'
import { functionsQuery, statusQuery, streamsQuery, triggersQuery } from '@/api/queries'
import { Badge, Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { EmptyState } from '@/components/ui/empty-state'
import { Skeleton } from '@/components/ui/skeleton'

export const Route = createFileRoute('/')({
  component: DashboardPage,
  loader: ({ context: { queryClient } }) => {
    // Use prefetchQuery instead of ensureQueryData to avoid throwing on errors
    // The components will handle loading/error states gracefully
    Promise.allSettled([
      queryClient.prefetchQuery(statusQuery),
      queryClient.prefetchQuery(functionsQuery()),
      queryClient.prefetchQuery(triggersQuery()),
      queryClient.prefetchQuery(streamsQuery),
    ])
    throw redirect({ to: '/workers' })
  },
})

interface MetricsChartProps {
  title: string
  value: number | string
  color: string
  icon: React.ElementType
  href?: string
}

function MetricsChart({ title, value, color, icon: Icon, href }: MetricsChartProps) {
  const content = (
    <div className="bg-elevated rounded-[var(--radius-lg)] border border-border-subtle p-4 transition-all duration-200 group-hover:border-muted/40 group-hover:-translate-y-0.5 cursor-pointer">
      <div className="flex items-center gap-2 mb-2">
        <div className="p-1.5 rounded-md" style={{ backgroundColor: `${color}20` }}>
          <Icon className="w-4 h-4" style={{ color }} />
        </div>
        <span className="font-sans font-semibold text-xs text-muted uppercase tracking-[0.04em]">
          {title}
        </span>
      </div>
      <div className="font-mono text-2xl font-bold">{value}</div>
    </div>
  )

  if (href) {
    return (
      <Link to={href} className="block hover:no-underline group">
        {content}
      </Link>
    )
  }

  return content
}

function DashboardPage() {
  const queryClient = useQueryClient()

  const { data: statusData, isLoading: statusLoading } = useQuery(statusQuery)
  const { data: functionsData, isLoading: functionsLoading } = useQuery(functionsQuery())
  const { data: triggersData, isLoading: triggersLoading } = useQuery(triggersQuery())
  const { data: streamsData } = useQuery(streamsQuery)

  const config = useConfig()
  const loading = statusLoading || functionsLoading || triggersLoading

  // Subscribe to real-time metrics
  useEffect(() => {
    const subscription = createMetricsSubscription(queryClient)
    subscription.connect()

    return () => {
      subscription.disconnect()
    }
  }, [queryClient])

  const status = statusData ?? null
  const triggers = triggersData?.triggers ?? []
  const functions = functionsData?.functions ?? []
  const streams = streamsData?.streams ?? []

  const userTriggers = triggers.filter((t) => !t.internal)
  const userFunctions = functions.filter((f) => !f.internal)

  return (
    <div className="p-4 md:p-6 space-y-4 md:space-y-6 max-w-[1800px] mx-auto">
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3">
        <div>
          <h1 className="font-sans font-semibold text-lg tracking-tight">Dashboard</h1>
          <p className="font-sans text-sm text-secondary mt-1">System overview</p>
        </div>
      </div>

      <div className="grid gap-3 md:gap-4 grid-cols-2 xl:grid-cols-4">
        <MetricsChart
          title="Functions"
          value={loading ? '—' : userFunctions.length}
          color="var(--success)"
          icon={Activity}
          href="/functions"
        />
        <MetricsChart
          title="Triggers"
          value={loading ? '—' : userTriggers.length}
          color="var(--accent)"
          icon={Zap}
          href="/triggers"
        />
        <MetricsChart
          title="Workers"
          value={loading ? '—' : (status?.workers ?? 0)}
          color="#06B6D4"
          icon={Users}
        />
        <MetricsChart
          title="Streams"
          value={loading ? '—' : streams.filter((s) => !s.internal).length}
          color="var(--info)"
          icon={Wifi}
          href="/streams"
        />
      </div>

      <Card>
        <CardHeader className="flex flex-col sm:flex-row sm:items-center justify-between gap-2 pb-0">
          <CardTitle className="text-sm md:text-base">Application Flow</CardTitle>
          {config.enableFlow && (
            <Link
              to="/flow"
              className="text-xs tracking-wider uppercase text-muted hover:text-yellow transition-colors flex items-center gap-1 group cursor-pointer"
            >
              Detailed View{' '}
              <ArrowRight className="w-3 h-3 transition-transform group-hover:translate-x-0.5" />
            </Link>
          )}
        </CardHeader>
        <CardContent className="p-3 md:p-4">
          {loading ? (
            <div className="space-y-3 py-4">
              <Skeleton className="h-4 w-48 mx-auto" />
              <div className="flex gap-3">
                <Skeleton className="h-24 flex-1" />
                <Skeleton className="h-24 flex-1" />
                <Skeleton className="h-24 flex-1" />
              </div>
            </div>
          ) : userTriggers.length === 0 && userFunctions.length === 0 ? (
            <EmptyState
              icon={Activity}
              title="No application components"
              description="Register functions and triggers to get started"
            />
          ) : (
            <div className="flex flex-col lg:flex-row items-stretch gap-3 lg:gap-0">
              {/* Triggers column */}
              <div className="flex-1 min-w-0">
                <div className="font-sans font-semibold text-xs text-muted uppercase tracking-[0.04em] mb-2 text-center">
                  Triggers
                </div>
                <div className="space-y-1.5">
                  {userTriggers.filter((t) => t.trigger_type === 'http').length > 0 && (
                    <div className="bg-cyan-500/10 border border-cyan-500/30 rounded-lg p-2.5">
                      <div className="flex items-center gap-1.5 mb-1.5">
                        <Globe className="w-3 h-3 text-cyan-400" />
                        <span className="text-xs font-bold text-cyan-400 tracking-wider uppercase">
                          REST API
                        </span>
                        <span className="text-xs text-cyan-400/60 ml-auto">
                          {userTriggers.filter((t) => t.trigger_type === 'http').length}
                        </span>
                      </div>
                      <div className="space-y-1">
                        {userTriggers
                          .filter((t) => t.trigger_type === 'http')
                          .slice(0, 4)
                          .map((t) => (
                            <div
                              key={t.id}
                              className="text-xs font-mono text-foreground/80 flex items-center gap-1 bg-black/30 px-1.5 py-0.5 rounded border border-cyan-500/10 overflow-hidden"
                            >
                              <span className="text-cyan-300/80 flex-shrink-0">
                                {(t.config as { http_method?: string })?.http_method || 'GET'}
                              </span>
                              <span className="truncate text-foreground/60">
                                /
                                {(
                                  (t.config as { api_path?: string })?.api_path ||
                                  t.function_id?.replace(/^api\./, '').replace(/\./g, '/')
                                )?.replace(/^\//, '')}
                              </span>
                            </div>
                          ))}
                        {userTriggers.filter((t) => t.trigger_type === 'http').length > 4 && (
                          <Link
                            to="/triggers"
                            className="block text-xs text-cyan-400/50 pl-1.5 hover:text-cyan-400 transition-colors cursor-pointer"
                          >
                            +{userTriggers.filter((t) => t.trigger_type === 'http').length - 4} more
                          </Link>
                        )}
                      </div>
                    </div>
                  )}
                  {userTriggers.filter((t) => t.trigger_type === 'cron').length > 0 && (
                    <div className="bg-orange-500/10 border border-orange-500/30 rounded-lg p-2.5">
                      <div className="flex items-center gap-1.5">
                        <Calendar className="w-3 h-3 text-orange-400" />
                        <span className="text-xs font-bold text-orange-400 tracking-wider uppercase">
                          Cron
                        </span>
                        <span className="text-xs text-orange-400/60 ml-auto">
                          {userTriggers.filter((t) => t.trigger_type === 'cron').length}
                        </span>
                      </div>
                    </div>
                  )}
                  {userTriggers.filter((t) => t.trigger_type === 'event').length > 0 && (
                    <div className="bg-purple-500/10 border border-purple-500/30 rounded-lg p-2.5">
                      <div className="flex items-center gap-1.5">
                        <MessageSquare className="w-3 h-3 text-purple-400" />
                        <span className="text-xs font-bold text-purple-400 tracking-wider uppercase">
                          Events
                        </span>
                        <span className="text-xs text-purple-400/60 ml-auto">
                          {userTriggers.filter((t) => t.trigger_type === 'event').length}
                        </span>
                      </div>
                    </div>
                  )}
                </div>
              </div>

              {/* Arrow connector */}
              <div className="hidden lg:flex flex-shrink-0 items-center justify-center w-10">
                <div className="flex items-center">
                  <div className="h-[1px] w-3 bg-muted/30" />
                  <ChevronRight className="w-3 h-3 text-muted/40" />
                </div>
              </div>

              {/* Functions column */}
              <div className="flex-1 min-w-0">
                <div className="font-sans font-semibold text-xs text-muted uppercase tracking-[0.04em] mb-2 text-center">
                  Functions
                </div>
                <div className="bg-dark-gray/40 border border-border/50 rounded-lg p-2.5">
                  <div className="flex items-center gap-1.5 mb-1.5 pb-1.5 border-b border-border/30">
                    <Activity className="w-3 h-3 text-foreground/70" />
                    <span className="text-xs font-bold tracking-wide uppercase">
                      {userFunctions.length} registered
                    </span>
                  </div>
                  <div className="space-y-0.5 max-h-[120px] overflow-y-auto custom-scrollbar">
                    {userFunctions.slice(0, 4).map((f) => (
                      <div
                        key={f.function_id}
                        className="text-xs font-mono text-foreground/70 bg-black/20 px-1.5 py-0.5 rounded truncate"
                      >
                        {f.function_id}
                      </div>
                    ))}
                    {userFunctions.length > 4 && (
                      <Link
                        to="/functions"
                        className="block text-xs text-muted/60 pl-1.5 hover:text-foreground transition-colors cursor-pointer"
                      >
                        +{userFunctions.length - 4} more
                      </Link>
                    )}
                  </div>
                </div>
              </div>

              {/* Arrow connector */}
              <div className="hidden lg:flex flex-shrink-0 items-center justify-center w-10">
                <div className="flex items-center">
                  <div className="h-[1px] w-3 bg-muted/30" />
                  <ChevronRight className="w-3 h-3 text-muted/40" />
                </div>
              </div>

              {/* States + Streams column */}
              <div className="flex-1 min-w-0">
                <div className="font-sans font-semibold text-xs text-muted uppercase tracking-[0.04em] mb-2 text-center">
                  Data
                </div>
                <div className="space-y-1.5">
                  <div className="bg-blue-500/10 border border-blue-500/30 rounded-lg p-2.5">
                    <div className="flex items-center gap-1.5 mb-1">
                      <Database className="w-3 h-3 text-blue-400" />
                      <span className="text-xs font-bold text-blue-400 tracking-wider uppercase">
                        KV Store
                      </span>
                      <span className="text-xs text-blue-400/60 ml-auto">
                        {streams.filter((s) => !s.internal).length}
                      </span>
                    </div>
                    {streams.filter((s) => !s.internal).length > 0 && (
                      <div className="space-y-0.5">
                        {streams
                          .filter((s) => !s.internal)
                          .slice(0, 2)
                          .map((s) => (
                            <div
                              key={s.id}
                              className="text-xs font-mono text-foreground/60 bg-black/30 px-1.5 py-0.5 rounded truncate"
                            >
                              {s.id}
                            </div>
                          ))}
                        {streams.filter((s) => !s.internal).length > 2 && (
                          <Link
                            to="/streams"
                            className="block text-xs text-blue-400/50 pl-1.5 hover:text-blue-400 transition-colors cursor-pointer"
                          >
                            +{streams.filter((s) => !s.internal).length - 2} more
                          </Link>
                        )}
                      </div>
                    )}
                  </div>
                  <div className="bg-green-500/10 border border-green-500/30 rounded-lg p-2.5">
                    <div className="flex items-center gap-1.5">
                      <Wifi className="w-3 h-3 text-green-400" />
                      <span className="text-xs font-bold text-green-400 tracking-wider uppercase">
                        WebSocket
                      </span>
                      <div className="flex items-center gap-1 ml-auto">
                        <span className="w-1.5 h-1.5 rounded-full bg-green-400 animate-pulse" />
                        <span className="text-xs text-green-400/60">:{config.wsPort}</span>
                      </div>
                    </div>
                  </div>
                </div>
              </div>
            </div>
          )}
        </CardContent>
      </Card>

      <div className="grid gap-3 md:gap-4 grid-cols-1 lg:grid-cols-3 items-start">
        <Card className="lg:col-span-2">
          <CardHeader className="flex flex-col sm:flex-row sm:items-center justify-between gap-2 p-3 md:p-4">
            <CardTitle className="text-sm md:text-base">Registered Triggers</CardTitle>
            <Link
              to="/triggers"
              className="text-xs tracking-wider uppercase text-muted hover:text-accent transition-colors flex items-center gap-1 group cursor-pointer"
            >
              View All{' '}
              <ArrowRight className="w-3 h-3 transition-transform group-hover:translate-x-0.5" />
            </Link>
          </CardHeader>
          <CardContent className="p-3 md:p-4 pt-0 md:pt-0">
            {loading ? (
              <div className="space-y-2 py-4">
                <Skeleton className="h-8 w-full" />
                <Skeleton className="h-8 w-full" />
                <Skeleton className="h-8 w-full" />
                <Skeleton className="h-8 w-full" />
              </div>
            ) : userTriggers.length === 0 ? (
              <div className="text-xs text-muted py-4 text-center border border-dashed border-border-subtle rounded">
                No user triggers registered
                {triggers.length > 0 && (
                  <div className="text-xs text-muted mt-1">
                    ({triggers.length} system triggers hidden)
                  </div>
                )}
              </div>
            ) : (
              <div className="overflow-x-auto">
                <table className="w-full text-sm">
                  <thead>
                    <tr className="border-b border-border-subtle">
                      <th className="text-left py-2 px-3 font-sans font-semibold text-xs uppercase tracking-[0.04em] text-muted">
                        Type
                      </th>
                      <th className="text-left py-2 px-3 font-sans font-semibold text-xs uppercase tracking-[0.04em] text-muted">
                        Function
                      </th>
                      <th className="text-left py-2 px-3 font-sans font-semibold text-xs uppercase tracking-[0.04em] text-muted hidden sm:table-cell">
                        Detail
                      </th>
                      <th className="text-left py-2 px-3 font-sans font-semibold text-xs uppercase tracking-[0.04em] text-muted">
                        Status
                      </th>
                    </tr>
                  </thead>
                  <tbody>
                    {userTriggers.slice(0, 5).map((trigger) => (
                      <tr
                        key={trigger.id}
                        className="border-b border-border-subtle/60 transition-colors hover:bg-white/[0.02]"
                      >
                        <td className="py-2 px-3">
                          <Badge variant="outline" className="text-xs">
                            {trigger.trigger_type}
                          </Badge>
                        </td>
                        <td className="py-2 px-3 font-mono text-[13px] max-w-[180px] truncate">
                          {trigger.function_id || '—'}
                        </td>
                        <td className="py-2 px-3 font-mono text-[13px] text-muted hidden sm:table-cell max-w-[200px] truncate">
                          {trigger.trigger_type === 'http'
                            ? `${(trigger.config as { http_method?: string })?.http_method || 'GET'} /${((trigger.config as { api_path?: string })?.api_path || '').replace(/^\//, '')}`
                            : trigger.trigger_type === 'cron'
                              ? (trigger.config as { schedule?: string })?.schedule || '—'
                              : trigger.trigger_type === 'event'
                                ? (trigger.config as { event_type?: string })?.event_type ||
                                  'listener'
                                : '—'}
                        </td>
                        <td className="py-2 px-3">
                          <Badge variant="success" className="text-xs">
                            Active
                          </Badge>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
                {userTriggers.length > 5 && (
                  <Link
                    to="/triggers"
                    className="block text-xs text-muted text-center py-2 hover:text-yellow transition-colors cursor-pointer"
                  >
                    +{userTriggers.length - 5} more triggers
                  </Link>
                )}
              </div>
            )}
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="p-3 md:p-4 pb-2">
            <CardTitle className="text-sm md:text-base">System</CardTitle>
          </CardHeader>
          <CardContent className="p-3 md:p-4 pt-0 space-y-2">
            {/* System info row */}
            <div className="grid grid-cols-3 gap-2">
              <div className="bg-elevated rounded-[var(--radius-lg)] border border-border-subtle p-2.5 text-center">
                <div className="font-sans font-semibold text-xs text-muted uppercase tracking-[0.04em] mb-1">
                  Uptime
                </div>
                <div className="font-mono text-[13px] font-medium truncate">
                  {status?.uptime_formatted || '—'}
                </div>
              </div>
              <div className="bg-elevated rounded-[var(--radius-lg)] border border-border-subtle p-2.5 text-center">
                <div className="font-sans font-semibold text-xs text-muted uppercase tracking-[0.04em] mb-1">
                  API
                </div>
                <div className="font-mono text-[13px] font-medium">:{config.enginePort}</div>
              </div>
              <div className="bg-elevated rounded-[var(--radius-lg)] border border-border-subtle p-2.5 text-center">
                <div className="font-sans font-semibold text-xs text-muted uppercase tracking-[0.04em] mb-1">
                  WS
                </div>
                <div className="font-mono text-[13px] font-medium">:{config.wsPort}</div>
              </div>
            </div>
          </CardContent>
        </Card>
      </div>
    </div>
  )
}

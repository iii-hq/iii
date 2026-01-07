'use client';

import { useEffect, useState, useCallback, useMemo } from 'react';
import { Card, CardContent, CardHeader, CardTitle, StatCard, Badge, Table, TableHeader, TableBody, TableRow, TableHead, TableCell } from "@/components/ui/card";
import { Activity, Zap, Server, Clock, ArrowRight, Users, Wifi, WifiOff, Globe, Calendar, MessageSquare, Database, Radio, ChevronRight, TrendingUp, BarChart3 } from "lucide-react";
import { fetchStatus, fetchTriggers, fetchFunctions, fetchStreams, fetchMetricsHistory, getConnectionStatus, subscribeToMetricsStream, SystemStatus, TriggerInfo, FunctionInfo, StreamInfo, MetricsSnapshot } from "@/lib/api";
import Link from 'next/link';

interface MiniChartProps {
  data: number[];
  color: string;
  height?: number;
}

function MiniChart({ data, color, height = 40 }: MiniChartProps) {
  if (data.length < 2) {
    return (
      <div className="flex items-center justify-center h-10 text-[10px] text-muted">
        Collecting data...
      </div>
    );
  }

  const max = Math.max(...data);
  const min = Math.min(...data);
  const range = max - min || 1;

  const points = data.map((value, i) => {
    const x = (i / (data.length - 1)) * 100;
    const y = ((max - value) / range) * height;
    return `${x},${y}`;
  }).join(' ');

  const areaPoints = `0,${height} ${points} 100,${height}`;

  return (
    <svg viewBox={`0 0 100 ${height}`} className="w-full h-full" preserveAspectRatio="none">
      <defs>
        <linearGradient id={`gradient-${color}`} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor={color} stopOpacity="0.3" />
          <stop offset="100%" stopColor={color} stopOpacity="0" />
        </linearGradient>
      </defs>
      <polygon
        points={areaPoints}
        fill={`url(#gradient-${color})`}
      />
      <polyline
        points={points}
        fill="none"
        stroke={color}
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
        vectorEffect="non-scaling-stroke"
      />
    </svg>
  );
}

interface MetricsChartProps {
  title: string;
  value: number | string;
  data: number[];
  color: string;
  icon: React.ElementType;
  trend?: number;
}

function MetricsChart({ title, value, data, color, icon: Icon, trend }: MetricsChartProps) {
  return (
    <div className="bg-dark-gray/40 rounded-xl border border-border p-4 hover:border-muted/40 transition-colors">
      <div className="flex items-center justify-between mb-3">
        <div className="flex items-center gap-2">
          <div className="p-1.5 rounded-md" style={{ backgroundColor: `${color}20` }}>
            <Icon className="w-4 h-4" style={{ color }} />
          </div>
          <span className="text-xs font-medium text-muted uppercase tracking-wider">{title}</span>
        </div>
        {trend !== undefined && trend !== 0 && (
          <div className={`flex items-center gap-1 text-[10px] font-medium ${trend > 0 ? 'text-green-400' : 'text-red-400'}`}>
            <TrendingUp className={`w-3 h-3 ${trend < 0 ? 'rotate-180' : ''}`} />
            {Math.abs(trend)}%
          </div>
        )}
      </div>
      <div className="text-2xl font-bold mb-3">{value}</div>
      <div className="h-10">
        <MiniChart data={data} color={color} />
      </div>
    </div>
  );
}

export default function Dashboard() {
  const [status, setStatus] = useState<SystemStatus | null>(null);
  const [triggers, setTriggers] = useState<TriggerInfo[]>([]);
  const [functions, setFunctions] = useState<FunctionInfo[]>([]);
  const [streams, setStreams] = useState<StreamInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [apiSource, setApiSource] = useState<'devtools' | 'management' | null>(null);
  const [streamConnected, setStreamConnected] = useState(false);
  const [lastUpdate, setLastUpdate] = useState<Date | null>(null);
  const [metricsHistory, setMetricsHistory] = useState<MetricsSnapshot[]>([]);

  const handleMetricsUpdate = useCallback((metrics: MetricsSnapshot) => {
    setLastUpdate(new Date());
    
    setMetricsHistory(prev => {
      const updated = [...prev, metrics];
      return updated;
    });
    
    setStatus(prev => prev ? {
      ...prev,
      functions: metrics.functions_count,
      triggers: metrics.triggers_count,
      workers: metrics.workers_count,
      uptime_seconds: metrics.uptime_seconds,
      uptime_formatted: formatUptime(metrics.uptime_seconds),
      timestamp: metrics.timestamp,
    } : prev);
  }, []);

  const formatUptime = (seconds: number): string => {
    if (seconds < 60) return `${seconds}s`;
    if (seconds < 3600) return `${Math.floor(seconds / 60)}m ${seconds % 60}s`;
    const hours = Math.floor(seconds / 3600);
    const mins = Math.floor((seconds % 3600) / 60);
    return `${hours}h ${mins}m`;
  };

  useEffect(() => {
    async function loadData() {
      try {
        const connectionStatus = await getConnectionStatus();
        setApiSource(connectionStatus.devtools ? 'devtools' : connectionStatus.management ? 'management' : null);

        const [statusData, triggersData, functionsData, streamsData, historyData] = await Promise.all([
          fetchStatus().catch(() => null),
          fetchTriggers().catch(() => ({ triggers: [], count: 0 })),
          fetchFunctions().catch(() => ({ functions: [], count: 0 })),
          fetchStreams().catch(() => ({ streams: [], count: 0 })),
          fetchMetricsHistory().catch(() => ({ history: [], count: 0 }))
        ]);

        setStatus(statusData);
        setTriggers(triggersData?.triggers || []);
        setFunctions(functionsData?.functions || []);
        setStreams(streamsData?.streams || []);
        if (historyData?.history) {
          setMetricsHistory(historyData.history);
        }
      } catch {
      } finally {
        setLoading(false);
      }
    }

    loadData();
  }, []);

  useEffect(() => {
    const unsubscribe = subscribeToMetricsStream(
      handleMetricsUpdate,
      (allMetrics) => {
        if (allMetrics.length > 0) {
          setMetricsHistory(allMetrics);
          const latest = allMetrics.sort((a, b) => b.timestamp - a.timestamp)[0];
          if (latest) {
            setStatus(prev => prev ? {
              ...prev,
              functions: latest.functions_count,
              triggers: latest.triggers_count,
              workers: latest.workers_count,
              uptime_seconds: latest.uptime_seconds,
              uptime_formatted: formatUptime(latest.uptime_seconds),
              timestamp: latest.timestamp,
            } : prev);
            setLastUpdate(new Date());
          }
        }
      },
      () => {},
      () => {
        setStreamConnected(true);
      },
      () => {
        setStreamConnected(false);
      }
    );

    return () => {
      unsubscribe();
    };
  }, [handleMetricsUpdate]);

  const isOnline = status !== null;

  const userTriggers = triggers.filter(t => !t.internal);
  const userFunctions = functions.filter(f => !f.internal);

  const functionsData = useMemo(() => 
    metricsHistory.map(m => m.functions_count), 
    [metricsHistory]
  );
  
  const triggersData = useMemo(() => 
    metricsHistory.map(m => m.triggers_count), 
    [metricsHistory]
  );
  
  const workersData = useMemo(() => 
    metricsHistory.map(m => m.workers_count), 
    [metricsHistory]
  );
  
  const uptimeData = useMemo(() => 
    metricsHistory.map(m => m.uptime_seconds), 
    [metricsHistory]
  );

  const calculateTrend = (data: number[]): number => {
    if (data.length < 2) return 0;
    const recent = data.slice(-5);
    const older = data.slice(0, 5);
    if (older.length === 0 || recent.length === 0) return 0;
    const recentAvg = recent.reduce((a, b) => a + b, 0) / recent.length;
    const olderAvg = older.reduce((a, b) => a + b, 0) / older.length;
    if (olderAvg === 0) return 0;
    return Math.round(((recentAvg - olderAvg) / olderAvg) * 100);
  };

  return (
    <div className="p-6 space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-xl font-semibold tracking-tight">Dashboard</h1>
          <p className="text-xs text-muted mt-1 tracking-wide">
            System overview and metrics
            {apiSource && (
              <span className="ml-2 text-[#F3F724]">• {apiSource === 'devtools' ? 'DevTools API' : 'Management API'}</span>
            )}
            {streamConnected && (
              <span className="ml-2 text-[#22C55E]">• Real-time</span>
            )}
          </p>
        </div>
        <div className="flex items-center gap-2">
          <div className={`
            flex items-center gap-2 px-3 py-1.5 rounded-full border
            ${streamConnected 
              ? 'border-[#F3F724]/40' 
              : 'border-border'
            }
          `}>
            {streamConnected ? (
              <Wifi className="w-3 h-3 text-[#F3F724]" />
            ) : (
              <WifiOff className="w-3 h-3 text-muted" />
            )}
            <span className="text-[10px] tracking-[0.1em] uppercase text-muted">
              {streamConnected ? 'Live' : 'Connecting'}
            </span>
          </div>
          <div className={`
            flex items-center gap-2 px-4 py-2 rounded-full border
            ${isOnline 
              ? 'border-[#22C55E]/40' 
              : 'border-[#EF4444]/40'
            }
          `}>
            <div className={`
              w-2 h-2 rounded-full
              ${isOnline 
                ? 'bg-[#22C55E] shadow-[0_0_8px_#22C55E]' 
                : 'bg-[#EF4444] shadow-[0_0_8px_#EF4444]'
              }
            `} />
            <span className="text-[11px] tracking-[0.1em] uppercase text-foreground">
              {isOnline ? 'Online' : 'Offline'}
            </span>
          </div>
        </div>
      </div>

      <div className="grid gap-4 grid-cols-2 lg:grid-cols-4">
        <MetricsChart
          title="Functions"
          value={loading ? '—' : userFunctions.length}
          data={functionsData}
          color="#22C55E"
          icon={Activity}
          trend={calculateTrend(functionsData)}
        />
        <MetricsChart
          title="Triggers"
          value={loading ? '—' : userTriggers.length}
          data={triggersData}
          color="#F3F724"
          icon={Zap}
          trend={calculateTrend(triggersData)}
        />
        <MetricsChart
          title="Workers"
          value={loading ? '—' : (status?.workers ?? 0)}
          data={workersData}
          color="#06B6D4"
          icon={Users}
          trend={calculateTrend(workersData)}
        />
        <MetricsChart
          title="Uptime"
          value={loading ? '—' : (status?.uptime_formatted ?? '—')}
          data={uptimeData}
          color="#A855F7"
          icon={Clock}
        />
      </div>

      <div className="grid gap-4 grid-cols-2">
        <StatCard 
          title="Version"
          value={loading ? '—' : (status?.version ?? '—')}
          subtitle="Engine"
          icon={Server}
        />
        <div className="bg-dark-gray/40 rounded-xl border border-border p-4 flex items-center gap-4">
          <div className="p-2 rounded-lg bg-primary/10">
            <BarChart3 className="w-5 h-5 text-primary" />
          </div>
          <div className="flex-1">
            <div className="text-xs text-muted uppercase tracking-wider mb-1">Metrics History</div>
            <div className="flex items-baseline gap-2">
              <div className="text-lg font-bold">{metricsHistory.length}</div>
              <span className="text-xs text-muted">data points</span>
            </div>
          </div>
          {streamConnected && (
            <div className="flex items-center gap-1.5 text-xs text-green-400">
              <span className="w-1.5 h-1.5 rounded-full bg-green-400 animate-pulse" />
              Live
            </div>
          )}
        </div>
      </div>

      <Card>
        <CardHeader className="flex flex-row items-center justify-between">
          <CardTitle>Application Flow</CardTitle>
          <div className="text-[10px] text-muted">How your triggers, functions, and streams connect</div>
        </CardHeader>
        <CardContent>
          {loading ? (
            <div className="text-xs text-muted py-8 text-center">Loading...</div>
          ) : userTriggers.length === 0 && userFunctions.length === 0 ? (
            <div className="text-xs text-muted py-8 text-center border border-dashed border-border rounded">
              <div className="mb-2">No application components registered</div>
              <div className="text-[10px] text-muted/60">Register functions and triggers using the SDK to see your application flow</div>
            </div>
          ) : (
            <div className="relative overflow-hidden">
              <div className="flex items-stretch justify-between gap-2 py-4">
                <div className="flex-1 min-w-0">
                  <div className="text-[11px] font-bold text-muted uppercase tracking-[0.2em] mb-4 text-center">Entry Points</div>
                  <div className="space-y-3">
                    {userTriggers.filter(t => t.trigger_type === 'api').length > 0 && (
                      <div className="bg-cyan-500/15 border border-cyan-500/40 rounded-xl p-4 shadow-[0_0_15px_rgba(6,182,212,0.05)]">
                        <div className="flex items-center gap-2 mb-3">
                          <Globe className="w-4 h-4 text-cyan-400" />
                          <span className="text-xs font-bold text-cyan-400 tracking-wider uppercase">REST API</span>
                        </div>
                        <div className="space-y-2">
                          {userTriggers.filter(t => t.trigger_type === 'api').slice(0, 4).map(t => (
                            <div key={t.id} className="text-[11px] font-mono text-foreground/90 flex items-center gap-2 bg-black/40 px-2 py-1.5 rounded border border-cyan-500/10 overflow-hidden">
                              <span className="text-cyan-400/80 flex-shrink-0">→</span>
                              <span className="text-cyan-300/90 flex-shrink-0">{(t.config as { http_method?: string })?.http_method || 'GET'}</span>
                              <span className="truncate">/{(t.config as { api_path?: string })?.api_path || t.function_path?.replace(/^api\./, '').replace(/\./g, '/')}</span>
                            </div>
                          ))}
                          {userTriggers.filter(t => t.trigger_type === 'api').length > 4 && (
                            <div className="text-[10px] text-cyan-400/60 font-medium pl-2 italic">
                              +{userTriggers.filter(t => t.trigger_type === 'api').length - 4} more
                            </div>
                          )}
                        </div>
                      </div>
                    )}
                    
                    {userTriggers.filter(t => t.trigger_type === 'cron').length > 0 && (
                      <div className="bg-yellow/15 border border-yellow/40 rounded-xl p-4 shadow-[0_0_15px_rgba(243,247,36,0.05)]">
                        <div className="flex items-center gap-2 mb-3">
                          <Calendar className="w-4 h-4 text-yellow" />
                          <span className="text-xs font-bold text-yellow tracking-wider uppercase">Scheduled Jobs</span>
                        </div>
                        <div className="space-y-2">
                          {userTriggers.filter(t => t.trigger_type === 'cron').slice(0, 3).map(t => (
                            <div key={t.id} className="text-[11px] font-mono text-foreground/90 bg-black/40 px-2 py-1.5 rounded border border-yellow/10 truncate">
                              <span className="text-yellow/80 pr-1">⏱</span>
                              {(t.config as { schedule?: string })?.schedule || 'scheduled'}
                            </div>
                          ))}
                        </div>
                      </div>
                    )}
                    
                    {userTriggers.filter(t => t.trigger_type === 'event').length > 0 && (
                      <div className="bg-purple-500/15 border border-purple-500/40 rounded-xl p-4 shadow-[0_0_15px_rgba(168,85,247,0.05)]">
                        <div className="flex items-center gap-2 mb-2">
                          <MessageSquare className="w-4 h-4 text-purple-400" />
                          <span className="text-xs font-bold text-purple-400 tracking-wider uppercase">Events</span>
                        </div>
                        <div className="text-[11px] text-foreground/80 pl-1 font-medium italic">
                          {userTriggers.filter(t => t.trigger_type === 'event').length} active listeners
                        </div>
                      </div>
                    )}
                  </div>
                </div>
                
                <div className="flex-shrink-0 flex items-center justify-center w-16">
                  <div className="flex flex-col items-center gap-3 opacity-60 group hover:opacity-100 transition-all duration-300">
                    <div className="text-[8px] text-muted font-bold uppercase tracking-[0.2em] bg-dark-gray px-1.5 py-0.5 rounded border border-border/60 shadow-sm group-hover:text-foreground transition-colors">
                      invoke
                    </div>
                    <div className="flex items-center justify-center w-full">
                      <div className="h-[1px] w-4 bg-gradient-to-r from-transparent to-muted/40" />
                      <div className="w-6 h-6 rounded-full border border-border/80 bg-black flex items-center justify-center shadow-lg group-hover:border-muted/60 transition-colors">
                        <ChevronRight className="w-3 h-3 text-muted group-hover:text-foreground" />
                      </div>
                      <div className="h-[1px] w-4 bg-gradient-to-r from-muted/40 to-transparent" />
                    </div>
                  </div>
                </div>
                
                <div className="flex-1 min-w-0">
                  <div className="text-[11px] font-bold text-muted uppercase tracking-[0.2em] mb-4 text-center">Functions</div>
                  <div className="bg-dark-gray/40 border border-border/60 rounded-xl p-4 shadow-sm h-full max-h-[350px] flex flex-col">
                    <div className="flex items-center gap-2 mb-4 border-b border-border/30 pb-3">
                      <Activity className="w-4 h-4 text-foreground" />
                      <span className="text-xs font-bold tracking-wide uppercase">{userFunctions.length} Active Functions</span>
                    </div>
                    <div className="space-y-2 overflow-y-auto flex-1 custom-scrollbar pr-1">
                      {userFunctions.map(f => (
                        <div key={f.path} className="text-[11px] font-mono text-foreground/90 bg-black/30 px-3 py-2 rounded border border-border/20 truncate hover:border-muted/40 transition-all hover:translate-x-0.5">
                          {f.path}
                        </div>
                      ))}
                    </div>
                  </div>
                </div>
                
                <div className="flex-shrink-0 flex items-center justify-center w-16">
                  <div className="flex flex-col items-center gap-3 opacity-60 group hover:opacity-100 transition-all duration-300">
                    <div className="text-[8px] text-muted font-bold uppercase tracking-[0.2em] bg-dark-gray px-1.5 py-0.5 rounded border border-border/60 shadow-sm group-hover:text-foreground transition-colors">
                      r/w
                    </div>
                    <div className="flex items-center justify-center w-full">
                      <div className="h-[1px] w-4 bg-gradient-to-r from-transparent to-muted/40" />
                      <div className="w-6 h-6 rounded-full border border-border/80 bg-black flex items-center justify-center shadow-lg group-hover:border-muted/60 transition-colors">
                        <ChevronRight className="w-3 h-3 text-muted group-hover:text-foreground" />
                      </div>
                      <div className="h-[1px] w-4 bg-gradient-to-r from-muted/40 to-transparent" />
                    </div>
                  </div>
                </div>
                
                <div className="flex-1 min-w-0">
                  <div className="text-[11px] font-bold text-muted uppercase tracking-[0.2em] mb-4 text-center">State & Streams</div>
                  <div className="space-y-3">
                    {streams.filter(s => !s.internal).length > 0 ? (
                      <div className="bg-green-500/15 border border-green-500/40 rounded-xl p-4 shadow-[0_0_15px_rgba(34,197,94,0.05)]">
                        <div className="flex items-center gap-2 mb-3">
                          <Database className="w-4 h-4 text-green-400" />
                          <span className="text-xs font-bold text-green-400 tracking-wider uppercase">Streams</span>
                        </div>
                        <div className="space-y-2">
                          {streams.filter(s => !s.internal).slice(0, 5).map(s => (
                            <div key={s.id} className="text-[11px] font-mono text-foreground/90 bg-black/40 px-2 py-1.5 rounded border border-green-500/10 truncate">
                              <span className="text-green-400/60 pr-1 font-bold">#</span>
                              {s.id}
                            </div>
                          ))}
                        </div>
                      </div>
                    ) : (
                      <div className="bg-dark-gray/30 border border-dashed border-border/60 rounded-xl p-4 text-center">
                        <Database className="w-5 h-5 text-muted/40 mx-auto mb-2" />
                        <div className="text-[10px] font-bold text-muted/60 uppercase tracking-widest">No Streams</div>
                      </div>
                    )}
                    
                    <div className="bg-dark-gray/40 border border-border/60 rounded-xl p-4 hover:border-muted/40 transition-colors overflow-hidden">
                      <div className="flex items-center justify-between gap-3">
                        <div className="flex items-center gap-3">
                          <div className="bg-muted/10 p-1.5 rounded flex-shrink-0">
                            <Radio className="w-4 h-4 text-muted" />
                          </div>
                          <span className="text-xs font-bold text-muted tracking-wide uppercase truncate">Events</span>
                        </div>
                        <div className="w-1.5 h-1.5 rounded-full bg-muted/30 flex-shrink-0" />
                      </div>
                    </div>
                  </div>
                </div>
              </div>
              
              <div className="flex items-center gap-8 mt-6 pt-6 border-t border-border/40 justify-center">
                <div className="flex items-center gap-2 group cursor-default">
                  <div className="w-3 h-3 rounded-sm bg-cyan-500/40 border border-cyan-500/60 group-hover:bg-cyan-500/60 transition-colors" />
                  <span className="text-[10px] font-bold text-muted group-hover:text-foreground transition-colors uppercase tracking-[0.15em]">HTTP Endpoints</span>
                </div>
                <div className="flex items-center gap-2 group cursor-default">
                  <div className="w-3 h-3 rounded-sm bg-yellow/40 border border-yellow/60 group-hover:bg-yellow/60 transition-colors" />
                  <span className="text-[10px] font-bold text-muted group-hover:text-foreground transition-colors uppercase tracking-[0.15em]">Cron Jobs</span>
                </div>
                <div className="flex items-center gap-2 group cursor-default">
                  <div className="w-3 h-3 rounded-sm bg-purple-500/40 border border-purple-500/60 group-hover:bg-purple-500/60 transition-colors" />
                  <span className="text-[10px] font-bold text-muted group-hover:text-foreground transition-colors uppercase tracking-[0.15em]">Event Listeners</span>
                </div>
                <div className="flex items-center gap-2 group cursor-default">
                  <div className="w-3 h-3 rounded-sm bg-green-500/40 border border-green-500/60 group-hover:bg-green-500/60 transition-colors" />
                  <span className="text-[10px] font-bold text-muted group-hover:text-foreground transition-colors uppercase tracking-[0.15em]">Data Streams</span>
                </div>
              </div>
            </div>
          )}
        </CardContent>
      </Card>

      <div className="grid gap-6 lg:grid-cols-3">
        <Card className="lg:col-span-2">
          <CardHeader className="flex flex-row items-center justify-between">
            <CardTitle>Registered Triggers</CardTitle>
            <Link href="/triggers" className="text-[10px] tracking-wider uppercase text-[#5B5B5B] hover:text-[#F3F724] transition-colors flex items-center gap-1 group">
              View All <ArrowRight className="w-3 h-3 transition-transform group-hover:translate-x-0.5" />
            </Link>
          </CardHeader>
          <CardContent>
            {loading ? (
              <div className="text-xs text-[#5B5B5B] py-8 text-center">Loading...</div>
            ) : userTriggers.length === 0 ? (
              <div className="text-xs text-[#5B5B5B] py-8 text-center border border-dashed border-[#1D1D1D] rounded">
                No user triggers registered
                {triggers.length > 0 && (
                  <div className="text-[9px] text-muted mt-1">
                    ({triggers.length} system triggers hidden)
                  </div>
                )}
              </div>
            ) : (
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>Type</TableHead>
                    <TableHead>ID</TableHead>
                    <TableHead>Function</TableHead>
                    <TableHead>Status</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {userTriggers.slice(0, 5).map((trigger) => (
                    <TableRow key={trigger.id}>
                      <TableCell>
                        <Badge variant="outline">{trigger.trigger_type}</Badge>
                      </TableCell>
                      <TableCell className="font-mono text-[#5B5B5B]">
                        {trigger.id.length > 20 ? `${trigger.id.slice(0, 20)}...` : trigger.id}
                      </TableCell>
                      <TableCell className="font-mono">{trigger.function_path || '—'}</TableCell>
                      <TableCell>
                        <Badge variant="success">Active</Badge>
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            )}
          </CardContent>
        </Card>

        <div className="space-y-4">
          <Card>
            <CardHeader>
              <CardTitle>Quick Actions</CardTitle>
            </CardHeader>
            <CardContent className="space-y-2">
              <Link href="/streams" className="block group">
                <div className="p-3 rounded border border-[#1D1D1D] group-hover:border-[#F3F724]/40 transition-colors cursor-pointer">
                  <div className="text-xs font-medium group-hover:text-[#F3F724] transition-colors">View Streams</div>
                  <div className="text-[10px] text-[#5B5B5B] mt-0.5">Manage queues and messages</div>
                </div>
              </Link>
              <Link href="/logs" className="block group">
                <div className="p-3 rounded border border-[#1D1D1D] group-hover:border-[#F3F724]/40 transition-colors cursor-pointer">
                  <div className="text-xs font-medium group-hover:text-[#F3F724] transition-colors">View Logs</div>
                  <div className="text-[10px] text-[#5B5B5B] mt-0.5">Debug system events</div>
                </div>
              </Link>
              <Link href="/config" className="block group">
                <div className="p-3 rounded border border-[#1D1D1D] group-hover:border-[#F3F724]/40 transition-colors cursor-pointer">
                  <div className="text-xs font-medium group-hover:text-[#F3F724] transition-colors">Configuration</div>
                  <div className="text-[10px] text-[#5B5B5B] mt-0.5">View environment settings</div>
                </div>
              </Link>
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle>System Info</CardTitle>
            </CardHeader>
            <CardContent className="space-y-3">
              <div className="flex justify-between items-center">
                <span className="text-[10px] text-muted uppercase tracking-wider">Uptime</span>
                <span className="text-xs font-mono">{status?.uptime_formatted || '—'}</span>
              </div>
              <div className="flex justify-between items-center">
                <span className="text-[10px] text-muted uppercase tracking-wider">REST API</span>
                <span className="text-xs font-mono">:3111</span>
              </div>
              <div className="flex justify-between items-center">
                <span className="text-[10px] text-muted uppercase tracking-wider">Streams</span>
                <span className="text-xs font-mono">:31112</span>
              </div>
              <div className="flex justify-between items-center">
                <span className="text-[10px] text-muted uppercase tracking-wider">Management</span>
                <span className="text-xs font-mono">:3111</span>
              </div>
              {lastUpdate && (
                <div className="flex justify-between items-center pt-2 border-t border-border">
                  <span className="text-[10px] text-muted uppercase tracking-wider">Last Update</span>
                  <span className="text-xs font-mono text-[#F3F724]">
                    {lastUpdate.toLocaleTimeString()}
                  </span>
                </div>
              )}
            </CardContent>
          </Card>
        </div>
      </div>

      {!loading && !isOnline && (
        <Card className="border-[#EF4444]/50 bg-[#EF4444]/5">
          <CardContent className="py-4">
            <div className="flex items-center gap-3">
              <div className="w-2 h-2 rounded-full bg-[#EF4444]" />
              <div>
                <div className="text-xs font-medium text-[#EF4444]">Engine Connection Failed</div>
                <div className="text-[10px] text-[#5B5B5B] mt-0.5">
                  Unable to connect to the iii engine. Make sure the engine is running with DevTools module enabled.
                </div>
              </div>
            </div>
          </CardContent>
        </Card>
      )}
    </div>
  );
}

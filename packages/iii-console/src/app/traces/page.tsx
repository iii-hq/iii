'use client';

import { useState, useEffect, useMemo } from 'react';
import { Card, CardContent, Badge, Button, Input } from "@/components/ui/card";
import { 
  GitBranch, Search, RefreshCw, Clock, CheckCircle2, XCircle, Zap,
  Eye, EyeOff, AlertCircle, ExternalLink, ChevronDown, ChevronRight,
  X, Activity, Wifi, WifiOff, Timer, Database, Radio, FileText, Hash
} from "lucide-react";
import { JsonViewer } from "@/components/ui/json-viewer";

interface TraceEvent {
  id: string;
  type: 'function' | 'stream' | 'state' | 'emit' | 'log';
  name: string;
  timestamp: number;
  duration?: number;
  status: 'ok' | 'error' | 'running';
  data?: Record<string, unknown>;
}

interface TraceGroup {
  traceId: string;
  rootOperation: string;
  status: 'ok' | 'error' | 'running';
  startTime: number;
  endTime?: number;
  duration?: number;
  spanCount: number;
  events: TraceEvent[];
  services: string[];
}

function formatTime(timestamp: number): string {
  return new Date(timestamp).toLocaleTimeString('en-US', { 
    hour12: false, 
    hour: '2-digit', 
    minute: '2-digit', 
    second: '2-digit'
  });
}

function formatDuration(ms: number | undefined): string {
  if (!ms) return '-';
  if (ms < 1) return '<1ms';
  if (ms < 1000) return `${Math.round(ms)}ms`;
  return `${(ms / 1000).toFixed(2)}s`;
}

const EVENT_ICONS: Record<string, typeof Zap> = {
  function: Zap,
  stream: Database,
  state: FileText,
  emit: Radio,
  log: FileText,
};

const EVENT_COLORS: Record<string, string> = {
  function: 'text-yellow',
  stream: 'text-purple-400',
  state: 'text-cyan-400',
  emit: 'text-green-400',
  log: 'text-muted',
};

export default function TracesPage() {
  const [searchQuery, setSearchQuery] = useState('');
  const [isLoading, setIsLoading] = useState(false);
  const [showSystem, setShowSystem] = useState(false);
  const [traceGroups, setTraceGroups] = useState<TraceGroup[]>([]);
  const [selectedTraceId, setSelectedTraceId] = useState<string | null>(null);
  const [expandedEvents, setExpandedEvents] = useState<Set<string>>(new Set());
  const [isConnected] = useState(true);
  const [hasOtelConfigured, setHasOtelConfigured] = useState(false);

  const loadTraces = async () => {
    setIsLoading(true);
    try {
      // First check if the engine is available
      const statusResponse = await fetch('http://localhost:3111/_console/status').catch(() => null);
      if (!statusResponse?.ok) {
        setHasOtelConfigured(false);
        setIsLoading(false);
        return;
      }
      
      // Try to load traces - this endpoint may not exist yet
      const response = await fetch('http://localhost:3111/_console/traces').catch(() => null);
      if (response?.ok) {
        const data = await response.json();
        if (data.traces && data.traces.length > 0) {
          setTraceGroups(data.traces);
          setHasOtelConfigured(true);
        } else {
          setHasOtelConfigured(false);
        }
      } else {
        // Traces endpoint not available - that's ok, show empty state
        setHasOtelConfigured(false);
      }
    } catch {
      setHasOtelConfigured(false);
    } finally {
      setIsLoading(false);
    }
  };

  useEffect(() => {
    loadTraces();
    // Poll less frequently to reduce network noise
    const interval = setInterval(loadTraces, 15000);
    return () => clearInterval(interval);
  }, []);

  const filteredGroups = useMemo(() => {
    return traceGroups.filter(group => {
      const searchLower = searchQuery.toLowerCase();
      return (
        group.traceId.toLowerCase().includes(searchLower) ||
        group.rootOperation.toLowerCase().includes(searchLower)
      );
    });
  }, [traceGroups, searchQuery]);

  const selectedTrace = traceGroups.find(g => g.traceId === selectedTraceId);

  const stats = useMemo(() => ({
    totalTraces: traceGroups.length,
    totalSpans: traceGroups.reduce((sum, g) => sum + g.spanCount, 0),
    errorCount: traceGroups.filter(g => g.status === 'error').length,
    avgDuration: traceGroups.length > 0 
      ? traceGroups.reduce((sum, g) => sum + (g.duration || 0), 0) / traceGroups.length 
      : 0,
  }), [traceGroups]);

  const toggleEventExpand = (eventId: string) => {
    setExpandedEvents(prev => {
      const next = new Set(prev);
      if (next.has(eventId)) next.delete(eventId);
      else next.add(eventId);
      return next;
    });
  };

  return (
    <div className="flex flex-col h-full bg-background text-foreground">
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-2 px-3 md:px-5 py-2 md:py-3 bg-dark-gray/30 border-b border-border">
        <div className="flex items-center gap-2 md:gap-4 flex-wrap">
          <h1 className="text-sm md:text-base font-semibold flex items-center gap-2">
            <GitBranch className="w-4 h-4 text-cyan-400" />
            Traces
          </h1>
          <Badge
            variant={isConnected ? 'success' : 'error'}
            className="gap-1 text-[10px] md:text-xs"
          >
            {isConnected ? <Wifi className="w-2.5 h-2.5 md:w-3 md:h-3" /> : <WifiOff className="w-2.5 h-2.5 md:w-3 md:h-3" />}
            <span className="hidden sm:inline">{isConnected ? 'Live' : 'Offline'}</span>
          </Badge>
        </div>

        <div className="flex items-center gap-1.5 md:gap-2">
          <Button
            variant={showSystem ? "accent" : "ghost"}
            size="sm"
            onClick={() => setShowSystem(!showSystem)}
            className="h-6 md:h-7 text-[10px] md:text-xs px-2"
          >
            {showSystem ? <Eye className="w-3 h-3 md:mr-1.5" /> : <EyeOff className="w-3 h-3 md:mr-1.5" />}
            <span className={`hidden md:inline ${showSystem ? '' : 'line-through opacity-60'}`}>System</span>
          </Button>
          <Button 
            variant="ghost" 
            size="sm" 
            onClick={loadTraces} 
            disabled={isLoading}
            className="h-7 text-xs text-muted hover:text-foreground"
          >
            <RefreshCw className={`w-3.5 h-3.5 mr-1.5 ${isLoading ? 'animate-spin' : ''}`} />
            Refresh
          </Button>
        </div>
      </div>

      <div className="flex items-center gap-2 p-2 border-b border-border bg-dark-gray/20">
        <div className="flex-1 relative">
          <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-muted" />
          <Input
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            className="pl-9 pr-9 h-9 font-medium"
            placeholder="Search by Trace ID or Operation"
          />
          {searchQuery && (
            <button
              onClick={() => setSearchQuery('')}
              className="absolute right-3 top-1/2 -translate-y-1/2 text-muted hover:text-foreground"
            >
              <X className="w-4 h-4" />
            </button>
          )}
        </div>
        
        <div className="flex items-center gap-4 px-2 text-xs text-muted">
          <div className="flex items-center gap-1.5">
            <Hash className="w-3 h-3" />
            <span className="font-medium text-foreground tabular-nums">{stats.totalTraces}</span>
            traces
          </div>
          <div className="flex items-center gap-1.5">
            <Zap className="w-3 h-3" />
            <span className="font-medium text-foreground tabular-nums">{stats.totalSpans}</span>
            spans
          </div>
          {stats.errorCount > 0 && (
            <div className="flex items-center gap-1.5 text-error">
              <XCircle className="w-3 h-3" />
              <span className="font-medium tabular-nums">{stats.errorCount}</span>
              errors
            </div>
          )}
          <div className="flex items-center gap-1.5">
            <Timer className="w-3 h-3" />
            <span className="font-medium text-foreground tabular-nums">{formatDuration(stats.avgDuration)}</span>
            avg
          </div>
        </div>
      </div>

      {!hasOtelConfigured && (
        <div className="flex-1 flex items-center justify-center p-8">
          <div className="text-center max-w-md">
            <div className="w-16 h-16 mb-4 mx-auto rounded-2xl bg-dark-gray border border-border flex items-center justify-center">
              <AlertCircle className="w-8 h-8 text-yellow" />
            </div>
            <h3 className="text-sm font-medium mb-2">OpenTelemetry Not Configured</h3>
            <p className="text-xs text-muted mb-4">
              Configure <code className="bg-dark-gray px-1 rounded font-mono">opentelemetry-rust</code> in the iii engine to enable distributed tracing.
            </p>
            <a 
              href="https://github.com/open-telemetry/opentelemetry-rust" 
              target="_blank" 
              rel="noopener noreferrer"
              className="inline-flex items-center gap-1 text-xs text-yellow hover:underline"
            >
              Learn more about OpenTelemetry <ExternalLink className="w-3 h-3" />
            </a>
          </div>
        </div>
      )}

      {hasOtelConfigured && (
        <div className={`flex-1 grid overflow-hidden ${selectedTraceId ? 'grid-cols-1 md:grid-cols-[260px_1fr] lg:grid-cols-[300px_1fr]' : 'grid-cols-1'}`}>
          <div className="flex flex-col h-full overflow-hidden border-r border-border bg-dark-gray/20">
            <div className="flex-1 overflow-y-auto">
              {isLoading && filteredGroups.length === 0 ? (
                <div className="flex items-center justify-center h-32">
                  <RefreshCw className="w-5 h-5 text-muted animate-spin" />
                </div>
              ) : filteredGroups.length === 0 ? (
                <div className="flex flex-col items-center justify-center h-64 px-4">
                  <div className="w-12 h-12 mb-3 rounded-xl bg-dark-gray border border-border flex items-center justify-center">
                    <GitBranch className="w-6 h-6 text-muted" />
                  </div>
                  <div className="text-sm font-medium mb-1">No traces found</div>
                  <div className="text-xs text-muted text-center">
                    Traces will appear here when OpenTelemetry is configured
                  </div>
                </div>
              ) : (
                filteredGroups.map(group => {
                  const isSelected = selectedTraceId === group.traceId;
                  
                  return (
                    <button
                      key={group.traceId}
                      onClick={() => setSelectedTraceId(isSelected ? null : group.traceId)}
                      className={`w-full p-3 border-b border-border text-left transition-colors
                        ${isSelected 
                          ? 'bg-primary/10 border-l-2 border-l-primary' 
                          : 'hover:bg-dark-gray/50'
                        }
                      `}
                    >
                      <div className="flex items-center gap-2 mb-1">
                        {group.status === 'ok' ? (
                          <CheckCircle2 className="w-3.5 h-3.5 text-success" />
                        ) : group.status === 'error' ? (
                          <XCircle className="w-3.5 h-3.5 text-error" />
                        ) : (
                          <Activity className="w-3.5 h-3.5 text-yellow animate-pulse" />
                        )}
                        <span className="font-medium text-sm truncate flex-1">
                          {group.rootOperation}
                        </span>
                      </div>
                      
                      <div className="flex items-center gap-3 text-[10px] text-muted">
                        <code className="font-mono">{group.traceId.slice(0, 8)}</code>
                        <span className="flex items-center gap-1">
                          <Zap className="w-2.5 h-2.5" />
                          {group.spanCount}
                        </span>
                        <span className="flex items-center gap-1">
                          <Timer className="w-2.5 h-2.5" />
                          {formatDuration(group.duration)}
                        </span>
                        <span className="ml-auto">{formatTime(group.startTime)}</span>
                      </div>
                    </button>
                  );
                })
              )}
            </div>
          </div>

          {selectedTrace && (
            <div className="flex flex-col h-full overflow-hidden bg-background">
              <div className="flex items-center justify-between px-4 py-3 border-b border-border bg-dark-gray/30">
                <div className="flex items-center gap-3 min-w-0">
                  <div className={`p-2 rounded-lg ${selectedTrace.status === 'ok' ? 'bg-success/10' : 'bg-error/10'}`}>
                    {selectedTrace.status === 'ok' ? (
                      <CheckCircle2 className="h-5 w-5 text-success" />
                    ) : (
                      <XCircle className="h-5 w-5 text-error" />
                    )}
                  </div>
                  <div className="min-w-0">
                    <h2 className="text-base font-semibold text-foreground truncate">
                      {selectedTrace.rootOperation}
                    </h2>
                    <code className="text-xs text-muted font-mono">{selectedTrace.traceId}</code>
                  </div>
                </div>

                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => setSelectedTraceId(null)}
                  className="h-7 w-7 p-0"
                >
                  <X className="w-4 h-4" />
                </Button>
              </div>

              <div className="grid grid-cols-4 gap-3 px-4 py-3 border-b border-border bg-dark-gray/20">
                <div className="flex items-center gap-3 p-3 rounded-lg bg-background border border-border">
                  <div className="p-2 rounded-md bg-cyan-500/10">
                    <Timer className="h-4 w-4 text-cyan-400" />
                  </div>
                  <div>
                    <div className="text-xs text-muted">Duration</div>
                    <div className="text-sm font-medium">{formatDuration(selectedTrace.duration)}</div>
                  </div>
                </div>

                <div className="flex items-center gap-3 p-3 rounded-lg bg-background border border-border">
                  <div className="p-2 rounded-md bg-purple-500/10">
                    <Zap className="h-4 w-4 text-purple-400" />
                  </div>
                  <div>
                    <div className="text-xs text-muted">Spans</div>
                    <div className="text-sm font-medium">{selectedTrace.spanCount}</div>
                  </div>
                </div>

                <div className="flex items-center gap-3 p-3 rounded-lg bg-background border border-border">
                  <div className="p-2 rounded-md bg-yellow/10">
                    <Clock className="h-4 w-4 text-yellow" />
                  </div>
                  <div>
                    <div className="text-xs text-muted">Started</div>
                    <div className="text-sm font-medium">{formatTime(selectedTrace.startTime)}</div>
                  </div>
                </div>

                <div className="flex items-center gap-3 p-3 rounded-lg bg-background border border-border">
                  <div className={`p-2 rounded-md ${selectedTrace.status === 'ok' ? 'bg-success/10' : 'bg-error/10'}`}>
                    {selectedTrace.status === 'ok' ? (
                      <CheckCircle2 className="h-4 w-4 text-success" />
                    ) : (
                      <XCircle className="h-4 w-4 text-error" />
                    )}
                  </div>
                  <div>
                    <div className="text-xs text-muted">Status</div>
                    <div className={`text-sm font-medium ${selectedTrace.status === 'ok' ? 'text-success' : 'text-error'}`}>
                      {selectedTrace.status.toUpperCase()}
                    </div>
                  </div>
                </div>
              </div>

              <div className="flex-1 overflow-auto p-4">
                <div className="text-xs font-medium text-muted uppercase tracking-wider mb-3">
                  Timeline
                </div>
                
                {selectedTrace.events && selectedTrace.events.length > 0 ? (
                  <div className="space-y-1">
                    {selectedTrace.events.map((event, i) => {
                      const Icon = EVENT_ICONS[event.type] || Zap;
                      const color = EVENT_COLORS[event.type] || 'text-muted';
                      const isExpanded = expandedEvents.has(event.id);
                      
                      return (
                        <div 
                          key={event.id}
                          className="relative"
                        >
                          {i < selectedTrace.events.length - 1 && (
                            <div className="absolute left-[11px] top-6 bottom-0 w-px bg-border" />
                          )}
                          
                          <button
                            onClick={() => event.data && toggleEventExpand(event.id)}
                            className="w-full flex items-start gap-3 p-2 rounded-md hover:bg-dark-gray/30 transition-colors text-left"
                          >
                            <div className={`p-1 rounded ${color.replace('text-', 'bg-')}/10`}>
                              <Icon className={`w-3.5 h-3.5 ${color}`} />
                            </div>
                            
                            <div className="flex-1 min-w-0">
                              <div className="flex items-center gap-2">
                                <span className="font-medium text-sm">{event.name}</span>
                                <Badge variant="outline" className="text-[10px]">{event.type}</Badge>
                                {event.status === 'error' && (
                                  <Badge variant="error" className="text-[10px]">ERROR</Badge>
                                )}
                              </div>
                              
                              <div className="flex items-center gap-3 text-[10px] text-muted mt-0.5">
                                <span>{formatTime(event.timestamp)}</span>
                                {event.duration && (
                                  <span className="flex items-center gap-1">
                                    <Timer className="w-2.5 h-2.5" />
                                    {formatDuration(event.duration)}
                                  </span>
                                )}
                              </div>
                            </div>
                            
                            {event.data && (
                              isExpanded ? (
                                <ChevronDown className="w-4 h-4 text-muted" />
                              ) : (
                                <ChevronRight className="w-4 h-4 text-muted" />
                              )
                            )}
                          </button>
                          
                          {isExpanded && event.data && (
                            <div className="ml-9 mt-1 p-3 bg-black/40 rounded overflow-x-auto max-h-64 overflow-y-auto">
                              <JsonViewer data={event.data} collapsed={false} maxDepth={4} />
                            </div>
                          )}
                        </div>
                      );
                    })}
                  </div>
                ) : (
                  <div className="text-sm text-muted text-center py-8">
                    No events in this trace
                  </div>
                )}
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

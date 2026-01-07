'use client';

import { useState, useEffect, useRef, useMemo, useCallback } from 'react';
import { Card, CardContent, CardHeader, CardTitle, Badge, Button, Input } from "@/components/ui/card";
import { 
  Terminal, Search, Filter, Pause, Play, Download, Trash2, 
  ChevronDown, ChevronRight, Clock, AlertCircle, Eye, EyeOff,
  X, RefreshCw, Wifi, WifiOff, Copy, Check, ExternalLink,
  Zap, ArrowUpRight, CornerDownRight
} from "lucide-react";
import { fetchLogs } from "@/lib/api";

interface LogEntry {
  id: string;
  timestamp: string;
  time: number;
  level: 'info' | 'warn' | 'error' | 'debug';
  message: string;
  source: string;
  traceId?: string;
  context?: Record<string, unknown>;
}

const LEVEL_CONFIG: Record<string, { 
  dot: string; 
  text: string; 
  bg: string; 
  badge: string;
  label: string;
}> = {
  debug: { 
    dot: 'bg-gray-400', 
    text: 'text-gray-400', 
    bg: 'hover:bg-gray-500/10',
    badge: 'bg-gray-500/20 text-gray-400 border-gray-500/30',
    label: 'DEBUG'
  },
  info: { 
    dot: 'bg-cyan-400', 
    text: 'text-cyan-400', 
    bg: 'hover:bg-cyan-500/10',
    badge: 'bg-cyan-500/20 text-cyan-400 border-cyan-500/30',
    label: 'INFO'
  },
  warn: { 
    dot: 'bg-yellow', 
    text: 'text-yellow', 
    bg: 'hover:bg-yellow/10',
    badge: 'bg-yellow/20 text-yellow border-yellow/30',
    label: 'WARN'
  },
  error: { 
    dot: 'bg-red-400', 
    text: 'text-red-400', 
    bg: 'hover:bg-red-500/10',
    badge: 'bg-red-500/20 text-red-400 border-red-500/30',
    label: 'ERROR'
  },
};

function formatTimestamp(time: number | string): string {
  const date = typeof time === 'number' ? new Date(time) : new Date(time);
  return date.toLocaleTimeString('en-US', { 
    hour12: false, 
    hour: '2-digit', 
    minute: '2-digit', 
    second: '2-digit',
    fractionalSecondDigits: 3
  });
}

function formatFullTimestamp(time: number | string): string {
  const date = typeof time === 'number' ? new Date(time) : new Date(time);
  return date.toISOString();
}

export default function LogsPage() {
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [searchQuery, setSearchQuery] = useState('');
  const [activeLevelFilters, setActiveLevelFilters] = useState<Set<string>>(new Set());
  const [isPaused, setIsPaused] = useState(false);
  const [loading, setLoading] = useState(true);
  const [hasLoggingAdapter, setHasLoggingAdapter] = useState(false);
  const [adapterInfo, setAdapterInfo] = useState<string[]>([]);
  const [selectedLogId, setSelectedLogId] = useState<string | undefined>();
  const [isConnected, setIsConnected] = useState(true);
  const [copied, setCopied] = useState<string | null>(null);
  const [autoScroll, setAutoScroll] = useState(true);
  const logContainerRef = useRef<HTMLDivElement>(null);
  const lastLogCountRef = useRef(0);

  const loadLogs = useCallback(async () => {
    if (isPaused) return;
    try {
      const data = await fetchLogs({ limit: 500 });
      if (data.logs && data.logs.length > 0) {
        setHasLoggingAdapter(true);
        setIsConnected(true);
        const transformedLogs: LogEntry[] = data.logs.map((log: { 
          trace_id?: string; 
          date?: string; 
          level?: string; 
          message: string; 
          function_name?: string;
          args?: string;
        }, i: number) => {
          let context: Record<string, unknown> | undefined;
          try {
            context = log.args ? JSON.parse(log.args) : undefined;
          } catch {
            context = log.args ? { raw: log.args } : undefined;
          }
          const time = log.date ? new Date(log.date).getTime() : Date.now();
          return {
            id: `${log.trace_id || time}-${i}`,
            timestamp: log.date || new Date().toISOString(),
            time,
            level: (log.level as LogEntry['level']) || 'info',
            message: log.message,
            source: log.function_name || 'unknown',
            traceId: log.trace_id,
            context,
          };
        });
        
        if (transformedLogs.length > lastLogCountRef.current && autoScroll && logContainerRef.current) {
          setTimeout(() => {
            logContainerRef.current?.scrollTo({ top: 0, behavior: 'smooth' });
          }, 100);
        }
        lastLogCountRef.current = transformedLogs.length;
        setLogs(transformedLogs);
      } else {
        setHasLoggingAdapter(data.logs !== undefined);
        setAdapterInfo(data.info?.adapters || []);
      }
    } catch (err) {
      console.error('Failed to fetch logs:', err);
      setIsConnected(false);
    } finally {
      setLoading(false);
    }
  }, [isPaused, autoScroll]);

  useEffect(() => {
    loadLogs();
    const interval = setInterval(loadLogs, 2000);
    return () => clearInterval(interval);
  }, [loadLogs]);

  const filteredLogs = useMemo(() => {
    return logs.filter(log => {
      if (activeLevelFilters.size > 0 && !activeLevelFilters.has(log.level)) {
        return false;
      }
      
      if (searchQuery) {
        const searchLower = searchQuery.toLowerCase();
        return (
          log.message?.toLowerCase().includes(searchLower) ||
          log.traceId?.toLowerCase().includes(searchLower) ||
          log.source?.toLowerCase().includes(searchLower)
        );
      }
      
      return true;
    });
  }, [logs, searchQuery, activeLevelFilters]);

  const selectedLog = useMemo(() => {
    return selectedLogId ? logs.find(log => log.id === selectedLogId) : undefined;
  }, [logs, selectedLogId]);

  const clearLogs = () => {
    setLogs([]);
    setSelectedLogId(undefined);
    lastLogCountRef.current = 0;
  };

  const toggleLevelFilter = (level: string) => {
    setActiveLevelFilters(prev => {
      const next = new Set(prev);
      if (next.has(level)) {
        next.delete(level);
      } else {
        next.add(level);
      }
      return next;
    });
  };

  const filterByTrace = (traceId: string) => {
    setSearchQuery(traceId);
  };

  const copyToClipboard = (text: string, key: string) => {
    navigator.clipboard.writeText(text);
    setCopied(key);
    setTimeout(() => setCopied(null), 2000);
  };

  const exportLogs = () => {
    const exportData = filteredLogs.map(log => ({
      timestamp: log.timestamp,
      level: log.level,
      source: log.source,
      message: log.message,
      traceId: log.traceId,
      context: log.context
    }));
    const blob = new Blob([JSON.stringify(exportData, null, 2)], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `iii-logs-${new Date().toISOString().slice(0, 10)}.json`;
    a.click();
    URL.revokeObjectURL(url);
  };

  const levelCounts = useMemo(() => ({
    info: logs.filter(l => l.level === 'info').length,
    warn: logs.filter(l => l.level === 'warn').length,
    error: logs.filter(l => l.level === 'error').length,
    debug: logs.filter(l => l.level === 'debug').length,
  }), [logs]);

  const sourceCounts = useMemo(() => {
    const counts: Record<string, number> = {};
    logs.forEach(log => {
      counts[log.source] = (counts[log.source] || 0) + 1;
    });
    return Object.entries(counts)
      .sort((a, b) => b[1] - a[1])
      .slice(0, 5);
  }, [logs]);

  return (
    <div className="flex flex-col h-full bg-background text-foreground">
      <div className="flex items-center justify-between px-5 py-3 bg-dark-gray/30 border-b border-border">
        <div className="flex items-center gap-4">
          <h1 className="text-base font-semibold flex items-center gap-2">
            <Terminal className="w-4 h-4" />
            Logs
          </h1>
          <Badge 
            variant={isConnected && !isPaused ? 'success' : isPaused ? 'warning' : 'error'}
            className="gap-1.5"
          >
            {isPaused ? (
              <>
                <Pause className="w-3 h-3" />
                Paused
              </>
            ) : isConnected ? (
              <>
                <Wifi className="w-3 h-3" />
                Live
              </>
            ) : (
              <>
                <WifiOff className="w-3 h-3" />
                Offline
              </>
            )}
          </Badge>
          {logs.length > 0 && (
            <span className="text-xs text-muted">
              {filteredLogs.length} of {logs.length} entries
            </span>
          )}
        </div>
        
        <div className="flex items-center gap-2">
          <Button 
            variant={isPaused ? 'accent' : 'ghost'} 
            size="sm" 
            onClick={() => setIsPaused(!isPaused)}
            disabled={!hasLoggingAdapter}
            className="h-7 text-xs"
          >
            {isPaused ? <Play className="w-3 h-3 mr-1.5" /> : <Pause className="w-3 h-3 mr-1.5" />}
            {isPaused ? 'Resume' : 'Pause'}
          </Button>
          <Button 
            variant="ghost" 
            size="sm" 
            onClick={exportLogs}
            disabled={filteredLogs.length === 0}
            className="h-7 text-xs"
          >
            <Download className="w-3 h-3 mr-1.5" />
            Export
          </Button>
          <Button 
            variant="ghost" 
            size="sm" 
            onClick={clearLogs} 
            disabled={logs.length === 0}
            className="h-7 text-xs"
          >
            <Trash2 className="w-3 h-3 mr-1.5" />
            Clear
          </Button>
        </div>
      </div>

      {!loading && !hasLoggingAdapter && (
        <div className="flex-1 flex items-center justify-center p-8">
          <div className="text-center max-w-md">
            <div className="w-16 h-16 mb-4 mx-auto rounded-2xl bg-dark-gray border border-border flex items-center justify-center">
              <AlertCircle className="w-8 h-8 text-yellow" />
            </div>
            <h3 className="text-sm font-medium mb-2">No Logging Adapter Configured</h3>
            <p className="text-xs text-muted mb-4">
              Configure a logging adapter in your <code className="bg-dark-gray px-1 rounded">config.yaml</code> to enable log storage and viewing.
            </p>
            <div className="bg-dark-gray/50 rounded-lg p-4 text-left mb-4">
              <code className="text-[10px] text-muted block">
                <span className="text-cyan-400">modules:</span><br />
                &nbsp;&nbsp;<span className="text-yellow">-</span> <span className="text-cyan-400">class:</span> modules::observability::LoggingModule<br />
                &nbsp;&nbsp;&nbsp;&nbsp;<span className="text-cyan-400">config:</span><br />
                &nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;<span className="text-cyan-400">adapter:</span><br />
                &nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;<span className="text-cyan-400">class:</span> modules::observability::adapters::RedisLogger<br />
                &nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;<span className="text-cyan-400">config:</span><br />
                &nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;<span className="text-cyan-400">redis_url:</span> redis://localhost:6379
              </code>
            </div>
            <div className="flex flex-wrap gap-2 justify-center">
              {adapterInfo.map(adapter => (
                <Badge key={adapter} variant="default">{adapter}</Badge>
              ))}
            </div>
          </div>
        </div>
      )}

      {hasLoggingAdapter && (
        <>
          <div className="flex items-center gap-2 p-2 border-b border-border bg-dark-gray/20">
            <div className="flex-1 relative">
              <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-muted" />
              <Input
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                className="pl-9 pr-9 h-9 font-medium"
                placeholder="Search by Trace ID, Source, or Message..."
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
            
            <div className="flex items-center gap-1 px-2 border-l border-border">
              {(['info', 'warn', 'error', 'debug'] as const).map((level) => {
                const config = LEVEL_CONFIG[level];
                const isActive = activeLevelFilters.has(level);
                const count = levelCounts[level];
                
                return (
                  <button
                    key={level}
                    onClick={() => toggleLevelFilter(level)}
                    className={`flex items-center gap-1.5 px-2.5 py-1.5 rounded text-xs transition-all ${
                      isActive 
                        ? `${config.badge} border` 
                        : 'hover:bg-dark-gray/50 border border-transparent'
                    }`}
                    title={`${isActive ? 'Hide' : 'Show only'} ${level} logs`}
                  >
                    <span className={`w-2 h-2 rounded-full ${config.dot}`} />
                    <span className={`font-medium tabular-nums ${isActive ? config.text : 'text-muted'}`}>
                      {count}
                    </span>
                  </button>
                );
              })}
              
              {activeLevelFilters.size > 0 && (
                <button
                  onClick={() => setActiveLevelFilters(new Set())}
                  className="ml-1 p-1 rounded hover:bg-dark-gray/50 text-muted hover:text-foreground transition-colors"
                  title="Clear level filters"
                >
                  <X className="w-3.5 h-3.5" />
                </button>
              )}
            </div>
          </div>

          <div className="flex-1 flex overflow-hidden">
            <div ref={logContainerRef} className="flex-1 overflow-auto">
              <div className="relative w-full">
                {filteredLogs.length === 0 ? (
                  <div className="flex flex-col items-center justify-center h-64 text-muted">
                    <Terminal className="w-8 h-8 mb-3 opacity-30" />
                    <span className="text-sm">
                      {searchQuery || activeLevelFilters.size > 0 
                        ? 'No logs match your filters' 
                        : 'No logs to display'}
                    </span>
                    {(searchQuery || activeLevelFilters.size > 0) && (
                      <button
                        onClick={() => {
                          setSearchQuery('');
                          setActiveLevelFilters(new Set());
                        }}
                        className="mt-2 text-xs text-primary hover:underline"
                      >
                        Clear all filters
                      </button>
                    )}
                  </div>
                ) : (
                  filteredLogs.map((log) => {
                    const config = LEVEL_CONFIG[log.level] || LEVEL_CONFIG.info;
                    const isSelected = selectedLogId === log.id;
                    
                    return (
                      <div
                        key={log.id}
                        role="row"
                        tabIndex={0}
                        className={`flex items-center font-mono cursor-pointer text-[13px] h-9 px-3 border-b border-border/20 transition-colors
                          ${isSelected ? 'bg-primary/10 border-l-2 border-l-primary' : `${config.bg} border-l-2 border-l-transparent`}
                        `}
                        onClick={() => setSelectedLogId(isSelected ? undefined : log.id)}
                        onKeyDown={(e) => {
                          if (e.key === 'Enter' || e.key === ' ') {
                            setSelectedLogId(isSelected ? undefined : log.id);
                          }
                        }}
                      >
                        <div className="flex items-center gap-2 text-muted shrink-0 w-[100px]">
                          <span className={`w-2 h-2 rounded-full shrink-0 ${config.dot}`} />
                          <span className="text-xs">{formatTimestamp(log.time)}</span>
                        </div>
                        
                        {log.traceId && (
                          <div className="flex items-center shrink-0 ml-2">
                            <span className="text-muted font-mono text-[11px] bg-black/30 px-1.5 py-0.5 rounded">
                              {log.traceId.slice(0, 8)}
                            </span>
                            <button
                              onClick={(e) => {
                                e.stopPropagation();
                                filterByTrace(log.traceId!);
                              }}
                              className="p-1 rounded hover:bg-dark-gray/50 text-muted hover:text-primary transition-colors ml-0.5"
                              title={`Filter by trace ${log.traceId}`}
                            >
                              <Filter className="w-3 h-3" />
                            </button>
                          </div>
                        )}
                        
                        <div className="text-muted shrink-0 ml-3 px-2 py-0.5 bg-dark-gray/50 rounded text-[11px] max-w-[180px] truncate">
                          {log.source}
                        </div>
                        
                        <div className={`ml-3 flex-1 truncate ${config.text}`}>
                          {log.message}
                        </div>
                        
                        {log.context && Object.keys(log.context).length > 0 && (
                          <div className="shrink-0 ml-2">
                            <span className="text-[10px] text-muted bg-dark-gray/50 px-1.5 py-0.5 rounded">
                              +{Object.keys(log.context).length}
                            </span>
                          </div>
                        )}
                      </div>
                    );
                  })
                )}
              </div>
            </div>

            {selectedLog && (
              <div className="w-[400px] border-l border-border bg-dark-gray/20 overflow-y-auto">
                <div className="p-4 border-b border-border sticky top-0 bg-dark-gray/80 backdrop-blur z-10">
                  <div className="flex items-center justify-between">
                    <div className="flex items-center gap-2">
                      <span className={`px-2 py-0.5 rounded text-[10px] font-semibold uppercase ${LEVEL_CONFIG[selectedLog.level]?.badge}`}>
                        {selectedLog.level}
                      </span>
                    </div>
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => setSelectedLogId(undefined)}
                      className="h-6 w-6 p-0"
                    >
                      <X className="w-3.5 h-3.5" />
                    </Button>
                  </div>
                  <p className="text-xs text-muted mt-2 font-mono">{formatFullTimestamp(selectedLog.time)}</p>
                </div>

                <div className="p-4 space-y-5">
                  <div>
                    <div className="text-[10px] text-muted uppercase tracking-wider mb-2 flex items-center gap-2">
                      Message
                      <button
                        onClick={() => copyToClipboard(selectedLog.message, 'message')}
                        className="p-0.5 rounded hover:bg-dark-gray/50 transition-colors"
                      >
                        {copied === 'message' ? (
                          <Check className="w-3 h-3 text-success" />
                        ) : (
                          <Copy className="w-3 h-3" />
                        )}
                      </button>
                    </div>
                    <p className={`text-sm leading-relaxed ${LEVEL_CONFIG[selectedLog.level]?.text}`}>
                      {selectedLog.message}
                    </p>
                  </div>

                  {selectedLog.traceId && (
                    <div>
                      <div className="text-[10px] text-muted uppercase tracking-wider mb-2 flex items-center gap-2">
                        Trace ID
                        <button
                          onClick={() => copyToClipboard(selectedLog.traceId!, 'traceId')}
                          className="p-0.5 rounded hover:bg-dark-gray/50 transition-colors"
                        >
                          {copied === 'traceId' ? (
                            <Check className="w-3 h-3 text-success" />
                          ) : (
                            <Copy className="w-3 h-3" />
                          )}
                        </button>
                      </div>
                      <div className="flex items-center gap-2">
                        <code className="text-xs font-mono bg-black/40 px-2 py-1.5 rounded flex-1 break-all">
                          {selectedLog.traceId}
                        </code>
                        <button
                          onClick={() => filterByTrace(selectedLog.traceId!)}
                          className="p-1.5 rounded bg-primary/20 hover:bg-primary/30 text-primary transition-colors"
                          title="Filter by this trace"
                        >
                          <Filter className="w-3.5 h-3.5" />
                        </button>
                      </div>
                    </div>
                  )}

                  <div>
                    <div className="text-[10px] text-muted uppercase tracking-wider mb-2">Source Function</div>
                    <div className="flex items-center gap-2">
                      <code className="text-xs font-mono bg-cyan-500/10 text-cyan-400 px-2 py-1.5 rounded border border-cyan-500/20">
                        <Zap className="w-3 h-3 inline mr-1.5" />
                        {selectedLog.source}
                      </code>
                    </div>
                  </div>

                  <div>
                    <div className="text-[10px] text-muted uppercase tracking-wider mb-2">Timestamp</div>
                    <div className="grid grid-cols-2 gap-2">
                      <div className="bg-black/30 rounded px-2 py-1.5">
                        <div className="text-[9px] text-muted uppercase">ISO</div>
                        <code className="text-[10px] font-mono">{selectedLog.timestamp}</code>
                      </div>
                      <div className="bg-black/30 rounded px-2 py-1.5">
                        <div className="text-[9px] text-muted uppercase">Unix</div>
                        <code className="text-[10px] font-mono">{selectedLog.time}</code>
                      </div>
                    </div>
                  </div>

                  {selectedLog.context && Object.keys(selectedLog.context).length > 0 && (
                    <div>
                      <div className="text-[10px] text-muted uppercase tracking-wider mb-2 flex items-center gap-2">
                        Context Data
                        <span className="text-[9px] bg-dark-gray/50 px-1.5 py-0.5 rounded">
                          {Object.keys(selectedLog.context).length} fields
                        </span>
                        <button
                          onClick={() => copyToClipboard(JSON.stringify(selectedLog.context, null, 2), 'context')}
                          className="p-0.5 rounded hover:bg-dark-gray/50 transition-colors ml-auto"
                        >
                          {copied === 'context' ? (
                            <Check className="w-3 h-3 text-success" />
                          ) : (
                            <Copy className="w-3 h-3" />
                          )}
                        </button>
                      </div>
                      <pre className="text-[11px] font-mono bg-black/40 px-3 py-2 rounded overflow-x-auto max-h-64 leading-relaxed">
                        {JSON.stringify(selectedLog.context, null, 2)}
                      </pre>
                    </div>
                  )}
                </div>
              </div>
            )}
          </div>

          {sourceCounts.length > 0 && (
            <div className="flex items-center gap-3 px-4 py-2 border-t border-border bg-dark-gray/20 text-[11px]">
              <span className="text-muted">Top sources:</span>
              {sourceCounts.map(([source, count]) => (
                <button
                  key={source}
                  onClick={() => setSearchQuery(source)}
                  className="flex items-center gap-1.5 px-2 py-1 rounded bg-dark-gray/50 hover:bg-dark-gray transition-colors"
                >
                  <span className="text-foreground font-medium truncate max-w-[120px]">{source}</span>
                  <span className="text-muted">({count})</span>
                </button>
              ))}
            </div>
          )}
        </>
      )}
    </div>
  );
}

'use client';

import { useState, useEffect, useRef } from 'react';
import { Card, CardContent, CardHeader, CardTitle, Badge, Button, Input } from "@/components/ui/card";
import { Terminal, Search, Filter, Pause, Play, Download, Trash2, ChevronDown, ChevronRight, Clock, AlertCircle, Eye, EyeOff } from "lucide-react";
import { fetchLogs } from "@/lib/api";

interface LogEntry {
  id: string;
  timestamp: string;
  level: 'info' | 'warn' | 'error' | 'debug';
  message: string;
  source: string;
  context?: Record<string, unknown>;
}

const LEVEL_COLORS: Record<string, { badge: 'default' | 'success' | 'warning' | 'error', text: string, bg: string, icon: string }> = {
  info: { badge: 'success', text: 'text-foreground', bg: 'bg-cyan-500/10 border-l-2 border-cyan-500/50', icon: 'text-cyan-400' },
  debug: { badge: 'default', text: 'text-muted', bg: 'bg-gray-500/5 border-l-2 border-gray-500/30', icon: 'text-gray-500' },
  warn: { badge: 'warning', text: 'text-yellow', bg: 'bg-yellow/10 border-l-2 border-yellow/50', icon: 'text-yellow' },
  error: { badge: 'error', text: 'text-error', bg: 'bg-red-500/10 border-l-2 border-red-500/50', icon: 'text-red-400' },
};

export default function LogsPage() {
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [levelFilter, setLevelFilter] = useState('all');
  const [searchQuery, setSearchQuery] = useState('');
  const [isPaused, setIsPaused] = useState(false);
  const [expandedLogs, setExpandedLogs] = useState<Set<string>>(new Set());
  const [autoScroll, setAutoScroll] = useState(true);
  const [showSystem, setShowSystem] = useState(false);
  const [loading, setLoading] = useState(true);
  const [hasLoggingAdapter, setHasLoggingAdapter] = useState(false);
  const [adapterInfo, setAdapterInfo] = useState<string[]>([]);
  const logContainerRef = useRef<HTMLDivElement>(null);

  const loadLogs = async () => {
    setLoading(true);
    try {
      const data = await fetchLogs({ limit: 100 });
      if (data.logs && data.logs.length > 0) {
        setHasLoggingAdapter(true);
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
          return {
            id: `${i}-${log.trace_id || Date.now()}`,
            timestamp: log.date ? new Date(log.date).toISOString().replace('T', ' ').slice(0, 23) : new Date().toISOString().replace('T', ' ').slice(0, 23),
            level: (log.level as LogEntry['level']) || 'info',
            message: log.message,
            source: log.function_name || 'unknown',
            context,
          };
        });
        setLogs(transformedLogs);
      } else {
        setHasLoggingAdapter(false);
        setAdapterInfo(data.info?.adapters || []);
      }
    } catch (err) {
      console.error('Failed to fetch logs:', err);
      setHasLoggingAdapter(false);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    loadLogs();
  }, []);

  useEffect(() => {
    if (autoScroll && logContainerRef.current) {
      logContainerRef.current.scrollTop = logContainerRef.current.scrollHeight;
    }
  }, [logs, autoScroll]);

  const filteredLogs = logs.filter(log => {
    if (levelFilter !== 'all' && log.level !== levelFilter) return false;
    if (searchQuery && !log.message.toLowerCase().includes(searchQuery.toLowerCase())) return false;
    return true;
  });

  const toggleExpand = (id: string) => {
    const newExpanded = new Set(expandedLogs);
    if (newExpanded.has(id)) {
      newExpanded.delete(id);
    } else {
      newExpanded.add(id);
    }
    setExpandedLogs(newExpanded);
  };

  const clearLogs = () => setLogs([]);

  const levelCounts = {
    info: logs.filter(l => l.level === 'info').length,
    warn: logs.filter(l => l.level === 'warn').length,
    error: logs.filter(l => l.level === 'error').length,
    debug: logs.filter(l => l.level === 'debug').length,
  };

  return (
    <div className="p-6 space-y-6 h-[calc(100vh-48px)] flex flex-col">
      {}
      <div className="flex items-center justify-between flex-shrink-0">
        <div>
          <h1 className="text-xl font-semibold tracking-tight">Logs</h1>
          <p className="text-xs text-muted mt-1 tracking-wide">
            Real-time log stream from iii engine
          </p>
        </div>
        <div className="flex items-center gap-2">
          <Button 
            variant={showSystem ? "accent" : "ghost"} 
            size="sm" 
            onClick={() => setShowSystem(!showSystem)}
          >
            {showSystem ? <EyeOff className="w-3 h-3 mr-2" /> : <Eye className="w-3 h-3 mr-2" />}
            {showSystem ? "Hide System" : "Show System"}
          </Button>
          <Button 
            variant={isPaused ? 'accent' : 'outline'} 
            size="sm" 
            onClick={() => setIsPaused(!isPaused)}
            disabled={!hasLoggingAdapter}
          >
            {isPaused ? <Play className="w-3 h-3 mr-2" /> : <Pause className="w-3 h-3 mr-2" />}
            {isPaused ? 'Resume' : 'Pause'}
          </Button>
          <Button variant="outline" size="sm" onClick={clearLogs} disabled={logs.length === 0}>
            <Trash2 className="w-3 h-3 mr-2" />
            Clear
          </Button>
          <Button variant="outline" size="sm" disabled={logs.length === 0}>
            <Download className="w-3 h-3 mr-2" />
            Export
          </Button>
        </div>
      </div>

      {}
      {!loading && !hasLoggingAdapter && (
        <Card className="flex-shrink-0">
          <CardContent className="py-8">
            <div className="flex flex-col items-center justify-center text-center">
              <AlertCircle className="w-12 h-12 text-yellow mb-4" />
              <h3 className="text-sm font-medium mb-2">No Logging Adapter Configured</h3>
              <p className="text-xs text-muted max-w-md mb-4">
                Logs are collected by the ObservabilityModule. Configure a logging adapter in your <code className="bg-dark-gray px-1 rounded">config.yaml</code> to enable log storage and viewing.
              </p>
              <div className="flex flex-wrap gap-2 justify-center">
                {adapterInfo.map(adapter => (
                  <Badge key={adapter} variant="default">{adapter}</Badge>
                ))}
              </div>
              <p className="text-[10px] text-muted mt-4">
                Available adapters: file_logger, redis_logger
              </p>
            </div>
          </CardContent>
        </Card>
      )}

      {}
      {hasLoggingAdapter && (
        <div className="grid grid-cols-5 gap-4 flex-shrink-0">
          <Card>
            <CardContent className="py-3 flex items-center justify-between">
              <div>
                <p className="text-[10px] text-muted uppercase tracking-wider">Total</p>
                <p className="text-2xl font-semibold">{logs.length}</p>
              </div>
              <Terminal className="w-5 h-5 text-muted" />
            </CardContent>
          </Card>
          {['info', 'warn', 'error', 'debug'].map(level => (
            <Card key={level}>
              <CardContent className="py-3 flex items-center justify-between">
                <div>
                  <p className="text-[10px] text-muted uppercase tracking-wider">{level}</p>
                  <p className={`text-2xl font-semibold ${LEVEL_COLORS[level]?.text || ''}`}>
                    {levelCounts[level as keyof typeof levelCounts]}
                  </p>
                </div>
              </CardContent>
            </Card>
          ))}
        </div>
      )}

      {}
      {hasLoggingAdapter && (
        <Card className="flex-shrink-0">
          <CardContent className="py-3 flex items-center justify-between">
            <div className="flex items-center gap-4">
              <div className="flex items-center gap-2">
                <Filter className="w-3 h-3 text-muted" />
                <span className="text-[10px] text-muted uppercase tracking-wider">Level</span>
              </div>
              <div className="flex items-center gap-1">
                {['all', 'info', 'warn', 'error', 'debug'].map(level => (
                  <button
                    key={level}
                    onClick={() => setLevelFilter(level)}
                    className={`px-2 py-1 text-[10px] uppercase tracking-wider rounded transition-colors ${
                      levelFilter === level 
                        ? 'bg-white text-black' 
                        : 'text-muted hover:text-foreground hover:bg-dark-gray'
                    }`}
                  >
                    {level}
                  </button>
                ))}
              </div>
            </div>
            <div className="relative">
              <Search className="w-3 h-3 absolute left-3 top-1/2 -translate-y-1/2 text-muted" />
              <Input 
                placeholder="Search logs..."
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                className="pl-8 w-64"
              />
            </div>
          </CardContent>
        </Card>
      )}

      {}
      {hasLoggingAdapter && (
        <Card className="flex-1 overflow-hidden flex flex-col">
          <CardHeader className="flex-shrink-0 flex flex-row items-center justify-between py-3">
            <div className="flex items-center gap-2">
              <Terminal className="w-4 h-4 text-muted" />
              <CardTitle>Log Stream</CardTitle>
              {!isPaused && (
                <span className="flex items-center gap-1 text-[10px] text-success">
                  <span className="w-1.5 h-1.5 rounded-full bg-success animate-pulse" />
                  LIVE
                </span>
              )}
            </div>
            <div className="flex items-center gap-2">
              <button 
                onClick={() => setAutoScroll(!autoScroll)}
                className={`text-[10px] uppercase tracking-wider px-2 py-1 rounded ${autoScroll ? 'bg-dark-gray text-foreground' : 'text-muted'}`}
              >
                Auto-scroll: {autoScroll ? 'ON' : 'OFF'}
              </button>
            </div>
          </CardHeader>
          <CardContent ref={logContainerRef} className="flex-1 overflow-y-auto p-0 font-mono text-xs">
            {filteredLogs.length === 0 ? (
              <div className="flex items-center justify-center h-full text-muted">
                No logs to display
              </div>
            ) : (
              <div className="space-y-1 p-2">
                {filteredLogs.map(log => {
                  const colors = LEVEL_COLORS[log.level] || LEVEL_COLORS.info;
                  return (
                    <div 
                      key={log.id}
                      className={`px-4 py-2.5 rounded-md transition-all hover:bg-dark-gray/50 ${colors.bg}`}
                    >
                      <div className="flex items-center gap-4">
                        <button 
                          onClick={() => log.context && toggleExpand(log.id)} 
                          className="flex-shrink-0"
                        >
                          {log.context ? (
                            expandedLogs.has(log.id) ? 
                              <ChevronDown className={`w-3.5 h-3.5 ${colors.icon}`} /> : 
                              <ChevronRight className={`w-3.5 h-3.5 ${colors.icon}`} />
                          ) : (
                            <span className="w-3.5 h-3.5 inline-block" />
                          )}
                        </button>
                        <span className="text-muted/70 w-36 flex-shrink-0 font-mono text-[11px]">
                          {log.timestamp.split(' ')[1] || log.timestamp}
                        </span>
                        <Badge 
                          variant={colors.badge} 
                          className="w-14 justify-center flex-shrink-0 text-[10px] font-semibold"
                        >
                          {log.level.toUpperCase()}
                        </Badge>
                        <span className={`flex-1 truncate ${colors.text}`}>
                          {log.message}
                        </span>
                        <span className="text-muted/50 text-[10px] flex-shrink-0 font-mono bg-dark-gray/50 px-2 py-0.5 rounded">
                          {log.source}
                        </span>
                      </div>
                      {log.context && expandedLogs.has(log.id) && (
                        <pre className="mt-3 ml-8 p-3 bg-black/60 rounded-md text-[10px] overflow-x-auto border border-border/30 text-foreground/80">
                          {JSON.stringify(log.context, null, 2)}
                        </pre>
                      )}
                    </div>
                  );
                })}
              </div>
            )}
          </CardContent>
        </Card>
      )}

      {}
      <div className="flex items-center justify-between text-[10px] text-muted flex-shrink-0">
        <div className="flex items-center gap-4">
          <span>Log Levels: DEBUG, INFO, WARN, ERROR</span>
        </div>
        <div className="flex items-center gap-2">
          <Clock className="w-3 h-3" />
          <span>Last updated: {new Date().toLocaleTimeString()}</span>
        </div>
      </div>
    </div>
  );
}

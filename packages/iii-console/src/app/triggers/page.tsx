'use client';

import { useEffect, useState, useMemo } from 'react';
import { Card, CardContent, CardHeader, CardTitle, Badge, Button, Input, Select } from "@/components/ui/card";
import { 
  Zap, Search, RefreshCw, Globe, Calendar, MessageSquare, Play, Pause, 
  Eye, EyeOff, Copy, ChevronDown, ChevronRight, ExternalLink, Clock, Hash,
  AlertCircle, CheckCircle, Timer, Activity, X, Wifi, WifiOff, Loader2
} from "lucide-react";
import { fetchTriggers } from "@/lib/api";

interface Trigger {
  id: string;
  trigger_type: string;
  function_path: string;
  config: Record<string, unknown>;
  worker_id: string | null;
  internal?: boolean;
  enabled?: boolean;
  status?: 'idle' | 'running' | 'completed' | 'failed';
  lastRunAt?: number;
  nextRunAt?: number;
  lastDuration?: number;
  runCount?: number;
  errorCount?: number;
  lastError?: string;
}

const TRIGGER_ICONS: Record<string, typeof Zap> = {
  'api': Globe,
  'cron': Calendar,
  'event': MessageSquare,
  'streams:join': Play,
  'streams:leave': Pause,
};

const STATUS_CONFIG: Record<string, { color: string; icon: typeof Clock; label: string }> = {
  idle: { color: 'text-cyan-400', icon: Clock, label: 'Idle' },
  running: { color: 'text-yellow', icon: Loader2, label: 'Running' },
  completed: { color: 'text-success', icon: CheckCircle, label: 'Completed' },
  failed: { color: 'text-error', icon: AlertCircle, label: 'Failed' },
};

function formatRelativeTime(timestamp: number | undefined): string {
  if (!timestamp) return 'Never';
  const now = Date.now();
  const diff = timestamp - now;
  const absDiff = Math.abs(diff);

  if (absDiff < 60_000) {
    const seconds = Math.round(absDiff / 1000);
    return diff > 0 ? `in ${seconds}s` : `${seconds}s ago`;
  }
  if (absDiff < 3600_000) {
    const minutes = Math.round(absDiff / 60_000);
    return diff > 0 ? `in ${minutes}m` : `${minutes}m ago`;
  }
  if (absDiff < 86400_000) {
    const hours = Math.round(absDiff / 3600_000);
    return diff > 0 ? `in ${hours}h` : `${hours}h ago`;
  }
  const days = Math.round(absDiff / 86400_000);
  return diff > 0 ? `in ${days}d` : `${days}d ago`;
}

function formatDuration(ms: number | undefined): string {
  if (!ms) return '-';
  if (ms < 1000) return `${ms}ms`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)}s`;
  return `${(ms / 60_000).toFixed(1)}m`;
}

function parseCronExpression(cron: string): string {
  const parts = cron.split(' ');
  if (parts.length < 5) return cron;
  
  const [min, hour, dom, month, dow] = parts;
  
  if (cron === '* * * * *') return 'Every minute';
  if (cron === '0 * * * *') return 'Every hour';
  if (cron === '0 0 * * *') return 'Every day at midnight';
  if (cron === '0 0 * * 0') return 'Every Sunday at midnight';
  if (min === '*/5' && hour === '*') return 'Every 5 minutes';
  if (min === '*/10' && hour === '*') return 'Every 10 minutes';
  if (min === '*/15' && hour === '*') return 'Every 15 minutes';
  if (min === '*/30' && hour === '*') return 'Every 30 minutes';
  if (min === '0' && hour !== '*' && dom === '*') return `Every day at ${hour}:00`;
  
  return cron;
}

export default function TriggersPage() {
  const [triggers, setTriggers] = useState<Trigger[]>([]);
  const [loading, setLoading] = useState(true);
  const [typeFilter, setTypeFilter] = useState('all');
  const [statusFilter, setStatusFilter] = useState('all');
  const [searchQuery, setSearchQuery] = useState('');
  const [showSystem, setShowSystem] = useState(false);
  const [expandedTriggers, setExpandedTriggers] = useState<Set<string>>(new Set());
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const [showSearch, setShowSearch] = useState(false);
  const [selectedTriggerId, setSelectedTriggerId] = useState<string | null>(null);
  const [isConnected] = useState(true);

  useEffect(() => {
    loadTriggers();
    const interval = setInterval(loadTriggers, 5000);
    return () => clearInterval(interval);
  }, []);

  const loadTriggers = () => {
    setLoading(true);
    fetchTriggers()
      .then(data => setTriggers(data?.triggers || []))
      .catch(() => {})
      .finally(() => setLoading(false));
  };

  const toggleExpand = (id: string) => {
    setExpandedTriggers(prev => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const copyToClipboard = (text: string, id: string) => {
    navigator.clipboard.writeText(text);
    setCopiedId(id);
    setTimeout(() => setCopiedId(null), 2000);
  };

  const stats = useMemo(() => {
    const userTriggers = triggers.filter(t => !t.internal || showSystem);
    const cronTriggers = userTriggers.filter(t => t.trigger_type === 'cron');
    const apiTriggers = userTriggers.filter(t => t.trigger_type === 'api');
    const eventTriggers = userTriggers.filter(t => t.trigger_type === 'event');
    const runningCount = userTriggers.filter(t => t.status === 'running').length;
    const failedCount = userTriggers.filter(t => t.errorCount && t.errorCount > 0).length;
    const nextCron = cronTriggers
      .filter(t => t.nextRunAt)
      .sort((a, b) => (a.nextRunAt || 0) - (b.nextRunAt || 0))[0];

    return {
      total: userTriggers.length,
      cron: cronTriggers.length,
      api: apiTriggers.length,
      events: eventTriggers.length,
      running: runningCount,
      failed: failedCount,
      nextScheduled: nextCron,
    };
  }, [triggers, showSystem]);

  const filteredTriggers = useMemo(() => {
    return triggers.filter(trigger => {
      if (!showSystem && trigger.internal) return false;
      if (typeFilter !== 'all' && trigger.trigger_type !== typeFilter) return false;
      if (statusFilter !== 'all' && trigger.status !== statusFilter) return false;
      if (searchQuery && !trigger.function_path?.toLowerCase().includes(searchQuery.toLowerCase())) return false;
      return true;
    });
  }, [triggers, showSystem, typeFilter, statusFilter, searchQuery]);

  const triggerTypes = [...new Set(triggers.filter(t => !t.internal || showSystem).map(t => t.trigger_type))];

  const getApiEndpoint = (trigger: Trigger): string | null => {
    if (trigger.trigger_type !== 'api') return null;
    const config = trigger.config as { api_path?: string; http_method?: string };
    const method = config.http_method || 'GET';
    const path = config.api_path || trigger.function_path?.replace(/^api\./, '').replace(/\./g, '/');
    return `${method} /${path}`;
  };

  const getCronSchedule = (trigger: Trigger): string | null => {
    if (trigger.trigger_type !== 'cron') return null;
    const config = trigger.config as { schedule?: string; interval?: string };
    return config.schedule || config.interval || null;
  };

  const selectedTrigger = triggers.find(t => t.id === selectedTriggerId);

  return (
    <div className="flex flex-col h-full bg-background text-foreground">
      <div className="flex items-center justify-between px-5 py-3 bg-dark-gray/30 border-b border-border">
        <div className="flex items-center gap-4">
          <h1 className="text-base font-semibold flex items-center gap-2">
            <Zap className="w-4 h-4 text-yellow" />
            Triggers
          </h1>
          <Badge 
            variant={isConnected ? 'success' : 'error'}
            className="gap-1.5"
          >
            {isConnected ? <Wifi className="w-3 h-3" /> : <WifiOff className="w-3 h-3" />}
            {isConnected ? 'Live' : 'Offline'}
          </Badge>
        </div>
        
        <div className="flex items-center gap-2">
          {showSearch ? (
            <div className="flex items-center gap-2">
              <div className="relative">
                <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-muted pointer-events-none" />
                <Input
                  type="text"
                  value={searchQuery}
                  onChange={(e) => setSearchQuery(e.target.value)}
                  placeholder="Search triggers..."
                  className="w-48 h-7 pl-8 text-xs"
                  autoFocus
                  onKeyDown={(e) => {
                    if (e.key === 'Escape') {
                      setShowSearch(false);
                      setSearchQuery('');
                    }
                  }}
                />
              </div>
              <Button
                variant="ghost"
                size="sm"
                onClick={() => { setShowSearch(false); setSearchQuery(''); }}
                className="h-7 w-7 p-0"
              >
                <X className="w-3.5 h-3.5" />
              </Button>
            </div>
          ) : (
            <Button
              variant="ghost"
              size="sm"
              onClick={() => setShowSearch(true)}
              className="h-7 w-7 p-0 text-muted hover:text-foreground"
            >
              <Search className="w-3.5 h-3.5" />
            </Button>
          )}
          
          <Button 
            variant={showSystem ? "accent" : "ghost"} 
            size="sm" 
            onClick={() => setShowSystem(!showSystem)}
            className="h-7 text-xs"
          >
            {showSystem ? <EyeOff className="w-3 h-3 mr-1.5" /> : <Eye className="w-3 h-3 mr-1.5" />}
            System
          </Button>
          
          <Button 
            variant="ghost" 
            size="sm" 
            onClick={loadTriggers} 
            disabled={loading}
            className="h-7 text-xs text-muted hover:text-foreground"
          >
            <RefreshCw className={`w-3.5 h-3.5 mr-1.5 ${loading ? 'animate-spin' : ''}`} />
            Refresh
          </Button>
        </div>
      </div>

      <div className="px-5 py-3 bg-dark-gray/20 border-b border-border/50">
        <div className="flex items-center gap-6">
          <div className="flex items-center gap-3">
            <span className="text-[11px] font-medium text-muted uppercase tracking-wide">Type</span>
            <Select 
              value={typeFilter}
              onChange={(e) => setTypeFilter(e.target.value)}
              className="h-7 text-xs min-w-[100px]"
            >
              <option value="all">All</option>
              {triggerTypes.map(type => (
                <option key={type} value={type}>{type}</option>
              ))}
            </Select>
          </div>

          <div className="w-px h-6 bg-border" />

          <div className="flex items-center gap-4 text-xs">
            <div className="flex items-center gap-1.5 text-muted">
              <Hash className="w-3 h-3" />
              <span className="font-medium text-foreground tabular-nums">{stats.total}</span>
              triggers
            </div>

            <div className="flex items-center gap-1.5 text-muted">
              <Globe className="w-3 h-3 text-cyan-400" />
              <span className="font-medium text-foreground tabular-nums">{stats.api}</span>
              API
            </div>

            <div className="flex items-center gap-1.5 text-muted">
              <Calendar className="w-3 h-3 text-yellow" />
              <span className="font-medium text-foreground tabular-nums">{stats.cron}</span>
              cron
            </div>

            {stats.running > 0 && (
              <div className="flex items-center gap-1.5 text-yellow">
                <Activity className="w-3 h-3" />
                <span className="font-medium tabular-nums">{stats.running}</span>
                running
              </div>
            )}

            {stats.failed > 0 && (
              <div className="flex items-center gap-1.5 text-error">
                <AlertCircle className="w-3 h-3" />
                <span className="font-medium tabular-nums">{stats.failed}</span>
                failed
              </div>
            )}

            {stats.nextScheduled && (
              <div className="flex items-center gap-1.5 text-muted">
                <Timer className="w-3 h-3 text-success" />
                <span className="font-medium text-foreground tabular-nums">
                  {formatRelativeTime(stats.nextScheduled.nextRunAt)}
                </span>
              </div>
            )}
          </div>
        </div>
      </div>

      <div className="flex-1 flex overflow-hidden">
        <div className="flex-1 overflow-y-auto p-4 space-y-2">
          {loading && filteredTriggers.length === 0 ? (
            <div className="flex items-center justify-center h-32">
              <RefreshCw className="w-6 h-6 text-muted animate-spin" />
            </div>
          ) : filteredTriggers.length === 0 ? (
            <div className="flex flex-col items-center justify-center h-64">
              <div className="w-16 h-16 mb-4 rounded-2xl bg-dark-gray border border-border flex items-center justify-center">
                <Zap className="w-8 h-8 text-muted" />
              </div>
              <div className="font-medium mb-1">No triggers found</div>
              <div className="text-xs text-muted text-center max-w-xs">
                {searchQuery || typeFilter !== 'all'
                  ? 'Try adjusting your filters'
                  : 'Register triggers using the SDK to expose your functions'}
              </div>
            </div>
          ) : (
            filteredTriggers.map((trigger) => {
              const Icon = TRIGGER_ICONS[trigger.trigger_type] || Zap;
              const isExpanded = expandedTriggers.has(trigger.id);
              const isSelected = selectedTriggerId === trigger.id;
              const apiEndpoint = getApiEndpoint(trigger);
              const cronSchedule = getCronSchedule(trigger);
              const statusConfig = STATUS_CONFIG[trigger.status || 'idle'];
              const StatusIcon = statusConfig?.icon || Clock;
              
              return (
                <div 
                  key={trigger.id}
                  className={`group relative rounded-lg border transition-all duration-200 cursor-pointer
                    hover:shadow-md hover:border-primary/50
                    ${isSelected
                      ? 'bg-primary/10 border-primary shadow-sm ring-1 ring-primary/30'
                      : 'bg-dark-gray/30 border-border hover:bg-dark-gray/50'
                    }
                    ${trigger.enabled === false ? 'opacity-60' : ''}
                  `}
                  onClick={() => setSelectedTriggerId(isSelected ? null : trigger.id)}
                >
                  <div className="p-4">
                    <div className="flex items-start justify-between gap-3">
                      <div className="flex-1 min-w-0">
                        <div className="flex items-center gap-2">
                          <h3 className="font-medium text-foreground truncate">
                            {trigger.function_path || trigger.id}
                          </h3>
                          <Badge variant="outline" className="shrink-0 gap-1">
                            <Icon className="w-3 h-3" />
                            {trigger.trigger_type.toUpperCase()}
                          </Badge>
                          {trigger.status && (
                            <Badge 
                              variant={trigger.status === 'failed' ? 'error' : trigger.status === 'running' ? 'warning' : 'success'}
                              className="gap-1 shrink-0"
                            >
                              <StatusIcon className={`w-3 h-3 ${trigger.status === 'running' ? 'animate-spin' : ''}`} />
                              {statusConfig?.label}
                            </Badge>
                          )}
                        </div>
                        
                        {apiEndpoint && (
                          <div className="text-xs text-muted font-mono mt-1 flex items-center gap-2">
                            <Globe className="w-3 h-3 text-cyan-400" />
                            <code className="text-cyan-400">{apiEndpoint}</code>
                          </div>
                        )}
                        
                        {cronSchedule && (
                          <div className="mt-2 flex items-center gap-2">
                            <code className="px-2 py-1 rounded bg-black/40 text-xs font-mono text-yellow">
                              {cronSchedule}
                            </code>
                            <span className="text-xs text-muted">{parseCronExpression(cronSchedule)}</span>
                          </div>
                        )}
                      </div>

                      <div className="flex items-center gap-1 shrink-0">
                        {trigger.trigger_type === 'cron' && (
                          <>
                            <Button
                              variant="ghost"
                              size="sm"
                              onClick={(e) => { e.stopPropagation(); }}
                              disabled={trigger.status === 'running'}
                              className="h-7 w-7 p-0 text-muted hover:text-foreground"
                              title="Trigger manually"
                            >
                              <Play className="w-3.5 h-3.5" />
                            </Button>
                            <Button
                              variant="ghost"
                              size="sm"
                              onClick={(e) => { e.stopPropagation(); }}
                              className="h-7 w-7 p-0 text-muted hover:text-foreground"
                              title={trigger.enabled !== false ? 'Disable' : 'Enable'}
                            >
                              {trigger.enabled !== false ? <Pause className="w-3.5 h-3.5" /> : <Play className="w-3.5 h-3.5" />}
                            </Button>
                          </>
                        )}
                        <ChevronRight className={`w-4 h-4 text-muted transition-transform ${isSelected ? 'rotate-90' : ''}`} />
                      </div>
                    </div>

                    {trigger.trigger_type === 'cron' && (
                      <div className="mt-3 flex items-center gap-4 text-xs text-muted">
                        <div className="flex items-center gap-1.5" title="Next run">
                          <Calendar className="w-3 h-3" />
                          <span className="tabular-nums">
                            {trigger.enabled !== false ? formatRelativeTime(trigger.nextRunAt) : 'Disabled'}
                          </span>
                        </div>
                        <div className="flex items-center gap-1.5" title="Last duration">
                          <Timer className="w-3 h-3" />
                          <span className="tabular-nums">{formatDuration(trigger.lastDuration)}</span>
                        </div>
                        <div className="flex items-center gap-1.5" title="Total runs">
                          <Hash className="w-3 h-3" />
                          <span className="tabular-nums">{trigger.runCount || 0}</span>
                        </div>
                        {(trigger.errorCount || 0) > 0 && (
                          <div className="flex items-center gap-1.5 text-error" title="Errors">
                            <AlertCircle className="w-3 h-3" />
                            <span className="tabular-nums">{trigger.errorCount}</span>
                          </div>
                        )}
                      </div>
                    )}

                    {trigger.lastError && trigger.status === 'failed' && (
                      <div className="mt-2 p-2 rounded bg-error/10 border border-error/20">
                        <p className="text-xs text-error line-clamp-2">{trigger.lastError}</p>
                      </div>
                    )}

                    {trigger.worker_id && (
                      <div className="mt-3 flex items-center gap-2">
                        <span className="px-2 py-0.5 rounded-full bg-purple-500/10 text-purple-400 text-[10px] font-medium">
                          Worker: {trigger.worker_id.slice(0, 8)}
                        </span>
                      </div>
                    )}
                  </div>
                </div>
              );
            })
          )}
        </div>

        {selectedTrigger && (
          <div className="w-80 border-l border-border bg-dark-gray/20 overflow-y-auto">
            <div className="p-4 border-b border-border">
              <div className="flex items-center justify-between">
                <h2 className="font-medium text-sm">Trigger Details</h2>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => setSelectedTriggerId(null)}
                  className="h-6 w-6 p-0"
                >
                  <X className="w-3.5 h-3.5" />
                </Button>
              </div>
              <p className="text-xs text-muted mt-1 truncate">{selectedTrigger.function_path}</p>
            </div>

            <div className="p-4 space-y-4">
              <div>
                <div className="text-[10px] text-muted uppercase tracking-wider mb-2">Trigger ID</div>
                <div className="flex items-center gap-2">
                  <code className="text-xs font-mono bg-black/40 px-2 py-1 rounded flex-1 truncate">
                    {selectedTrigger.id}
                  </code>
                  <button 
                    onClick={() => copyToClipboard(selectedTrigger.id, selectedTrigger.id)}
                    className="p-1 hover:bg-dark-gray rounded text-muted hover:text-foreground"
                  >
                    <Copy className="w-3 h-3" />
                  </button>
                </div>
              </div>

              <div>
                <div className="text-[10px] text-muted uppercase tracking-wider mb-2">Type</div>
                <Badge variant="outline" className="gap-1">
                  {(() => { const I = TRIGGER_ICONS[selectedTrigger.trigger_type] || Zap; return <I className="w-3 h-3" />; })()}
                  {selectedTrigger.trigger_type}
                </Badge>
              </div>

              {selectedTrigger.trigger_type === 'api' && (
                <div>
                  <div className="text-[10px] text-muted uppercase tracking-wider mb-2">Endpoint</div>
                  <div className="flex items-center gap-2">
                    <code className="text-xs font-mono bg-cyan-500/10 text-cyan-400 px-2 py-1 rounded border border-cyan-500/30">
                      {getApiEndpoint(selectedTrigger)}
                    </code>
                    <a 
                      href={`http://localhost:3111/${(selectedTrigger.config as { api_path?: string }).api_path || ''}`}
                      target="_blank"
                      rel="noopener noreferrer"
                      className="p-1 hover:bg-dark-gray rounded text-muted hover:text-foreground"
                    >
                      <ExternalLink className="w-3 h-3" />
                    </a>
                  </div>
                </div>
              )}

              <div>
                <div className="text-[10px] text-muted uppercase tracking-wider mb-2">Configuration</div>
                <pre className="text-[10px] font-mono bg-black/40 px-3 py-2 rounded overflow-x-auto text-muted max-h-48">
                  {JSON.stringify(selectedTrigger.config, null, 2)}
                </pre>
              </div>

              {selectedTrigger.worker_id && (
                <div>
                  <div className="text-[10px] text-muted uppercase tracking-wider mb-2">Worker</div>
                  <code className="text-xs font-mono">{selectedTrigger.worker_id}</code>
                </div>
              )}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

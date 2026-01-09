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

// Consistent color scheme for trigger types
const TRIGGER_TYPE_CONFIG: Record<string, { icon: typeof Zap; color: string; bg: string; label: string }> = {
  'api': { icon: Globe, color: 'text-cyan-400', bg: 'bg-cyan-500/10 border-cyan-500/30', label: 'API' },
  'cron': { icon: Calendar, color: 'text-orange-400', bg: 'bg-orange-500/10 border-orange-500/30', label: 'CRON' },
  'event': { icon: MessageSquare, color: 'text-purple-400', bg: 'bg-purple-500/10 border-purple-500/30', label: 'EVENT' },
  'streams:join': { icon: Play, color: 'text-green-400', bg: 'bg-green-500/10 border-green-500/30', label: 'STREAM JOIN' },
  'streams:leave': { icon: Pause, color: 'text-green-400', bg: 'bg-green-500/10 border-green-500/30', label: 'STREAM LEAVE' },
};

const TRIGGER_ICONS: Record<string, typeof Zap> = {
  'api': Globe,
  'cron': Calendar,
  'event': MessageSquare,
  'streams:join': Play,
  'streams:leave': Pause,
};

const STATUS_CONFIG: Record<string, { color: string; icon: typeof Clock; label: string }> = {
  idle: { color: 'text-muted', icon: Clock, label: 'Idle' },
  running: { color: 'text-orange-400', icon: Loader2, label: 'Running' },
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
  const [selectedTriggerId, setSelectedTriggerId] = useState<string | null>(null);
  const [isConnected] = useState(true);
  
  // API Testing state
  const [pathParams, setPathParams] = useState<Record<string, string>>({});
  const [queryParams, setQueryParams] = useState<Record<string, string>>({});
  const [requestBody, setRequestBody] = useState('{}');
  const [httpMethod, setHttpMethod] = useState('GET');
  const [invoking, setInvoking] = useState(false);
  const [invocationResult, setInvocationResult] = useState<{
    success: boolean;
    status?: number;
    duration?: number;
    data?: unknown;
    error?: string;
  } | null>(null);

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

  const handleSelectTrigger = (trigger: Trigger) => {
    if (selectedTriggerId === trigger.id) {
      setSelectedTriggerId(null);
      return;
    }
    
    setSelectedTriggerId(trigger.id);
    setInvocationResult(null);
    
    if (trigger.trigger_type === 'api') {
      const config = trigger.config as { api_path?: string; http_method?: string };
      const path = config.api_path || '';
      const method = config.http_method || 'GET';
      setHttpMethod(method);
      
      // Extract path parameters
      const matches = path.match(/:([a-zA-Z_]+)/g);
      if (matches) {
        const params: Record<string, string> = {};
        matches.forEach(m => params[m.slice(1)] = '');
        setPathParams(params);
      } else {
        setPathParams({});
      }
      
      setQueryParams({});
      setRequestBody(method === 'POST' || method === 'PUT' ? '{\n  \n}' : '{}');
    }
  };

  const invokeTrigger = async (trigger: Trigger) => {
    if (trigger.trigger_type !== 'api') return;
    
    setInvoking(true);
    setInvocationResult(null);
    const startTime = Date.now();

    try {
      const config = trigger.config as { api_path?: string; http_method?: string };
      let path = config.api_path || '';
      const method = httpMethod;
      
      // Replace path parameters
      const pathParamMatches = path.match(/:([a-zA-Z_]+)/g);
      if (pathParamMatches) {
        for (const match of pathParamMatches) {
          const paramName = match.slice(1);
          const value = pathParams[paramName];
          if (!value) {
            setInvocationResult({ success: false, error: `Missing path parameter: ${paramName}` });
            setInvoking(false);
            return;
          }
          path = path.replace(match, encodeURIComponent(value));
        }
      }
      
      // Build query string
      const queryEntries = Object.entries(queryParams).filter(([_, v]) => v);
      const queryString = queryEntries.length > 0
        ? '?' + queryEntries.map(([k, v]) => `${encodeURIComponent(k)}=${encodeURIComponent(v)}`).join('&')
        : '';
      
      const fetchOptions: RequestInit = {
        method,
        headers: { 'Content-Type': 'application/json' },
      };

      if (method !== 'GET' && method !== 'HEAD') {
        try {
          JSON.parse(requestBody);
          fetchOptions.body = requestBody;
        } catch {
          setInvocationResult({ success: false, error: 'Invalid JSON in request body' });
          setInvoking(false);
          return;
        }
      }

      const fullUrl = `http://localhost:3111/${path}${queryString}`;
      const response = await fetch(fullUrl, fetchOptions);
      const duration = Date.now() - startTime;
      
      let data;
      const contentType = response.headers.get('content-type');
      if (contentType?.includes('application/json')) {
        data = await response.json();
      } else {
        data = await response.text();
      }

      setInvocationResult({ success: response.ok, status: response.status, duration, data });
    } catch (err) {
      setInvocationResult({
        success: false,
        duration: Date.now() - startTime,
        error: err instanceof Error ? err.message : 'Invocation failed'
      });
    } finally {
      setInvoking(false);
    }
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
          <Button 
            variant={showSystem ? "accent" : "ghost"} 
            size="sm" 
            onClick={() => setShowSystem(!showSystem)}
            className="h-7 text-xs"
          >
            {showSystem ? <Eye className="w-3 h-3 mr-1.5" /> : <EyeOff className="w-3 h-3 mr-1.5" />}
            <span className={showSystem ? '' : 'line-through opacity-60'}>System</span>
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

      {/* Search Bar Row - Consistent with Logs/Traces */}
      <div className="flex items-center gap-2 p-2 border-b border-border bg-dark-gray/20">
        <div className="flex-1 relative">
          <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-muted" />
          <Input
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            className="pl-9 pr-9 h-9"
            placeholder="Search..."
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
        
        <div className="flex items-center gap-3 px-2 border-l border-border">
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

        <div className="flex items-center gap-4 px-2 text-xs border-l border-border">
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
            <Calendar className="w-3 h-3 text-orange-400" />
            <span className="font-medium text-foreground tabular-nums">{stats.cron}</span>
            cron
          </div>

          {stats.running > 0 && (
            <div className="flex items-center gap-1.5 text-orange-400">
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
                  onClick={() => handleSelectTrigger(trigger)}
                >
                  <div className="p-4">
                    <div className="flex items-start justify-between gap-3">
                      <div className="flex-1 min-w-0">
                        <div className="flex items-center gap-2">
                          <h3 className="font-medium text-foreground truncate">
                            {trigger.function_path || trigger.id}
                          </h3>
                          <Badge 
                            variant="outline" 
                            className={`shrink-0 gap-1 border ${TRIGGER_TYPE_CONFIG[trigger.trigger_type]?.bg || 'bg-muted/10 border-border'}`}
                          >
                            <Icon className={`w-3 h-3 ${TRIGGER_TYPE_CONFIG[trigger.trigger_type]?.color || 'text-muted'}`} />
                            <span className={TRIGGER_TYPE_CONFIG[trigger.trigger_type]?.color || 'text-muted'}>
                              {TRIGGER_TYPE_CONFIG[trigger.trigger_type]?.label || trigger.trigger_type.toUpperCase()}
                            </span>
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
                            <code className="px-2 py-1 rounded bg-orange-500/10 border border-orange-500/20 text-xs font-mono text-orange-400">
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
          <div className="w-[480px] shrink-0 border-l border-border bg-dark-gray/20 overflow-y-auto">
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
                <>
                  <div>
                    <div className="text-[10px] text-muted uppercase tracking-wider mb-2">Endpoint</div>
                    <div className="flex items-center gap-2">
                      <code className="text-xs font-mono bg-cyan-500/10 text-cyan-400 px-2 py-1 rounded border border-cyan-500/30 flex-1 truncate">
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

                  {/* API Testing Section */}
                  <div className="border-t border-border pt-4">
                    <div className="text-[10px] text-muted uppercase tracking-wider mb-3 flex items-center gap-2">
                      <Play className="w-3 h-3" />
                      Test API
                    </div>
                    
                    <div className="space-y-3">
                      <div className="flex items-center gap-2">
                        <Select
                          value={httpMethod}
                          onChange={(e) => setHttpMethod(e.target.value)}
                          className="w-24 h-8 text-xs"
                        >
                          <option value="GET">GET</option>
                          <option value="POST">POST</option>
                          <option value="PUT">PUT</option>
                          <option value="PATCH">PATCH</option>
                          <option value="DELETE">DELETE</option>
                        </Select>
                        <code className="flex-1 text-xs font-mono text-muted bg-black/30 px-2 py-1.5 rounded truncate">
                          /{(selectedTrigger.config as { api_path?: string }).api_path || ''}
                        </code>
                      </div>

                      {/* Path Parameters */}
                      {Object.keys(pathParams).length > 0 && (
                        <div className="space-y-2">
                          <div className="text-[10px] text-muted uppercase tracking-wider">Path Parameters</div>
                          {Object.keys(pathParams).map(param => (
                            <div key={param} className="flex items-center gap-2">
                              <label className="text-xs font-mono text-orange-400 w-16 shrink-0">:{param}</label>
                              <Input
                                value={pathParams[param]}
                                onChange={(e) => setPathParams(prev => ({ ...prev, [param]: e.target.value }))}
                                placeholder={`Enter ${param}`}
                                className="h-7 text-xs font-mono"
                              />
                            </div>
                          ))}
                        </div>
                      )}

                      {/* Query Parameters */}
                      {httpMethod === 'GET' && (
                        <div className="space-y-2">
                          <div className="flex items-center justify-between">
                            <div className="text-[10px] text-muted uppercase tracking-wider">Query Parameters</div>
                            <button
                              onClick={() => setQueryParams(prev => ({ ...prev, [`param${Object.keys(prev).length + 1}`]: '' }))}
                              className="text-[10px] text-cyan-400 hover:text-cyan-300"
                            >
                              + Add
                            </button>
                          </div>
                          {Object.keys(queryParams).length === 0 ? (
                            <div className="text-[10px] text-muted italic">No query parameters</div>
                          ) : (
                            Object.entries(queryParams).map(([key, value], idx) => (
                              <div key={idx} className="flex items-center gap-2">
                                <Input
                                  value={key}
                                  onChange={(e) => {
                                    const newParams = { ...queryParams };
                                    delete newParams[key];
                                    newParams[e.target.value] = value;
                                    setQueryParams(newParams);
                                  }}
                                  placeholder="key"
                                  className="h-7 text-xs font-mono w-20"
                                />
                                <span className="text-muted">=</span>
                                <Input
                                  value={value}
                                  onChange={(e) => setQueryParams(prev => ({ ...prev, [key]: e.target.value }))}
                                  placeholder="value"
                                  className="h-7 text-xs font-mono flex-1"
                                />
                                <button
                                  onClick={() => {
                                    const newParams = { ...queryParams };
                                    delete newParams[key];
                                    setQueryParams(newParams);
                                  }}
                                  className="p-1 text-muted hover:text-error"
                                >
                                  <X className="w-3 h-3" />
                                </button>
                              </div>
                            ))
                          )}
                        </div>
                      )}

                      {/* Request Body */}
                      {(httpMethod === 'POST' || httpMethod === 'PUT' || httpMethod === 'PATCH') && (
                        <div>
                          <div className="text-[10px] text-muted uppercase tracking-wider mb-1.5">Request Body</div>
                          <textarea
                            value={requestBody}
                            onChange={(e) => setRequestBody(e.target.value)}
                            className="w-full h-20 text-xs font-mono bg-black/40 text-foreground px-3 py-2 rounded border border-border focus:border-primary focus:outline-none resize-none"
                            placeholder='{"key": "value"}'
                          />
                        </div>
                      )}

                      <Button
                        onClick={() => invokeTrigger(selectedTrigger)}
                        disabled={invoking}
                        className="w-full h-8"
                      >
                        {invoking ? (
                          <>
                            <Loader2 className="w-3.5 h-3.5 mr-2 animate-spin" />
                            Sending...
                          </>
                        ) : (
                          <>
                            <Play className="w-3.5 h-3.5 mr-2" />
                            Send Request
                          </>
                        )}
                      </Button>
                    </div>
                  </div>

                  {/* Response */}
                  {invocationResult && (
                    <div className={`border rounded-lg overflow-hidden ${
                      invocationResult.success 
                        ? 'border-success/30 bg-success/5' 
                        : 'border-error/30 bg-error/5'
                    }`}>
                      <div className={`flex items-center justify-between px-3 py-2 border-b ${
                        invocationResult.success ? 'border-success/20' : 'border-error/20'
                      }`}>
                        <div className="flex items-center gap-2">
                          {invocationResult.success ? (
                            <CheckCircle className="w-3.5 h-3.5 text-success" />
                          ) : (
                            <AlertCircle className="w-3.5 h-3.5 text-error" />
                          )}
                          <span className={`text-xs font-medium ${
                            invocationResult.success ? 'text-success' : 'text-error'
                          }`}>
                            {invocationResult.status || 'Error'}
                          </span>
                        </div>
                        {invocationResult.duration && (
                          <span className="text-[10px] text-muted">{invocationResult.duration}ms</span>
                        )}
                      </div>
                      <div className="p-3 max-h-48 overflow-auto">
                        {invocationResult.error ? (
                          <p className="text-xs text-error">{invocationResult.error}</p>
                        ) : (
                          <pre className="text-[10px] font-mono text-foreground whitespace-pre-wrap">
                            {typeof invocationResult.data === 'string' 
                              ? invocationResult.data 
                              : JSON.stringify(invocationResult.data, null, 2)}
                          </pre>
                        )}
                      </div>
                    </div>
                  )}
                </>
              )}

              <div>
                <div className="text-[10px] text-muted uppercase tracking-wider mb-2">Configuration</div>
                <pre className="text-[10px] font-mono bg-black/40 px-3 py-2 rounded overflow-x-auto text-muted max-h-32">
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

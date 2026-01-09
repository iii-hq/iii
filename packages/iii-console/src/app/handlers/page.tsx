'use client';

import { useEffect, useState, useMemo } from 'react';
import { Card, CardContent, CardHeader, CardTitle, Badge, Table, TableHeader, TableBody, TableRow, TableHead, TableCell, Button, Input, Select } from "@/components/ui/card";
import { 
  Server, Search, RefreshCw, Activity, Clock, CheckCircle, XCircle, Code2, Eye, EyeOff,
  Play, Terminal, ExternalLink, Copy, Check, ChevronRight, X, Zap, Globe, Calendar,
  MessageSquare, Send, Loader2, AlertCircle, ArrowRight
} from "lucide-react";
import { fetchFunctions, fetchWorkers, fetchTriggers, FunctionInfo, WorkerInfo, TriggerInfo, emitEvent, triggerCron } from "@/lib/api";
import { JsonViewer } from "@/components/ui/json-viewer";

interface InvocationResult {
  success: boolean;
  status?: number;
  duration?: number;
  data?: unknown;
  error?: string;
}

export default function HandlersPage() {
  const [functions, setFunctions] = useState<FunctionInfo[]>([]);
  const [workers, setWorkers] = useState<WorkerInfo[]>([]);
  const [triggers, setTriggers] = useState<TriggerInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [searchQuery, setSearchQuery] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [showSystem, setShowSystem] = useState(false);
  const [selectedFunction, setSelectedFunction] = useState<FunctionInfo | null>(null);
  const [copied, setCopied] = useState<string | null>(null);
  const [invoking, setInvoking] = useState(false);
  const [invocationResult, setInvocationResult] = useState<InvocationResult | null>(null);
  const [requestBody, setRequestBody] = useState('{}');
  const [httpMethod, setHttpMethod] = useState('GET');
  const [pathParams, setPathParams] = useState<Record<string, string>>({});
  const [queryParams, setQueryParams] = useState<Record<string, string>>({});
  const [eventPayload, setEventPayload] = useState('{"test": true}');
  const [triggerResult, setTriggerResult] = useState<{ success: boolean; message: string } | null>(null);
  const [triggering, setTriggering] = useState(false);

  const loadData = async () => {
    setLoading(true);
    setError(null);
    try {
      const [functionsData, workersData, triggersData] = await Promise.all([
        fetchFunctions(),
        fetchWorkers(),
        fetchTriggers()
      ]);
      setFunctions(functionsData.functions);
      setWorkers(workersData.workers);
      setTriggers(triggersData.triggers || []);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load data');
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    loadData();
  }, []);

  const userFunctions = functions.filter(f => !f.internal);
  const systemFunctions = functions.filter(f => f.internal);

  const filteredFunctions = functions.filter(f => {
    if (!showSystem && f.internal) return false;
    if (searchQuery && !f.path.toLowerCase().includes(searchQuery.toLowerCase())) return false;
    return true;
  });

  const groupedFunctions = filteredFunctions.reduce((acc, fn) => {
    const parts = fn.path.split('.');
    const group = parts.length > 1 ? parts[0].toUpperCase() : 'CORE';
    if (!acc[group]) acc[group] = [];
    acc[group].push(fn);
    return acc;
  }, {} as Record<string, FunctionInfo[]>);

  const groups = Object.keys(groupedFunctions).sort((a, b) => {
    if (a === 'API') return -1;
    if (b === 'API') return 1;
    if (a === 'CRON') return -1;
    if (b === 'CRON') return 1;
    return a.localeCompare(b);
  });

  const getAssociatedTrigger = (fn: FunctionInfo): TriggerInfo | undefined => {
    return triggers.find(t => t.function_path === fn.path);
  };

  const getApiPath = (fn: FunctionInfo): string | null => {
    const trigger = getAssociatedTrigger(fn);
    if (!trigger || trigger.trigger_type !== 'api') return null;
    const config = trigger.config as { api_path?: string };
    return config.api_path || null;
  };

  const getHttpMethod = (fn: FunctionInfo): string => {
    const trigger = getAssociatedTrigger(fn);
    if (!trigger) return 'GET';
    const config = trigger.config as { http_method?: string };
    return config.http_method || 'GET';
  };

  // Parse cron expression to human-readable format
  const parseCronExpression = (expression: string): { readable: string; nextRun: string } => {
    if (!expression) return { readable: 'No schedule', nextRun: 'Unknown' };
    
    const parts = expression.split(' ');
    if (parts.length < 5) return { readable: expression, nextRun: 'Unknown' };
    
    const [minute, hour, dayOfMonth, month, dayOfWeek] = parts;
    
    let readable = '';
    
    // Common patterns
    if (minute === '*' && hour === '*') {
      readable = 'Every minute';
    } else if (minute.startsWith('*/')) {
      const interval = minute.slice(2);
      readable = `Every ${interval} minutes`;
    } else if (hour.startsWith('*/')) {
      const interval = hour.slice(2);
      readable = `Every ${interval} hours`;
    } else if (minute === '0' && hour === '0' && dayOfMonth === '*' && month === '*' && dayOfWeek === '*') {
      readable = 'Daily at midnight';
    } else if (minute === '0' && hour !== '*' && dayOfMonth === '*' && month === '*' && dayOfWeek === '*') {
      readable = `Daily at ${hour}:00`;
    } else if (minute !== '*' && hour !== '*' && dayOfMonth === '*' && month === '*' && dayOfWeek === '*') {
      readable = `Daily at ${hour}:${minute.padStart(2, '0')}`;
    } else if (dayOfWeek === '0') {
      readable = `Weekly on Sunday at ${hour}:${minute.padStart(2, '0')}`;
    } else if (dayOfWeek === '1-5' || dayOfWeek === 'MON-FRI') {
      readable = `Weekdays at ${hour}:${minute.padStart(2, '0')}`;
    } else {
      readable = expression;
    }
    
    // Calculate approximate next run
    const now = new Date();
    let nextRun = 'Soon';
    
    if (minute.startsWith('*/')) {
      const interval = parseInt(minute.slice(2));
      const nextMinute = Math.ceil(now.getMinutes() / interval) * interval;
      if (nextMinute >= 60) {
        nextRun = `In ~${interval} min`;
      } else {
        nextRun = `In ~${nextMinute - now.getMinutes()} min`;
      }
    } else if (hour === '0' && minute === '0') {
      nextRun = 'At midnight';
    }
    
    return { readable, nextRun };
  };

  const copyToClipboard = (text: string, key: string) => {
    navigator.clipboard.writeText(text);
    setCopied(key);
    setTimeout(() => setCopied(null), 2000);
  };

  const invokeFunction = async (fn: FunctionInfo) => {
    const trigger = getAssociatedTrigger(fn);
    if (!trigger) return;

    setInvoking(true);
    setInvocationResult(null);
    const startTime = Date.now();

    try {
      if (trigger.trigger_type === 'api') {
        const config = trigger.config as { api_path?: string; http_method?: string };
        let path = config.api_path || '';
        const method = httpMethod || config.http_method || 'GET';
        
        // Replace path parameters like :id with actual values
        const pathParamMatches = path.match(/:([a-zA-Z_]+)/g);
        if (pathParamMatches) {
          for (const match of pathParamMatches) {
            const paramName = match.slice(1); // Remove the :
            const value = pathParams[paramName];
            if (!value) {
              setInvocationResult({
                success: false,
                error: `Missing path parameter: ${paramName}`
              });
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
            setInvocationResult({
              success: false,
              error: 'Invalid JSON in request body'
            });
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

        setInvocationResult({
          success: response.ok,
          status: response.status,
          duration,
          data
        });
      }
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

  const handleSelectFunction = (fn: FunctionInfo) => {
    if (selectedFunction?.path === fn.path) {
      setSelectedFunction(null);
    } else {
      setSelectedFunction(fn);
      setInvocationResult(null);
      setTriggerResult(null);
      setEventPayload('{"test": true}');
      const method = getHttpMethod(fn);
      setHttpMethod(method);
      
      // Extract path parameters from the API path
      const trigger = getAssociatedTrigger(fn);
      if (trigger?.trigger_type === 'api') {
        const config = trigger.config as { api_path?: string };
        const path = config.api_path || '';
        const matches = path.match(/:([a-zA-Z_]+)/g);
        if (matches) {
          const params: Record<string, string> = {};
          matches.forEach(m => params[m.slice(1)] = '');
          setPathParams(params);
        } else {
          setPathParams({});
        }
      }
      
      setQueryParams({});
      
      if (method === 'POST' || method === 'PUT') {
        setRequestBody('{\n  \n}');
      } else {
        setRequestBody('{}');
      }
    }
  };

  const getFunctionIcon = (fn: FunctionInfo) => {
    const trigger = getAssociatedTrigger(fn);
    if (!trigger) return <Code2 className="w-4 h-4 text-muted" />;
    switch (trigger.trigger_type) {
      case 'api': return <Globe className="w-4 h-4 text-cyan-400" />;
      case 'cron': return <Calendar className="w-4 h-4 text-orange-400" />;
      case 'event': return <MessageSquare className="w-4 h-4 text-purple-400" />;
      case 'streams:join': case 'streams:leave': return <Zap className="w-4 h-4 text-success" />;
      default: return <Zap className="w-4 h-4 text-muted" />;
    }
  };

  return (
    <div className="flex flex-col h-full bg-background text-foreground">
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-2 px-3 md:px-5 py-2 md:py-3 bg-dark-gray/30 border-b border-border">
        <div className="flex items-center gap-2 md:gap-4 flex-wrap">
          <h1 className="text-sm md:text-base font-semibold flex items-center gap-2">
            <Server className="w-4 h-4" />
            <span className="hidden sm:inline">Functions & Triggers</span>
            <span className="sm:hidden">Functions</span>
          </h1>
          <Badge variant="success" className="gap-1 text-[10px] md:text-xs">
            <Activity className="w-2.5 h-2.5 md:w-3 md:h-3" />
            {userFunctions.length}
          </Badge>
          {systemFunctions.length > 0 && !showSystem && (
            <span className="text-[10px] md:text-xs text-muted hidden md:inline">({systemFunctions.length} system)</span>
          )}
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
            onClick={loadData} 
            disabled={loading}
            className="h-7 text-xs"
          >
            <RefreshCw className={`w-3.5 h-3.5 mr-1.5 ${loading ? 'animate-spin' : ''}`} />
            Refresh
          </Button>
        </div>
      </div>

      {error && (
        <div className="mx-4 mt-4 bg-error/10 border border-error/30 rounded px-4 py-3 text-sm text-error flex items-center gap-2">
          <AlertCircle className="w-4 h-4" />
          {error}
        </div>
      )}

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
        
        <div className="flex items-center gap-4 px-2 text-xs border-l border-border">
          <div className="flex items-center gap-1.5 text-muted">
            <Code2 className="w-3 h-3 text-cyan-400" />
            <span className="font-medium text-foreground tabular-nums">{userFunctions.length}</span>
            functions
          </div>
          <div className="flex items-center gap-1.5 text-muted">
            <Server className="w-3 h-3 text-purple-400" />
            <span className="font-medium text-foreground tabular-nums">{workers.length}</span>
            workers
          </div>
          <div className="flex items-center gap-1.5 text-muted">
            <Zap className="w-3 h-3 text-yellow" />
            <span className="font-medium text-foreground tabular-nums">{triggers.filter(t => !t.internal).length}</span>
            triggers
          </div>
        </div>
      </div>

      <div className={`flex-1 flex overflow-hidden ${selectedFunction ? 'divide-x divide-border' : ''}`}>
        <div className="flex-1 overflow-y-auto p-5 space-y-6">
          {loading ? (
            <div className="flex items-center justify-center py-12 text-muted">
              <RefreshCw className="w-5 h-5 animate-spin mr-2" />
              Loading functions...
            </div>
          ) : filteredFunctions.length === 0 ? (
            <div className="flex flex-col items-center justify-center py-12">
              <Code2 className="w-12 h-12 text-muted/30 mb-4" />
              <div className="text-sm font-medium mb-1">No functions found</div>
              <div className="text-xs text-muted">
                {searchQuery ? 'Try a different search term' : 'Register functions using the SDK'}
              </div>
            </div>
          ) : (
            groups.map(group => (
              <div key={group}>
                <div className="flex items-center gap-2 mb-3">
                  <Badge variant="outline" className="text-[10px] uppercase tracking-wider">{group}</Badge>
                  <span className="text-[10px] text-muted">{groupedFunctions[group].length} functions</span>
                </div>
                <div className="space-y-1">
                  {groupedFunctions[group].map((fn) => {
                    const trigger = getAssociatedTrigger(fn);
                    const apiPath = getApiPath(fn);
                    const isSelected = selectedFunction?.path === fn.path;
                    
                    return (
                      <div
                        key={fn.path}
                        onClick={() => handleSelectFunction(fn)}
                        className={`group flex items-center gap-3 px-3 py-2.5 rounded-lg cursor-pointer transition-all
                          ${isSelected 
                            ? 'bg-primary/10 border border-primary/30 ring-1 ring-primary/20' 
                            : 'bg-dark-gray/30 border border-transparent hover:bg-dark-gray/50 hover:border-border'
                          }
                        `}
                      >
                        <div className="shrink-0">
                          {getFunctionIcon(fn)}
                        </div>
                        
                        <div className="flex-1 min-w-0">
                          <div className="flex items-center gap-2">
                            <span className={`font-mono text-sm font-medium ${isSelected ? 'text-primary' : 'text-yellow'}`}>
                              {fn.path}
                            </span>
                            {trigger && (
                              <Badge 
                                variant="outline" 
                                className={`text-[9px] uppercase ${
                                  trigger.trigger_type === 'api' ? 'bg-cyan-500/10 text-cyan-400 border-cyan-500/30' :
                                  trigger.trigger_type === 'cron' ? 'bg-orange-500/10 text-orange-400 border-orange-500/30' :
                                  trigger.trigger_type === 'event' ? 'bg-purple-500/10 text-purple-400 border-purple-500/30' :
                                  ''
                                }`}
                              >
                                {trigger.trigger_type}
                              </Badge>
                            )}
                          </div>
                          {/* Show trigger-specific info */}
                          {trigger?.trigger_type === 'api' && apiPath && (
                            <div className="text-xs text-cyan-400/70 font-mono mt-0.5">
                              {getHttpMethod(fn)} /{apiPath}
                            </div>
                          )}
                          {trigger?.trigger_type === 'cron' && (() => {
                            const expr = (trigger.config as { expression?: string }).expression || '';
                            const { readable } = parseCronExpression(expr);
                            return (
                              <div className="text-xs text-orange-400/70 font-mono mt-0.5 flex items-center gap-1.5">
                                <Clock className="w-3 h-3" />
                                {readable}
                              </div>
                            );
                          })()}
                          {trigger?.trigger_type === 'event' && (
                            <div className="text-xs text-purple-400/70 font-mono mt-0.5 flex items-center gap-1.5">
                              <MessageSquare className="w-3 h-3" />
                              {(trigger.config as { topic?: string }).topic || 'No topic'}
                            </div>
                          )}
                        </div>

                        <div className="flex items-center gap-2 shrink-0">
                          <div className="w-2 h-2 rounded-full bg-success" />
                          {trigger?.trigger_type === 'api' && (
                            <Button
                              variant="ghost"
                              size="sm"
                              onClick={(e) => {
                                e.stopPropagation();
                                window.open(`http://localhost:3111/${apiPath}`, '_blank');
                              }}
                              className="h-6 w-6 p-0 opacity-0 group-hover:opacity-100"
                              title="Open in browser"
                            >
                              <ExternalLink className="w-3 h-3" />
                            </Button>
                          )}
                          <ChevronRight className={`w-4 h-4 text-muted transition-transform ${isSelected ? 'rotate-90' : ''}`} />
                        </div>
                      </div>
                    );
                  })}
                </div>
              </div>
            ))
          )}
        </div>

        {selectedFunction && (
          <div className="
            fixed inset-0 z-50 md:relative md:inset-auto
            w-full md:w-[360px] lg:w-[480px] shrink-0 
            flex flex-col h-full overflow-hidden bg-background md:bg-dark-gray/20 border-l border-border
          ">
            <div className="flex items-center justify-between px-3 md:px-4 py-2 md:py-3 border-b border-border bg-dark-gray/30">
              <div className="flex items-center gap-2 min-w-0">
                {getFunctionIcon(selectedFunction)}
                <h2 className="font-medium text-xs md:text-sm truncate">{selectedFunction.path}</h2>
              </div>
              <Button
                variant="ghost"
                size="sm"
                onClick={() => setSelectedFunction(null)}
                className="h-7 w-7 md:h-6 md:w-6 p-0 shrink-0"
              >
                <X className="w-4 h-4" />
              </Button>
            </div>

            <div className="flex-1 overflow-y-auto p-4 space-y-5">
              {(() => {
                const trigger = getAssociatedTrigger(selectedFunction);
                const apiPath = getApiPath(selectedFunction);

                return (
                  <>
                    {trigger && (
                      <div>
                        <div className="text-[10px] text-muted uppercase tracking-wider mb-2">Trigger Type</div>
                        <Badge variant="outline" className="gap-1.5">
                          {getFunctionIcon(selectedFunction)}
                          {trigger.trigger_type}
                        </Badge>
                      </div>
                    )}

                    {trigger?.trigger_type === 'api' && apiPath && (
                      <>
                        <div>
                          <div className="text-[10px] text-muted uppercase tracking-wider mb-2">API Endpoint</div>
                          <div className="flex items-center gap-2">
                            <code className="flex-1 text-xs font-mono bg-black/40 text-cyan-400 px-3 py-2 rounded border border-cyan-500/20">
                              {getHttpMethod(selectedFunction)} http://localhost:3111/{apiPath}
                            </code>
                            <button
                              onClick={() => copyToClipboard(`http://localhost:3111/${apiPath}`, 'endpoint')}
                              className="p-1.5 hover:bg-dark-gray rounded transition-colors"
                            >
                              {copied === 'endpoint' ? (
                                <Check className="w-3.5 h-3.5 text-success" />
                              ) : (
                                <Copy className="w-3.5 h-3.5 text-muted" />
                              )}
                            </button>
                          </div>
                        </div>

                        <div className="border-t border-border pt-4">
                          <div className="text-[10px] text-muted uppercase tracking-wider mb-3 flex items-center gap-2">
                            <Terminal className="w-3 h-3" />
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
                                /{apiPath}
                              </code>
                            </div>

                            {/* Path Parameters */}
                            {Object.keys(pathParams).length > 0 && (
                              <div className="space-y-2">
                                <div className="text-[10px] text-muted uppercase tracking-wider">Path Parameters</div>
                                {Object.keys(pathParams).map(param => (
                                  <div key={param} className="flex items-center gap-2">
                                    <label className="text-xs font-mono text-orange-400 w-20 shrink-0">:{param}</label>
                                    <Input
                                      value={pathParams[param]}
                                      onChange={(e) => setPathParams(prev => ({ ...prev, [param]: e.target.value }))}
                                      placeholder={`Enter ${param}`}
                                      className="h-8 text-xs font-mono"
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
                                        className="h-7 text-xs font-mono w-24"
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
                                <div className="text-[10px] text-muted uppercase tracking-wider mb-1.5">Request Body (JSON)</div>
                                <textarea
                                  value={requestBody}
                                  onChange={(e) => setRequestBody(e.target.value)}
                                  className="w-full h-24 text-xs font-mono bg-black/40 text-foreground px-3 py-2 rounded border border-border focus:border-primary focus:outline-none resize-none"
                                  placeholder='{"key": "value"}'
                                />
                              </div>
                            )}

                            <Button
                              onClick={() => invokeFunction(selectedFunction)}
                              disabled={invoking}
                              className="w-full h-9"
                            >
                              {invoking ? (
                                <>
                                  <Loader2 className="w-3.5 h-3.5 mr-2 animate-spin" />
                                  Sending...
                                </>
                              ) : (
                                <>
                                  <Send className="w-3.5 h-3.5 mr-2" />
                                  Send Request
                                </>
                              )}
                            </Button>
                          </div>
                        </div>

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
                                  <XCircle className="w-3.5 h-3.5 text-error" />
                                )}
                                <span className={`text-xs font-medium ${
                                  invocationResult.success ? 'text-success' : 'text-error'
                                }`}>
                                  {invocationResult.status || (invocationResult.success ? 'Success' : 'Error')}
                                </span>
                              </div>
                              {invocationResult.duration && (
                                <span className="text-[10px] text-muted">
                                  {invocationResult.duration}ms
                                </span>
                              )}
                            </div>
                            <div className="p-3 overflow-x-auto max-h-48 overflow-y-auto">
                              {invocationResult.error ? (
                                <pre className="text-[11px] font-mono text-error">{invocationResult.error}</pre>
                              ) : (
                                <JsonViewer data={invocationResult.data} collapsed={false} maxDepth={4} />
                              )}
                            </div>
                          </div>
                        )}
                      </>
                    )}

                    {trigger?.trigger_type === 'cron' && (() => {
                      const config = trigger.config as { expression?: string; description?: string };
                      const expression = config.expression || '';
                      const { readable, nextRun } = parseCronExpression(expression);
                      
                      return (
                        <>
                          <div>
                            <div className="text-[10px] text-muted uppercase tracking-wider mb-2">Schedule</div>
                            <div className="bg-orange-500/10 border border-orange-500/30 rounded-lg p-3 space-y-2">
                              <div className="flex items-center gap-2">
                                <Calendar className="w-4 h-4 text-orange-400" />
                                <span className="text-sm font-medium text-orange-400">{readable}</span>
                              </div>
                              <code className="text-xs font-mono text-orange-400/70 block">
                                {expression}
                              </code>
                            </div>
                          </div>

                          <div className="grid grid-cols-2 gap-3">
                            <div className="bg-dark-gray/50 rounded-lg p-3">
                              <div className="text-[10px] text-muted uppercase tracking-wider mb-1">Next Run</div>
                              <div className="flex items-center gap-1.5 text-sm">
                                <Clock className="w-3.5 h-3.5 text-orange-400" />
                                <span className="text-foreground">{nextRun}</span>
                              </div>
                            </div>
                            <div className="bg-dark-gray/50 rounded-lg p-3">
                              <div className="text-[10px] text-muted uppercase tracking-wider mb-1">Status</div>
                              <div className="flex items-center gap-1.5 text-sm">
                                <Activity className="w-3.5 h-3.5 text-success" />
                                <span className="text-success">Active</span>
                              </div>
                            </div>
                          </div>

                          {config.description && (
                            <div>
                              <div className="text-[10px] text-muted uppercase tracking-wider mb-2">Description</div>
                              <p className="text-xs text-muted">{config.description}</p>
                            </div>
                          )}

                          <div className="border-t border-border pt-4">
                            <div className="text-[10px] text-muted uppercase tracking-wider mb-3 flex items-center gap-2">
                              <Play className="w-3 h-3" />
                              Manual Trigger
                            </div>
                            <Button
                              variant="outline"
                              className="w-full h-9 border-orange-500/30 text-orange-400 hover:bg-orange-500/10"
                              disabled={triggering}
                              onClick={async () => {
                                setTriggering(true);
                                setTriggerResult(null);
                                const result = await triggerCron(trigger.id, selectedFunction.path);
                                setTriggerResult({
                                  success: result.success,
                                  message: result.success ? 'Cron job triggered successfully!' : (result.error || 'Failed to trigger')
                                });
                                setTriggering(false);
                              }}
                            >
                              {triggering ? (
                                <>
                                  <Loader2 className="w-3.5 h-3.5 mr-2 animate-spin" />
                                  Running...
                                </>
                              ) : (
                                <>
                                  <Play className="w-3.5 h-3.5 mr-2" />
                                  Run Now
                                </>
                              )}
                            </Button>
                            {triggerResult && (
                              <div className={`mt-2 text-xs p-2 rounded ${triggerResult.success ? 'bg-success/10 text-success' : 'bg-error/10 text-error'}`}>
                                {triggerResult.message}
                              </div>
                            )}
                          </div>
                        </>
                      );
                    })()}

                    {trigger?.trigger_type === 'event' && (() => {
                      const config = trigger.config as { topic?: string; description?: string };
                      const topic = config.topic || 'Unknown';
                      
                      return (
                        <>
                          <div>
                            <div className="text-[10px] text-muted uppercase tracking-wider mb-2">Event Topic</div>
                            <div className="bg-purple-500/10 border border-purple-500/30 rounded-lg p-3">
                              <div className="flex items-center gap-2">
                                <MessageSquare className="w-4 h-4 text-purple-400" />
                                <code className="text-sm font-mono text-purple-400">{topic}</code>
                              </div>
                            </div>
                          </div>

                          <div className="grid grid-cols-2 gap-3">
                            <div className="bg-dark-gray/50 rounded-lg p-3">
                              <div className="text-[10px] text-muted uppercase tracking-wider mb-1">Type</div>
                              <div className="flex items-center gap-1.5 text-sm">
                                <Zap className="w-3.5 h-3.5 text-purple-400" />
                                <span className="text-foreground">Subscriber</span>
                              </div>
                            </div>
                            <div className="bg-dark-gray/50 rounded-lg p-3">
                              <div className="text-[10px] text-muted uppercase tracking-wider mb-1">Status</div>
                              <div className="flex items-center gap-1.5 text-sm">
                                <Activity className="w-3.5 h-3.5 text-success" />
                                <span className="text-success">Listening</span>
                              </div>
                            </div>
                          </div>

                          {config.description && (
                            <div>
                              <div className="text-[10px] text-muted uppercase tracking-wider mb-2">Description</div>
                              <p className="text-xs text-muted">{config.description}</p>
                            </div>
                          )}

                          <div className="border-t border-border pt-4">
                            <div className="text-[10px] text-muted uppercase tracking-wider mb-3 flex items-center gap-2">
                              <Send className="w-3 h-3" />
                              Emit Test Event
                            </div>
                            <div className="space-y-2">
                              <textarea
                                className="w-full h-20 text-xs font-mono bg-black/40 text-foreground px-3 py-2 rounded border border-border focus:border-purple-500 focus:outline-none resize-none"
                                placeholder='{"test": "data"}'
                                value={eventPayload}
                                onChange={(e) => setEventPayload(e.target.value)}
                              />
                              <Button
                                variant="outline"
                                className="w-full h-9 border-purple-500/30 text-purple-400 hover:bg-purple-500/10"
                                disabled={triggering}
                                onClick={async () => {
                                  try {
                                    const payload = JSON.parse(eventPayload);
                                    setTriggering(true);
                                    setTriggerResult(null);
                                    const result = await emitEvent(topic, payload);
                                    setTriggerResult({
                                      success: result.success,
                                      message: result.success ? `Event emitted to "${topic}"!` : (result.error || 'Failed to emit')
                                    });
                                  } catch {
                                    setTriggerResult({
                                      success: false,
                                      message: 'Invalid JSON payload'
                                    });
                                  } finally {
                                    setTriggering(false);
                                  }
                                }}
                              >
                                {triggering ? (
                                  <>
                                    <Loader2 className="w-3.5 h-3.5 mr-2 animate-spin" />
                                    Emitting...
                                  </>
                                ) : (
                                  <>
                                    <Send className="w-3.5 h-3.5 mr-2" />
                                    Emit Event
                                  </>
                                )}
                              </Button>
                              {triggerResult && (
                                <div className={`mt-2 text-xs p-2 rounded ${triggerResult.success ? 'bg-success/10 text-success' : 'bg-error/10 text-error'}`}>
                                  {triggerResult.message}
                                </div>
                              )}
                            </div>
                          </div>
                        </>
                      );
                    })()}

                    <div>
                      <div className="text-[10px] text-muted uppercase tracking-wider mb-2">Function Path</div>
                      <div className="flex items-center gap-2">
                        <code className="flex-1 text-xs font-mono bg-black/40 px-3 py-2 rounded">
                          {selectedFunction.path}
                        </code>
                        <button
                          onClick={() => copyToClipboard(selectedFunction.path, 'path')}
                          className="p-1.5 hover:bg-dark-gray rounded transition-colors"
                        >
                          {copied === 'path' ? (
                            <Check className="w-3.5 h-3.5 text-success" />
                          ) : (
                            <Copy className="w-3.5 h-3.5 text-muted" />
                          )}
                        </button>
                      </div>
                    </div>

                    {selectedFunction.description && (
                      <div>
                        <div className="text-[10px] text-muted uppercase tracking-wider mb-2">Description</div>
                        <p className="text-xs text-muted">{selectedFunction.description}</p>
                      </div>
                    )}

                    {trigger && (
                      <div>
                        <div className="text-[10px] text-muted uppercase tracking-wider mb-2">Configuration</div>
                        <pre className="text-[10px] font-mono bg-black/40 px-3 py-2 rounded overflow-x-auto max-h-32 text-muted">
                          {JSON.stringify(trigger.config, null, 2)}
                        </pre>
                      </div>
                    )}
                  </>
                );
              })()}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

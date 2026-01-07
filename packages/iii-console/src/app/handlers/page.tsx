'use client';

import { useEffect, useState, useMemo } from 'react';
import { Card, CardContent, CardHeader, CardTitle, Badge, Table, TableHeader, TableBody, TableRow, TableHead, TableCell, Button, Input, Select } from "@/components/ui/card";
import { 
  Server, Search, RefreshCw, Activity, Clock, CheckCircle, XCircle, Code2, Eye, EyeOff,
  Play, Terminal, ExternalLink, Copy, Check, ChevronRight, X, Zap, Globe, Calendar,
  MessageSquare, Send, Loader2, AlertCircle, ArrowRight
} from "lucide-react";
import { fetchFunctions, fetchWorkers, fetchTriggers, FunctionInfo, WorkerInfo, TriggerInfo } from "@/lib/api";

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
        const path = config.api_path || '';
        const method = httpMethod || config.http_method || 'GET';
        
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

        const response = await fetch(`http://localhost:3111/${path}`, fetchOptions);
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
      const method = getHttpMethod(fn);
      setHttpMethod(method);
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
      case 'cron': return <Calendar className="w-4 h-4 text-yellow" />;
      case 'event': return <MessageSquare className="w-4 h-4 text-purple-400" />;
      case 'streams:join': case 'streams:leave': return <Zap className="w-4 h-4 text-success" />;
      default: return <Zap className="w-4 h-4 text-muted" />;
    }
  };

  return (
    <div className="flex flex-col h-full bg-background text-foreground">
      <div className="flex items-center justify-between px-5 py-3 bg-dark-gray/30 border-b border-border">
        <div className="flex items-center gap-4">
          <h1 className="text-base font-semibold flex items-center gap-2">
            <Code2 className="w-4 h-4" />
            Functions
          </h1>
          <Badge variant="success" className="gap-1.5">
            <Activity className="w-3 h-3" />
            {userFunctions.length} Active
          </Badge>
          {systemFunctions.length > 0 && !showSystem && (
            <span className="text-xs text-muted">({systemFunctions.length} system hidden)</span>
          )}
        </div>
        
        <div className="flex items-center gap-2">
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

      <div className="grid grid-cols-4 gap-3 px-5 py-4 border-b border-border bg-dark-gray/20">
        <div className="flex items-center gap-3 p-3 rounded-lg bg-background border border-border">
          <div className="p-2 rounded-md bg-cyan-500/10">
            <Code2 className="h-4 w-4 text-cyan-400" />
          </div>
          <div>
            <div className="text-xs text-muted">Functions</div>
            <div className="text-lg font-semibold">{userFunctions.length}</div>
          </div>
        </div>

        <div className="flex items-center gap-3 p-3 rounded-lg bg-background border border-border">
          <div className="p-2 rounded-md bg-purple-500/10">
            <Server className="h-4 w-4 text-purple-400" />
          </div>
          <div>
            <div className="text-xs text-muted">Workers</div>
            <div className="text-lg font-semibold">{workers.length}</div>
          </div>
        </div>

        <div className="flex items-center gap-3 p-3 rounded-lg bg-background border border-border">
          <div className="p-2 rounded-md bg-yellow/10">
            <Zap className="h-4 w-4 text-yellow" />
          </div>
          <div>
            <div className="text-xs text-muted">Triggers</div>
            <div className="text-lg font-semibold">{triggers.filter(t => !t.internal).length}</div>
          </div>
        </div>

        <div className="flex items-center gap-3 p-3 rounded-lg bg-background border border-border">
          <div className="p-2 rounded-md bg-success/10">
            <CheckCircle className="h-4 w-4 text-success" />
          </div>
          <div>
            <div className="text-xs text-muted">Status</div>
            <div className="text-lg font-semibold text-success">Healthy</div>
          </div>
        </div>
      </div>

      <div className={`flex-1 flex overflow-hidden ${selectedFunction ? 'divide-x divide-border' : ''}`}>
        <div className="flex-1 overflow-y-auto p-5 space-y-6">
          <div className="flex items-center gap-3">
            <div className="relative flex-1 max-w-xs">
              <Search className="w-3.5 h-3.5 absolute left-3 top-1/2 -translate-y-1/2 text-muted" />
              <Input 
                placeholder="Search functions..."
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                className="pl-9 h-8 text-xs"
              />
            </div>
          </div>

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
                              <Badge variant="outline" className="text-[9px] uppercase">
                                {trigger.trigger_type}
                              </Badge>
                            )}
                          </div>
                          {apiPath && (
                            <div className="text-xs text-muted font-mono mt-0.5">
                              {getHttpMethod(fn)} /{apiPath}
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
          <div className="w-[420px] flex flex-col h-full overflow-hidden bg-dark-gray/20">
            <div className="flex items-center justify-between px-4 py-3 border-b border-border bg-dark-gray/30">
              <div className="flex items-center gap-2 min-w-0">
                {getFunctionIcon(selectedFunction)}
                <h2 className="font-medium text-sm truncate">{selectedFunction.path}</h2>
              </div>
              <Button
                variant="ghost"
                size="sm"
                onClick={() => setSelectedFunction(null)}
                className="h-6 w-6 p-0 shrink-0"
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

                            {(httpMethod === 'POST' || httpMethod === 'PUT' || httpMethod === 'PATCH') && (
                              <div>
                                <div className="text-[10px] text-muted mb-1.5">Request Body (JSON)</div>
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
                            <div className="p-3">
                              <pre className="text-[11px] font-mono overflow-x-auto max-h-48 text-muted">
                                {invocationResult.error || JSON.stringify(invocationResult.data, null, 2)}
                              </pre>
                            </div>
                          </div>
                        )}
                      </>
                    )}

                    {trigger?.trigger_type === 'cron' && (
                      <div>
                        <div className="text-[10px] text-muted uppercase tracking-wider mb-2">Schedule</div>
                        <code className="text-xs font-mono bg-yellow/10 text-yellow px-3 py-2 rounded border border-yellow/30 block">
                          {(trigger.config as { schedule?: string })?.schedule || 'Unknown'}
                        </code>
                      </div>
                    )}

                    {trigger?.trigger_type === 'event' && (
                      <div>
                        <div className="text-[10px] text-muted uppercase tracking-wider mb-2">Event Topic</div>
                        <code className="text-xs font-mono bg-purple-500/10 text-purple-400 px-3 py-2 rounded border border-purple-500/30 block">
                          {(trigger.config as { topic?: string })?.topic || 'Unknown'}
                        </code>
                      </div>
                    )}

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

'use client';

import { useEffect, useState, useMemo } from 'react';
import { Card, CardContent, CardHeader, CardTitle, Button, Input, Badge } from "@/components/ui/card";
import { 
  Settings, RefreshCw, Copy, Check, Server, Zap, Eye, EyeOff, 
  Play, CheckCircle, XCircle, Clock, Database, Globe, Cpu, 
  Activity, Box, ChevronDown, ChevronRight, Download,
  Users, Layers, Radio, FileText, Wifi, WifiOff, Terminal, 
  AlertCircle, Calendar, Hash, Timer, Search, X, Inbox
} from "lucide-react";
import { fetchConfig, fetchTriggerTypes, fetchAdapters, fetchStatus, fetchFunctions, fetchTriggers, fetchStreams } from "@/lib/api";

interface ModuleInfo {
  id: string;
  type: string;
  status: string;
  health: string;
  description?: string;
  internal?: boolean;
  port?: number;
  count?: number;
  config?: Record<string, unknown>;
}

interface TriggerType {
  id: string;
  description: string;
}

interface EndpointStatus {
  url: string;
  name: string;
  icon: React.ReactNode;
  status: 'checking' | 'online' | 'offline';
  latency?: number;
  description?: string;
}

export default function ConfigPage() {
  const [loading, setLoading] = useState(true);
  const [copied, setCopied] = useState<string | null>(null);
  const [showSystem, setShowSystem] = useState(false);
  const [searchQuery, setSearchQuery] = useState('');
  const [devtoolsConfig, setDevtoolsConfig] = useState<Record<string, unknown> | null>(null);
  const [triggerTypes, setTriggerTypes] = useState<TriggerType[]>([]);
  const [adapters, setAdapters] = useState<ModuleInfo[]>([]);
  const [status, setStatus] = useState<Record<string, unknown> | null>(null);
  const [selectedModule, setSelectedModule] = useState<string | null>(null);
  const [functionCount, setFunctionCount] = useState(0);
  const [triggerCount, setTriggerCount] = useState(0);
  const [streamCount, setStreamCount] = useState(0);
  const [endpoints, setEndpoints] = useState<EndpointStatus[]>([
    { url: 'http://localhost:3111', name: 'iii Engine', icon: <Terminal className="w-4 h-4" />, status: 'checking', description: 'REST API & DevTools' },
    { url: 'ws://localhost:31112', name: 'Streams', icon: <Activity className="w-4 h-4" />, status: 'checking', description: 'WebSocket streams' },
    { url: 'ws://localhost:49134', name: 'SDK Bridge', icon: <Cpu className="w-4 h-4" />, status: 'checking', description: 'Worker connections' },
  ]);
  const [error, setError] = useState<string | null>(null);

  const loadData = async () => {
    setLoading(true);
    setError(null);
    try {
      const [configData, triggerData, adapterData, statusData, funcData, trigData, streamData] = await Promise.all([
        fetchConfig().catch(() => ({})),
        fetchTriggerTypes().catch(() => ({ trigger_types: [] })),
        fetchAdapters().catch(() => ({ adapters: [] })),
        fetchStatus().catch(() => ({})),
        fetchFunctions().catch(() => ({ functions: [] })),
        fetchTriggers().catch(() => ({ triggers: [] })),
        fetchStreams().catch(() => ({ streams: [] })),
      ]);
      setDevtoolsConfig(configData.devtools || null);
      setTriggerTypes(triggerData.trigger_types || []);
      setAdapters(adapterData.adapters || []);
      setStatus(statusData);
      setFunctionCount((funcData.functions || []).filter((f: { internal?: boolean }) => !f.internal).length);
      setTriggerCount((trigData.triggers || []).filter((t: { internal?: boolean }) => !t.internal).length);
      setStreamCount((streamData.streams || []).filter((s: { internal?: boolean }) => !s.internal).length);
    } catch (err) {
      setError('Failed to load configuration');
      console.error(err);
    } finally {
      setLoading(false);
    }
  };

  const checkEndpoints = async () => {
    const results = await Promise.all(endpoints.map(async (ep) => {
      if (ep.url.startsWith('ws://')) {
        return { ...ep, status: 'online' as const };
      }
      const start = Date.now();
      try {
        const response = await fetch(ep.url + '/_console/health', { 
          signal: AbortSignal.timeout(3000) 
        });
        return { 
          ...ep, 
          status: response.ok ? 'online' as const : 'offline' as const,
          latency: Date.now() - start
        };
      } catch {
        return { ...ep, status: 'offline' as const };
      }
    }));
    setEndpoints(results);
  };

  useEffect(() => {
    loadData();
    checkEndpoints();
    const interval = setInterval(() => {
      loadData();
      checkEndpoints();
    }, 10000);
    return () => clearInterval(interval);
  }, []);

  const copyToClipboard = (value: string, key: string) => {
    navigator.clipboard.writeText(value);
    setCopied(key);
    setTimeout(() => setCopied(null), 2000);
  };

  const modules = useMemo(() => {
    return adapters
      .filter(a => a.type === 'module' && (showSystem || !a.internal))
      .filter(a => !searchQuery || a.id.toLowerCase().includes(searchQuery.toLowerCase()));
  }, [adapters, showSystem, searchQuery]);

  const workers = useMemo(() => {
    return adapters.filter(a => a.type === 'worker_pool');
  }, [adapters]);

  const triggerHandlers = useMemo(() => {
    return adapters.filter(a => a.type === 'trigger' && (showSystem || !a.internal));
  }, [adapters, showSystem]);

  const selectedModuleData = adapters.find(a => a.id === selectedModule);

  const stats = useMemo(() => ({
    modules: modules.length,
    functions: functionCount,
    triggers: triggerCount,
    streams: streamCount,
    workers: workers.reduce((sum, w) => sum + (w.count || 0), 0),
    healthy: modules.filter(m => m.health === 'healthy').length,
  }), [modules, functionCount, triggerCount, streamCount, workers]);

  const exportConfig = () => {
    const yaml = `# iii Engine Configuration
# Generated from Developer Console at ${new Date().toISOString()}

modules:
${modules.map(m => `  - class: ${m.id}
    config:
      # Add module-specific configuration here
`).join('\n')}
`;
    const blob = new Blob([yaml], { type: 'text/yaml' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = 'config.yaml';
    a.click();
    URL.revokeObjectURL(url);
  };

  return (
    <div className="flex flex-col h-full bg-background text-foreground">
      <div className="flex items-center justify-between px-5 py-3 bg-dark-gray/30 border-b border-border">
        <div className="flex items-center gap-4">
          <h1 className="text-base font-semibold flex items-center gap-2">
            <Settings className="w-4 h-4" />
            Configuration
          </h1>
          <Badge variant={stats.healthy === stats.modules ? 'success' : 'warning'} className="gap-1.5">
            <CheckCircle className="w-3 h-3" />
            {stats.healthy}/{stats.modules} healthy
          </Badge>
        </div>
        
        <div className="flex items-center gap-2">
          <Button variant="ghost" size="sm" onClick={exportConfig} className="h-7 text-xs">
            <Download className="w-3 h-3 mr-1.5" />
            Export
          </Button>
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
            onClick={() => { loadData(); checkEndpoints(); }} 
            disabled={loading}
            className="h-7 text-xs text-muted hover:text-foreground"
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

      <div className="grid grid-cols-6 gap-3 p-4 border-b border-border bg-dark-gray/20">
        <div className="p-3 rounded-lg bg-background border border-border">
          <div className="flex items-center justify-between mb-1">
            <Layers className="w-4 h-4 text-cyan" />
            <span className="text-xs text-muted">Modules</span>
          </div>
          <div className="text-2xl font-bold text-cyan">{stats.modules}</div>
        </div>
        <div className="p-3 rounded-lg bg-background border border-border">
          <div className="flex items-center justify-between mb-1">
            <Box className="w-4 h-4 text-purple-400" />
            <span className="text-xs text-muted">Functions</span>
          </div>
          <div className="text-2xl font-bold text-purple-400">{stats.functions}</div>
        </div>
        <div className="p-3 rounded-lg bg-background border border-border">
          <div className="flex items-center justify-between mb-1">
            <Zap className="w-4 h-4 text-yellow" />
            <span className="text-xs text-muted">Triggers</span>
          </div>
          <div className="text-2xl font-bold text-yellow">{stats.triggers}</div>
        </div>
        <div className="p-3 rounded-lg bg-background border border-border">
          <div className="flex items-center justify-between mb-1">
            <Database className="w-4 h-4 text-green-400" />
            <span className="text-xs text-muted">Streams</span>
          </div>
          <div className="text-2xl font-bold text-green-400">{stats.streams}</div>
        </div>
        <div className="p-3 rounded-lg bg-background border border-border">
          <div className="flex items-center justify-between mb-1">
            <Users className="w-4 h-4 text-blue-400" />
            <span className="text-xs text-muted">Workers</span>
          </div>
          <div className="text-2xl font-bold text-blue-400">{stats.workers}</div>
        </div>
        <div className="p-3 rounded-lg bg-background border border-border">
          <div className="flex items-center justify-between mb-1">
            <CheckCircle className="w-4 h-4 text-success" />
            <span className="text-xs text-muted">Healthy</span>
          </div>
          <div className="text-2xl font-bold text-success">{stats.healthy}</div>
        </div>
      </div>

      <div className="grid grid-cols-3 gap-3 p-4 border-b border-border">
        {endpoints.map((ep) => (
          <div 
            key={`${ep.url}-${ep.name}`}
            className={`p-3 rounded-lg border transition-all ${
              ep.status === 'online' 
                ? 'bg-success/5 border-success/30' 
                : ep.status === 'offline'
                ? 'bg-error/5 border-error/30'
                : 'bg-dark-gray/30 border-border'
            }`}
          >
            <div className="flex items-center gap-2 mb-2">
              <div className={`p-1.5 rounded ${
                ep.status === 'online' ? 'bg-success/20 text-success' : 
                ep.status === 'offline' ? 'bg-error/20 text-error' : 
                'bg-muted/20 text-muted'
              }`}>
                {ep.icon}
              </div>
              <div className="flex-1 min-w-0">
                <div className="font-medium text-sm">{ep.name}</div>
                <div className="text-[10px] text-muted">{ep.description}</div>
              </div>
              {ep.status === 'online' && ep.latency && (
                <span className="text-xs text-success font-mono">{ep.latency}ms</span>
              )}
            </div>
            <div className="flex items-center justify-between">
              <code className="text-[10px] text-muted font-mono truncate">{ep.url}</code>
              <button 
                onClick={() => copyToClipboard(ep.url, ep.url)}
                className="p-1 hover:bg-dark-gray rounded transition-colors"
              >
                {copied === ep.url ? (
                  <Check className="w-3 h-3 text-success" />
                ) : (
                  <Copy className="w-3 h-3 text-muted" />
                )}
              </button>
            </div>
          </div>
        ))}
      </div>

      <div className={`flex-1 grid overflow-hidden ${selectedModule ? 'grid-cols-[1fr_320px]' : 'grid-cols-1'}`}>
        <div className="flex flex-col h-full overflow-hidden">
          <div className="p-3 border-b border-border bg-dark-gray/20">
            <div className="relative">
              <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-muted" />
              <Input
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                placeholder="Search modules..."
                className="pl-8 h-8 text-xs"
              />
              {searchQuery && (
                <button
                  onClick={() => setSearchQuery('')}
                  className="absolute right-2.5 top-1/2 -translate-y-1/2 text-muted hover:text-foreground"
                >
                  <X className="w-3.5 h-3.5" />
                </button>
              )}
            </div>
          </div>

          <div className="flex-1 overflow-y-auto p-4">
            {loading ? (
              <div className="flex items-center justify-center h-32">
                <RefreshCw className="w-5 h-5 text-muted animate-spin" />
              </div>
            ) : (
              <div className="space-y-6">
                <div>
                  <h3 className="text-xs font-medium text-muted uppercase tracking-wider mb-3 flex items-center gap-2">
                    <Layers className="w-3.5 h-3.5" />
                    Active Modules ({modules.length})
                  </h3>
                  <div className="grid gap-2 md:grid-cols-2 lg:grid-cols-3">
                    {modules.map((mod) => (
                      <button
                        key={mod.id}
                        onClick={() => setSelectedModule(selectedModule === mod.id ? null : mod.id)}
                        className={`p-3 rounded-lg border text-left transition-all ${
                          selectedModule === mod.id
                            ? 'bg-primary/10 border-primary'
                            : mod.health === 'healthy'
                            ? 'bg-success/5 border-success/30 hover:border-success/50'
                            : 'bg-error/5 border-error/30 hover:border-error/50'
                        }`}
                      >
                        <div className="flex items-center gap-2 mb-1">
                          {mod.health === 'healthy' ? (
                            <CheckCircle className="w-3.5 h-3.5 text-success" />
                          ) : (
                            <XCircle className="w-3.5 h-3.5 text-error" />
                          )}
                          <span className="font-medium text-sm truncate">{mod.id.split('::').pop()}</span>
                          {mod.port && (
                            <code className="text-[10px] px-1.5 py-0.5 rounded bg-black/40 text-muted font-mono ml-auto">
                              :{mod.port}
                            </code>
                          )}
                        </div>
                        <div className="text-[10px] text-muted truncate">{mod.id}</div>
                      </button>
                    ))}
                  </div>
                </div>

                {triggerHandlers.length > 0 && (
                  <div>
                    <h3 className="text-xs font-medium text-muted uppercase tracking-wider mb-3 flex items-center gap-2">
                      <Zap className="w-3.5 h-3.5 text-yellow" />
                      Trigger Handlers ({triggerHandlers.length})
                    </h3>
                    <div className="grid gap-2 md:grid-cols-2 lg:grid-cols-4">
                      {triggerHandlers.map((handler) => (
                        <div 
                          key={handler.id}
                          className="p-3 rounded-lg border border-yellow/30 bg-yellow/5"
                        >
                          <div className="flex items-center gap-2 mb-1">
                            <Zap className="w-3.5 h-3.5 text-yellow" />
                            <span className="font-medium text-sm truncate">{handler.id}</span>
                          </div>
                          <div className="flex items-center gap-2">
                            <span className={`text-[10px] px-1.5 py-0.5 rounded ${
                              handler.status === 'active' ? 'bg-success/20 text-success' : 'bg-muted/20 text-muted'
                            }`}>
                              {handler.status}
                            </span>
                          </div>
                        </div>
                      ))}
                    </div>
                  </div>
                )}

                {workers.length > 0 && (
                  <div>
                    <h3 className="text-xs font-medium text-muted uppercase tracking-wider mb-3 flex items-center gap-2">
                      <Users className="w-3.5 h-3.5 text-blue-400" />
                      Worker Pools ({workers.length})
                    </h3>
                    <div className="grid gap-2 md:grid-cols-2 lg:grid-cols-3">
                      {workers.map((worker) => (
                        <div 
                          key={worker.id}
                          className="p-3 rounded-lg border border-blue-400/30 bg-blue-400/5"
                        >
                          <div className="flex items-center justify-between mb-1">
                            <div className="flex items-center gap-2">
                              <Users className="w-3.5 h-3.5 text-blue-400" />
                              <span className="font-medium text-sm">{worker.id}</span>
                            </div>
                            <span className="text-sm font-bold text-blue-400">{worker.count || 0}</span>
                          </div>
                          <div className="text-[10px] text-muted">connected workers</div>
                        </div>
                      ))}
                    </div>
                  </div>
                )}

                <div>
                  <h3 className="text-xs font-medium text-muted uppercase tracking-wider mb-3 flex items-center gap-2">
                    <Radio className="w-3.5 h-3.5 text-green-400" />
                    Trigger Types ({triggerTypes.length})
                  </h3>
                  <div className="grid gap-2 md:grid-cols-2 lg:grid-cols-4 xl:grid-cols-5">
                    {triggerTypes.map((tt) => (
                      <div 
                        key={tt.id}
                        className="p-3 rounded-lg border border-border bg-dark-gray/30 hover:border-green-400/30 transition-colors group"
                      >
                        <div className="flex items-center gap-2 mb-1">
                          {tt.id === 'api' && <Globe className="w-3.5 h-3.5 text-cyan" />}
                          {tt.id === 'cron' && <Calendar className="w-3.5 h-3.5 text-yellow" />}
                          {tt.id === 'event' && <Radio className="w-3.5 h-3.5 text-green-400" />}
                          {tt.id.includes('streams') && <Database className="w-3.5 h-3.5 text-purple-400" />}
                          {!['api', 'cron', 'event'].includes(tt.id) && !tt.id.includes('streams') && (
                            <Zap className="w-3.5 h-3.5 text-muted" />
                          )}
                          <span className="font-medium text-sm">{tt.id}</span>
                        </div>
                        <div className="text-[10px] text-muted line-clamp-1 mb-2">{tt.description}</div>
                        <div className="flex items-center gap-1">
                          <code className="text-[10px] px-1.5 py-0.5 rounded bg-black/40 text-muted font-mono flex-1 truncate">
                            "{tt.id}"
                          </code>
                          <button 
                            onClick={() => copyToClipboard(tt.id, `tt-${tt.id}`)}
                            className="p-1 hover:bg-dark-gray rounded transition-colors opacity-0 group-hover:opacity-100"
                          >
                            {copied === `tt-${tt.id}` ? (
                              <Check className="w-3 h-3 text-success" />
                            ) : (
                              <Copy className="w-3 h-3 text-muted" />
                            )}
                          </button>
                        </div>
                      </div>
                    ))}
                  </div>
                </div>
              </div>
            )}
          </div>
        </div>

        {selectedModuleData && (
          <div className="border-l border-border bg-dark-gray/20 overflow-y-auto">
            <div className="p-4 border-b border-border sticky top-0 bg-dark-gray/50 backdrop-blur">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2">
                  {selectedModuleData.health === 'healthy' ? (
                    <CheckCircle className="w-4 h-4 text-success" />
                  ) : (
                    <XCircle className="w-4 h-4 text-error" />
                  )}
                  <h2 className="font-medium text-sm">{selectedModuleData.id.split('::').pop()}</h2>
                </div>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => setSelectedModule(null)}
                  className="h-6 w-6 p-0"
                >
                  <X className="w-3.5 h-3.5" />
                </Button>
              </div>
            </div>

            <div className="p-4 space-y-4">
              <div>
                <div className="text-[10px] text-muted uppercase tracking-wider mb-2">Full ID</div>
                <code className="text-xs font-mono bg-black/40 px-2 py-1 rounded block break-all">
                  {selectedModuleData.id}
                </code>
              </div>

              <div>
                <div className="text-[10px] text-muted uppercase tracking-wider mb-2">Type</div>
                <Badge variant="outline">{selectedModuleData.type}</Badge>
              </div>

              <div>
                <div className="text-[10px] text-muted uppercase tracking-wider mb-2">Status</div>
                <div className="flex items-center gap-2">
                  <span className={`px-2 py-0.5 rounded text-xs ${
                    selectedModuleData.status === 'active' ? 'bg-success/20 text-success' : 'bg-muted/20 text-muted'
                  }`}>
                    {selectedModuleData.status}
                  </span>
                  <span className={`px-2 py-0.5 rounded text-xs ${
                    selectedModuleData.health === 'healthy' ? 'bg-success/20 text-success' : 'bg-error/20 text-error'
                  }`}>
                    {selectedModuleData.health}
                  </span>
                </div>
              </div>

              {selectedModuleData.port && (
                <div>
                  <div className="text-[10px] text-muted uppercase tracking-wider mb-2">Port</div>
                  <code className="text-sm font-mono">{selectedModuleData.port}</code>
                </div>
              )}

              {selectedModuleData.description && (
                <div>
                  <div className="text-[10px] text-muted uppercase tracking-wider mb-2">Description</div>
                  <p className="text-xs text-muted">{selectedModuleData.description}</p>
                </div>
              )}

              {selectedModuleData.config && Object.keys(selectedModuleData.config).length > 0 && (
                <div>
                  <div className="text-[10px] text-muted uppercase tracking-wider mb-2">Configuration</div>
                  <pre className="text-[10px] font-mono bg-black/40 px-3 py-2 rounded overflow-x-auto max-h-48">
                    {JSON.stringify(selectedModuleData.config, null, 2)}
                  </pre>
                </div>
              )}

              {selectedModuleData.internal && (
                <div className="pt-2 border-t border-border">
                  <span className="text-[10px] px-2 py-0.5 rounded bg-muted/20 text-muted">
                    Internal Module
                  </span>
                </div>
              )}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

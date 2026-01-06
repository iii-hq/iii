'use client';

import { useState, useEffect } from 'react';
import { Card, CardContent, CardHeader, CardTitle, Badge, Button } from "@/components/ui/card";
import { Plug, RefreshCw, Database, Zap, Server, Wifi, Eye, EyeOff, CheckCircle2 } from "lucide-react";
import { fetchAdapters } from "@/lib/api";

interface Adapter {
  id: string;
  type: string;
  status: string;
  health: string;
  description?: string;
  internal?: boolean;
  port?: number;
  count?: number;
}

export default function AdaptersPage() {
  const [loading, setLoading] = useState(true);
  const [showSystem, setShowSystem] = useState(false);
  const [adapters, setAdapters] = useState<Adapter[]>([]);
  const [error, setError] = useState<string | null>(null);

  const loadData = async () => {
    setLoading(true);
    setError(null);
    try {
      const data = await fetchAdapters();
      setAdapters(data.adapters || []);
    } catch (err) {
      setError('Failed to fetch adapters');
      console.error(err);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    loadData();
  }, []);

  const userAdapters = adapters.filter(a => !a.internal);
  const systemAdapters = adapters.filter(a => a.internal === true);
  const displayAdapters = showSystem ? adapters : userAdapters;

  const triggerTypes = displayAdapters.filter(a => a.type === 'trigger');
  const modules = displayAdapters.filter(a => a.type === 'module' || a.type === 'worker_pool');

  const getTypeIcon = (type: string) => {
    switch (type) {
      case 'trigger': return <Zap className="w-4 h-4" />;
      case 'module': return <Server className="w-4 h-4" />;
      case 'worker_pool': return <Database className="w-4 h-4" />;
      default: return <Plug className="w-4 h-4" />;
    }
  };

  return (
    <div className="p-6 space-y-6">
      {}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-xl font-semibold tracking-tight text-foreground">Adapters & Modules</h1>
          <p className="text-xs text-muted mt-1 tracking-wide">
            Your engine components and trigger types
            {systemAdapters.length > 0 && !showSystem && (
              <span className="text-muted/60 ml-2">({systemAdapters.length} system hidden)</span>
            )}
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
          <Button variant="outline" size="sm" onClick={loadData} disabled={loading}>
            <RefreshCw className={`w-3 h-3 mr-2 ${loading ? 'animate-spin' : ''}`} />
            Refresh
          </Button>
        </div>
      </div>

      {}
      {error && (
        <div className="bg-error/10 border border-error/30 rounded px-4 py-3 text-sm text-error">
          {error}
        </div>
      )}

      {}
      {loading && (
        <div className="flex items-center justify-center py-12 text-muted">
          <RefreshCw className="w-4 h-4 animate-spin mr-2" />
          Loading adapters...
        </div>
      )}

      {!loading && !error && (
        <>
          {}
          <div className="grid gap-4 grid-cols-2 lg:grid-cols-4">
            <Card>
              <CardContent className="p-4">
                <div className="flex items-center justify-between">
                  <div>
                    <div className="text-[10px] text-muted uppercase tracking-wider">Your Adapters</div>
                    <div className="text-2xl font-semibold mt-1 text-foreground">{userAdapters.length}</div>
                  </div>
                  <Plug className="w-5 h-5 text-muted" />
                </div>
              </CardContent>
            </Card>
            <Card>
              <CardContent className="p-4">
                <div className="flex items-center justify-between">
                  <div>
                    <div className="text-[10px] text-muted uppercase tracking-wider">Trigger Types</div>
                    <div className="text-2xl font-semibold mt-1 text-foreground">{triggerTypes.length}</div>
                  </div>
                  <Zap className="w-5 h-5 text-yellow" />
                </div>
              </CardContent>
            </Card>
            <Card>
              <CardContent className="p-4">
                <div className="flex items-center justify-between">
                  <div>
                    <div className="text-[10px] text-muted uppercase tracking-wider">Active</div>
                    <div className="text-2xl font-semibold mt-1 text-success">{displayAdapters.filter(a => a.status === 'active').length}</div>
                  </div>
                  <CheckCircle2 className="w-5 h-5 text-success" />
                </div>
              </CardContent>
            </Card>
            <Card>
              <CardContent className="p-4">
                <div className="flex items-center justify-between">
                  <div>
                    <div className="text-[10px] text-muted uppercase tracking-wider">Modules</div>
                    <div className="text-2xl font-semibold mt-1 text-foreground">{modules.length}</div>
                  </div>
                  <Server className="w-5 h-5 text-muted" />
                </div>
              </CardContent>
            </Card>
          </div>

          {}
          {triggerTypes.length > 0 && (
            <Card>
              <CardHeader>
                <CardTitle className="flex items-center gap-2">
                  <Zap className="w-4 h-4 text-yellow" />
                  Trigger Types
                </CardTitle>
              </CardHeader>
              <CardContent>
                <div className="grid gap-3 md:grid-cols-3">
                  {triggerTypes.map(adapter => (
                    <div key={adapter.id} className="p-4 bg-dark-gray/30 rounded border border-border hover:border-yellow/30 transition-colors">
                      <div className="flex items-center justify-between mb-2">
                        <div className="flex items-center gap-2">
                          <Zap className="w-4 h-4 text-yellow" />
                          <span className="font-medium text-sm">{adapter.id}</span>
                        </div>
                        <div className="w-2 h-2 rounded-full bg-success" />
                      </div>
                      <div className="text-xs text-muted">{adapter.description}</div>
                    </div>
                  ))}
                </div>
              </CardContent>
            </Card>
          )}

          {}
          {modules.length > 0 && (
            <Card>
              <CardHeader>
                <CardTitle className="flex items-center gap-2">
                  <Server className="w-4 h-4 text-muted" />
                  Core Modules
                </CardTitle>
              </CardHeader>
              <CardContent>
                <div className="grid gap-4 md:grid-cols-2">
                  {modules.map(adapter => (
                    <div key={adapter.id} className="p-4 bg-dark-gray/30 rounded border border-border">
                      <div className="flex items-start justify-between">
                        <div className="flex items-center gap-3">
                          <div className="p-2 bg-dark-gray rounded">
                            {getTypeIcon(adapter.type)}
                          </div>
                          <div>
                            <div className="font-medium capitalize">{adapter.id}</div>
                            <div className="text-[10px] text-muted">{adapter.type}</div>
                          </div>
                        </div>
                        <Badge variant="success">Healthy</Badge>
                      </div>
                      <div className="mt-3 text-xs text-muted">{adapter.description}</div>
                      <div className="mt-2 flex items-center gap-4 text-[10px] text-muted">
                        <span className="flex items-center gap-1">
                          <CheckCircle2 className="w-3 h-3" />
                          Status: {adapter.status}
                        </span>
                        {adapter.port && (
                          <span className="flex items-center gap-1">
                            <Wifi className="w-3 h-3" />
                            Port: {adapter.port}
                          </span>
                        )}
                        {adapter.count !== undefined && (
                          <span className="flex items-center gap-1">
                            <Database className="w-3 h-3" />
                            Count: {adapter.count}
                          </span>
                        )}
                      </div>
                    </div>
                  ))}
                </div>
              </CardContent>
            </Card>
          )}

          {displayAdapters.length === 0 && (
            <Card>
              <CardContent className="py-16">
                <div className="flex flex-col items-center justify-center text-center">
                  <Plug className="w-12 h-12 text-muted mb-4 opacity-30" />
                  <h3 className="text-sm font-medium mb-2">No adapters found</h3>
                  <p className="text-xs text-muted">Toggle "Show System" to see internal adapters</p>
                </div>
              </CardContent>
            </Card>
          )}
        </>
      )}
    </div>
  );
}

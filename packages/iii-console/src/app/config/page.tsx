'use client';

import { useEffect, useState } from 'react';
import { Card, CardContent, CardHeader, CardTitle, Button } from "@/components/ui/card";
import { Settings, RefreshCw, Copy, Check, Server, Zap, Eye, EyeOff } from "lucide-react";
import { fetchConfig, fetchTriggerTypes } from "@/lib/api";

interface DevToolsConfig {
  enabled: boolean;
  api_prefix: string;
  metrics_enabled: boolean;
  metrics_interval: number;
  state_stream: string;
  event_topic: string;
}

interface EngineConfig {
  version: string;
}

interface TriggerType {
  id: string;
  description: string;
}

export default function ConfigPage() {
  const [loading, setLoading] = useState(true);
  const [copied, setCopied] = useState<string | null>(null);
  const [showSystem, setShowSystem] = useState(false);
  const [devtoolsConfig, setDevtoolsConfig] = useState<DevToolsConfig | null>(null);
  const [engineConfig, setEngineConfig] = useState<EngineConfig | null>(null);
  const [triggerTypes, setTriggerTypes] = useState<TriggerType[]>([]);
  const [error, setError] = useState<string | null>(null);

  const loadData = async () => {
    setLoading(true);
    setError(null);
    try {
      const [configData, triggerData] = await Promise.all([
        fetchConfig(),
        fetchTriggerTypes()
      ]);
      setDevtoolsConfig(configData.devtools);
      setEngineConfig(configData.engine);
      setTriggerTypes(triggerData.trigger_types || []);
    } catch (err) {
      setError('Failed to fetch configuration');
      console.error(err);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    loadData();
  }, []);

  const copyToClipboard = (value: string, key: string) => {
    navigator.clipboard.writeText(value);
    setCopied(key);
    setTimeout(() => setCopied(null), 2000);
  };

  return (
    <div className="p-6 space-y-6">
      {}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-xl font-semibold tracking-tight">Configuration</h1>
          <p className="text-xs text-muted mt-1 tracking-wide">Engine settings and registered trigger types</p>
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
          Loading configuration...
        </div>
      )}

      {!loading && !error && (
        <>
          {}
          {devtoolsConfig && showSystem && (
            <Card>
              <CardHeader>
                <CardTitle className="flex items-center gap-2">
                  <Settings className="w-4 h-4 text-muted" />
                  DevTools Module Configuration
                </CardTitle>
              </CardHeader>
              <CardContent>
                <div className="grid gap-4 md:grid-cols-2">
                  <ConfigItem label="Enabled" value={devtoolsConfig.enabled ? 'TRUE' : 'FALSE'} isBoolean copied={copied} onCopy={copyToClipboard} />
                  <ConfigItem label="API Prefix" value={devtoolsConfig.api_prefix} copied={copied} onCopy={copyToClipboard} />
                  <ConfigItem label="Metrics Enabled" value={devtoolsConfig.metrics_enabled ? 'TRUE' : 'FALSE'} isBoolean copied={copied} onCopy={copyToClipboard} />
                  <ConfigItem label="Metrics Interval" value={`${devtoolsConfig.metrics_interval}s`} copied={copied} onCopy={copyToClipboard} />
                  <ConfigItem label="State Stream" value={devtoolsConfig.state_stream} copied={copied} onCopy={copyToClipboard} />
                  <ConfigItem label="Event Topic" value={devtoolsConfig.event_topic} copied={copied} onCopy={copyToClipboard} />
                </div>
              </CardContent>
            </Card>
          )}

          {}
          <Card>
            <CardHeader>
              <CardTitle className="flex items-center gap-2">
                <Zap className="w-4 h-4 text-yellow" />
                Registered Trigger Types
              </CardTitle>
            </CardHeader>
            <CardContent>
              <div className="grid gap-3 md:grid-cols-3">
                {triggerTypes.map(tt => (
                  <div key={tt.id} className="p-4 bg-dark-gray/30 rounded border border-border hover:border-yellow/30 transition-colors">
                    <div className="flex items-center gap-2 mb-2">
                      <Zap className="w-4 h-4 text-yellow" />
                      <span className="font-medium text-sm">{tt.id}</span>
                    </div>
                    <div className="text-xs text-muted">{tt.description}</div>
                  </div>
                ))}
              </div>
            </CardContent>
          </Card>

          {}
          {showSystem && (
            <Card>
              <CardHeader>
                <CardTitle className="flex items-center gap-2">
                  <Server className="w-4 h-4 text-muted" />
                  Events Configuration
                </CardTitle>
              </CardHeader>
              <CardContent>
                <div className="grid gap-4 md:grid-cols-2">
                  <ConfigItem label="Event Topic" value={devtoolsConfig?.event_topic || 'N/A'} copied={copied} onCopy={copyToClipboard} />
                  <ConfigItem label="State Stream" value={devtoolsConfig?.state_stream || 'N/A'} copied={copied} onCopy={copyToClipboard} />
                </div>
                <div className="mt-4 text-xs text-muted">
                  Subscribe to this topic for real-time DevTools updates
                </div>
              </CardContent>
            </Card>
          )}

          {}
          {showSystem && (
            <Card>
              <CardHeader>
                <CardTitle>Raw Configuration (from iii API)</CardTitle>
              </CardHeader>
              <CardContent>
                <pre className="bg-black p-4 rounded border border-border text-xs overflow-auto max-h-64 font-mono">
                  {JSON.stringify({ devtools: devtoolsConfig, engine: engineConfig }, null, 2)}
                </pre>
              </CardContent>
            </Card>
          )}

          {}
          <Card>
            <CardHeader>
              <CardTitle className="flex items-center gap-2">
                <Server className="w-4 h-4 text-muted" />
                Engine Endpoints
              </CardTitle>
            </CardHeader>
            <CardContent>
              <div className="grid gap-4 md:grid-cols-3">
                <ConfigItem 
                  label="REST API" 
                  value="http://localhost:3111" 
                  copied={copied}
                  onCopy={copyToClipboard}
                />
                <ConfigItem 
                  label="WebSocket Streams" 
                  value="ws://localhost:31112" 
                  copied={copied}
                  onCopy={copyToClipboard}
                />
                <ConfigItem 
                  label="Management API" 
                  value="http://localhost:9001" 
                  copied={copied}
                  onCopy={copyToClipboard}
                />
              </div>
            </CardContent>
          </Card>
        </>
      )}
    </div>
  );
}

function ConfigItem({ 
  label, 
  value, 
  copied,
  onCopy,
  isBoolean = false
}: { 
  label: string; 
  value: string; 
  copied: string | null;
  onCopy: (value: string, key: string) => void;
  isBoolean?: boolean;
}) {
  return (
    <div className="p-3 bg-dark-gray/30 rounded border border-border">
      <div className="text-[10px] text-muted uppercase tracking-wider mb-1">{label}</div>
      <div className="flex items-center justify-between">
        {isBoolean ? (
          <span className={`text-xs font-medium px-2 py-0.5 rounded ${value === 'TRUE' ? 'bg-success/20 text-success' : 'bg-error/20 text-error'}`}>
            {value}
          </span>
        ) : (
          <code className="text-xs font-mono text-foreground">{value}</code>
        )}
        <button 
          onClick={() => onCopy(value, label)}
          className="p-1 hover:bg-dark-gray rounded transition-colors"
        >
          {copied === label ? (
            <Check className="w-3 h-3 text-success" />
          ) : (
            <Copy className="w-3 h-3 text-muted" />
          )}
        </button>
      </div>
    </div>
  );
}

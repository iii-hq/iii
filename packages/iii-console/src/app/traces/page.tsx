'use client';

import { useState } from 'react';
import { Card, CardContent, CardHeader, CardTitle, Badge, Button, Input, Select } from "@/components/ui/card";
import { 
  GitBranch, 
  Search, 
  Filter, 
  RefreshCw, 
  Download, 
  Clock, 
  CheckCircle2,
  XCircle,
  Zap,
  Eye,
  EyeOff,
  AlertCircle,
  ExternalLink
} from "lucide-react";

export default function TracesPage() {
  const [searchQuery, setSearchQuery] = useState('');
  const [statusFilter, setStatusFilter] = useState<'all' | 'ok' | 'error'>('all');
  const [serviceFilter, setServiceFilter] = useState('all');
  const [isLoading, setIsLoading] = useState(false);
  const [showSystem, setShowSystem] = useState(false);

  const refreshTraces = () => {
    setIsLoading(true);
    setTimeout(() => {
      setIsLoading(false);
    }, 500);
  };

  return (
    <div className="p-6 space-y-6 h-[calc(100vh-48px)] flex flex-col">
      {}
      <div className="flex items-center justify-between flex-shrink-0">
        <div>
          <h1 className="text-xl font-semibold tracking-tight">Traces</h1>
          <p className="text-xs text-muted mt-1 tracking-wide">
            OpenTelemetry distributed tracing
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
          <Button variant="outline" size="sm" onClick={refreshTraces} disabled={isLoading}>
            <RefreshCw className={`w-3 h-3 mr-2 ${isLoading ? 'animate-spin' : ''}`} />
            Refresh
          </Button>
          <Button variant="outline" size="sm" disabled>
            <Download className="w-3 h-3 mr-2" />
            Export
          </Button>
        </div>
      </div>

      {}
      <Card className="flex-shrink-0">
        <CardContent className="py-8">
          <div className="flex flex-col items-center justify-center text-center">
            <AlertCircle className="w-12 h-12 text-yellow mb-4" />
            <h3 className="text-sm font-medium mb-2">OpenTelemetry Not Configured</h3>
            <p className="text-xs text-muted max-w-md mb-4">
              Configure <code className="bg-dark-gray px-1 rounded font-mono">opentelemetry-rust</code> in the iii engine to enable distributed tracing.
              Traces will be compatible with Datadog, Jaeger, Dash0, and other OpenTelemetry backends.
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
        </CardContent>
      </Card>

      {}
      <div className="grid grid-cols-4 gap-4 flex-shrink-0">
        <Card>
          <CardContent className="py-3 flex items-center justify-between">
            <div>
              <p className="text-[10px] text-muted uppercase tracking-wider">Traces</p>
              <p className="text-2xl font-semibold">0</p>
            </div>
            <GitBranch className="w-5 h-5 text-muted" />
          </CardContent>
        </Card>
        <Card>
          <CardContent className="py-3 flex items-center justify-between">
            <div>
              <p className="text-[10px] text-muted uppercase tracking-wider">Spans</p>
              <p className="text-2xl font-semibold">0</p>
            </div>
            <Zap className="w-5 h-5 text-muted" />
          </CardContent>
        </Card>
        <Card>
          <CardContent className="py-3 flex items-center justify-between">
            <div>
              <p className="text-[10px] text-muted uppercase tracking-wider">Errors</p>
              <p className="text-2xl font-semibold">0</p>
            </div>
            <XCircle className="w-5 h-5 text-muted" />
          </CardContent>
        </Card>
        <Card>
          <CardContent className="py-3 flex items-center justify-between">
            <div>
              <p className="text-[10px] text-muted uppercase tracking-wider">Avg Duration</p>
              <p className="text-2xl font-semibold">—</p>
            </div>
            <Clock className="w-5 h-5 text-muted" />
          </CardContent>
        </Card>
      </div>

      {}
      <Card className="flex-shrink-0">
        <CardContent className="py-3 flex items-center justify-between">
          <div className="flex items-center gap-4">
            <div className="flex items-center gap-2">
              <Filter className="w-3 h-3 text-muted" />
              <span className="text-[10px] text-muted uppercase tracking-wider">Filters</span>
            </div>
            <div className="flex items-center gap-2">
              {(['all', 'ok', 'error'] as const).map(status => (
                <button
                  key={status}
                  onClick={() => setStatusFilter(status)}
                  className={`flex items-center gap-1 px-2 py-1 text-[10px] uppercase tracking-wider rounded transition-colors ${
                    statusFilter === status 
                      ? 'bg-white text-black' 
                      : 'text-muted hover:text-foreground hover:bg-dark-gray'
                  }`}
                >
                  {status === 'ok' && <CheckCircle2 className="w-3 h-3" />}
                  {status === 'error' && <XCircle className="w-3 h-3" />}
                  {status}
                </button>
              ))}
            </div>
            <Select
              value={serviceFilter}
              onChange={(e) => setServiceFilter(e.target.value)}
              className="text-xs w-40"
            >
              <option value="all">All Services</option>
            </Select>
          </div>
          <div className="relative">
            <Search className="w-3 h-3 absolute left-3 top-1/2 -translate-y-1/2 text-muted" />
            <Input 
              placeholder="Search by operation or trace ID..."
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              className="pl-8 w-72"
            />
          </div>
        </CardContent>
      </Card>

      {}
      <Card className="flex-1 overflow-hidden flex flex-col">
        <CardHeader className="flex-shrink-0 flex flex-row items-center justify-between py-3">
          <div className="flex items-center gap-2">
            <GitBranch className="w-4 h-4 text-muted" />
            <CardTitle>Recent Traces</CardTitle>
          </div>
          <div className="text-[10px] text-muted">
            0 traces
          </div>
        </CardHeader>
        
        {}
        <div className="grid grid-cols-12 gap-2 py-2 px-4 bg-dark-gray/30 text-[10px] text-muted uppercase tracking-wider border-y border-border">
          <div className="col-span-1">Status</div>
          <div className="col-span-3">Trace ID</div>
          <div className="col-span-3">Root Operation</div>
          <div className="col-span-2">Services</div>
          <div className="col-span-1 text-right">Spans</div>
          <div className="col-span-1 text-right">Duration</div>
          <div className="col-span-1 text-right">Time</div>
        </div>
        
        <CardContent className="flex-1 overflow-y-auto p-0">
          <div className="flex flex-col items-center justify-center h-full text-muted py-16">
            <GitBranch className="w-12 h-12 mb-4 opacity-30" />
            <div className="text-sm">No traces to display</div>
            <div className="text-xs mt-1 opacity-60">
              Configure OpenTelemetry to start collecting traces
            </div>
          </div>
        </CardContent>
      </Card>

      {}
      <div className="flex items-center justify-between text-[10px] text-muted flex-shrink-0">
        <div className="flex items-center gap-4">
          <span>OpenTelemetry Compatible</span>
          <span>•</span>
          <span>Export to: Datadog, Jaeger, Dash0, Prometheus</span>
        </div>
        <div className="flex items-center gap-2">
          <Clock className="w-3 h-3" />
          <span>Last updated: {new Date().toLocaleTimeString()}</span>
        </div>
      </div>
    </div>
  );
}

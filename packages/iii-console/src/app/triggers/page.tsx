'use client';

import { useEffect, useState } from 'react';
import { Card, CardContent, CardHeader, CardTitle, Badge, Table, TableHeader, TableBody, TableRow, TableHead, TableCell, Button, Input, Select } from "@/components/ui/card";
import { Zap, Search, RefreshCw, Globe, Calendar, MessageSquare, Play, Pause, Eye, EyeOff, Copy, ChevronDown, ChevronRight, ExternalLink, Clock, Hash } from "lucide-react";
import { fetchTriggers } from "@/lib/api";

interface Trigger {
  id: string;
  trigger_type: string;
  function_path: string;
  config: Record<string, unknown>;
  worker_id: string | null;
  internal?: boolean;
}

const TRIGGER_ICONS: Record<string, typeof Zap> = {
  'api': Globe,
  'cron': Calendar,
  'event': MessageSquare,
  'streams:join': Play,
  'streams:leave': Pause,
};

const TRIGGER_DESCRIPTIONS: Record<string, string> = {
  'api': 'HTTP REST endpoint - invoked via HTTP requests',
  'cron': 'Scheduled job - runs on a time-based schedule',
  'event': 'Event listener - triggered by published events',
  'streams:join': 'Stream subscription - triggered when client joins',
  'streams:leave': 'Stream unsubscription - triggered when client leaves',
};

export default function TriggersPage() {
  const [triggers, setTriggers] = useState<Trigger[]>([]);
  const [loading, setLoading] = useState(true);
  const [typeFilter, setTypeFilter] = useState('all');
  const [searchQuery, setSearchQuery] = useState('');
  const [showSystem, setShowSystem] = useState(false);
  const [expandedTriggers, setExpandedTriggers] = useState<Set<string>>(new Set());
  const [copiedId, setCopiedId] = useState<string | null>(null);

  useEffect(() => {
    loadTriggers();
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
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  };

  const copyToClipboard = (text: string, id: string) => {
    navigator.clipboard.writeText(text);
    setCopiedId(id);
    setTimeout(() => setCopiedId(null), 2000);
  };

  const userTriggers = triggers.filter(t => !t.internal);
  const systemTriggers = triggers.filter(t => t.internal);

  const visibleTriggers = showSystem ? triggers : userTriggers;
  const triggerTypes = [...new Set(visibleTriggers.map(t => t.trigger_type))];

  const filteredTriggers = triggers.filter(trigger => {
    if (!showSystem && trigger.internal) return false;
    if (typeFilter !== 'all' && trigger.trigger_type !== typeFilter) return false;
    if (searchQuery && !trigger.function_path?.toLowerCase().includes(searchQuery.toLowerCase())) return false;
    return true;
  });

  const triggersByType = visibleTriggers.reduce((acc, trigger) => {
    acc[trigger.trigger_type] = (acc[trigger.trigger_type] || 0) + 1;
    return acc;
  }, {} as Record<string, number>);

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

  return (
    <div className="p-6 space-y-6">
      {}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-xl font-semibold tracking-tight">Triggers</h1>
          <p className="text-xs text-muted mt-1 tracking-wide">
            Entry points that invoke your functions
            {systemTriggers.length > 0 && !showSystem && (
              <span className="text-muted/60 ml-2">({systemTriggers.length} system hidden)</span>
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
          <Button variant="outline" size="sm" onClick={loadTriggers}>
            <RefreshCw className={`w-3 h-3 mr-2 ${loading ? 'animate-spin' : ''}`} />
            Refresh
          </Button>
        </div>
      </div>

      {}
      <div className="grid gap-4 grid-cols-2 lg:grid-cols-5">
        {Object.entries(triggersByType).map(([type, count]) => {
          const Icon = TRIGGER_ICONS[type] || Zap;
          const description = TRIGGER_DESCRIPTIONS[type] || 'Custom trigger type';
          
          return (
            <Card 
              key={type}
              className={`card-interactive cursor-pointer ${typeFilter === type ? 'border-accent' : ''}`}
              onClick={() => setTypeFilter(typeFilter === type ? 'all' : type)}
            >
              <CardContent className="p-4">
                <div className="flex items-center justify-between mb-2">
                  <Icon className="w-4 h-4 text-muted" />
                  <Badge variant="outline">{count}</Badge>
                </div>
                <div className="text-xs font-medium mb-1">{type.toUpperCase()}</div>
                <div className="text-[10px] text-muted leading-relaxed">{description}</div>
              </CardContent>
            </Card>
          );
        })}
        
        {triggerTypes.length === 0 && !loading && (
          <Card className="col-span-full">
            <CardContent className="p-8 text-center">
              <Zap className="w-8 h-8 text-muted mx-auto mb-3" />
              <div className="text-sm mb-1">No triggers registered</div>
              <div className="text-xs text-muted">
                Register triggers using the SDK to expose your functions
              </div>
            </CardContent>
          </Card>
        )}
      </div>

      {}
      <Card>
        <CardHeader className="flex flex-row items-center justify-between">
          <CardTitle>Registered Triggers ({filteredTriggers.length})</CardTitle>
          <div className="flex items-center gap-2">
            <div className="relative">
              <Search className="w-3 h-3 absolute left-3 top-1/2 -translate-y-1/2 text-muted" />
              <Input 
                placeholder="Search function..."
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                className="pl-8 w-48"
              />
            </div>
            <Select 
              value={typeFilter}
              onChange={(e) => setTypeFilter(e.target.value)}
            >
              <option value="all">All Types</option>
              {triggerTypes.map(type => (
                <option key={type} value={type}>{type}</option>
              ))}
            </Select>
          </div>
        </CardHeader>
        <CardContent>
          {loading ? (
            <div className="text-xs text-muted py-8 text-center">Loading triggers...</div>
          ) : filteredTriggers.length === 0 ? (
            <div className="text-xs text-muted py-8 text-center border border-dashed border-border rounded">
              No triggers found
            </div>
          ) : (
            <div className="space-y-2">
              {filteredTriggers.map((trigger) => {
                const Icon = TRIGGER_ICONS[trigger.trigger_type] || Zap;
                const isExpanded = expandedTriggers.has(trigger.id);
                const apiEndpoint = getApiEndpoint(trigger);
                const cronSchedule = getCronSchedule(trigger);
                
                return (
                  <div 
                    key={trigger.id} 
                    className="border border-border rounded-lg overflow-hidden hover:border-muted/50 transition-colors"
                  >
                    {}
                    <div 
                      className="flex items-center gap-4 p-4 cursor-pointer hover:bg-dark-gray/30"
                      onClick={() => toggleExpand(trigger.id)}
                    >
                      <button className="flex-shrink-0">
                        {isExpanded ? (
                          <ChevronDown className="w-4 h-4 text-muted" />
                        ) : (
                          <ChevronRight className="w-4 h-4 text-muted" />
                        )}
                      </button>
                      
                      <div className="flex items-center gap-2 flex-shrink-0 w-20">
                        <Icon className="w-4 h-4 text-muted" />
                        <Badge variant="outline" className="text-[10px]">
                          {trigger.trigger_type.toUpperCase()}
                        </Badge>
                      </div>
                      
                      <div className="flex-1 min-w-0">
                        <div className="font-mono text-sm font-medium truncate">
                          {trigger.function_path || '—'}
                        </div>
                        {apiEndpoint && (
                          <div className="text-xs text-muted font-mono mt-0.5 flex items-center gap-2">
                            <Globe className="w-3 h-3" />
                            <span className="text-cyan-400">{apiEndpoint}</span>
                          </div>
                        )}
                        {cronSchedule && (
                          <div className="text-xs text-muted font-mono mt-0.5 flex items-center gap-2">
                            <Clock className="w-3 h-3" />
                            <span className="text-yellow">{cronSchedule}</span>
                          </div>
                        )}
                      </div>
                      
                      <div className="flex items-center gap-3">
                        {trigger.worker_id && (
                          <div className="text-[10px] text-muted font-mono bg-dark-gray px-2 py-1 rounded">
                            Worker: {trigger.worker_id.slice(0, 8)}
                          </div>
                        )}
                        <Badge variant="success">ACTIVE</Badge>
                      </div>
                    </div>
                    
                    {}
                    {isExpanded && (
                      <div className="border-t border-border bg-dark-gray/20 p-4 space-y-4">
                        {}
                        <div className="flex items-start gap-4">
                          <div className="w-24 text-[10px] text-muted uppercase tracking-wider flex-shrink-0 pt-1">
                            Trigger ID
                          </div>
                          <div className="flex-1 flex items-center gap-2">
                            <code className="text-xs font-mono bg-black/40 px-3 py-1.5 rounded flex-1">
                              {trigger.id}
                            </code>
                            <button 
                              onClick={(e) => { e.stopPropagation(); copyToClipboard(trigger.id, trigger.id); }}
                              className="p-1.5 hover:bg-dark-gray rounded text-muted hover:text-foreground"
                            >
                              <Copy className="w-3 h-3" />
                            </button>
                            {copiedId === trigger.id && (
                              <span className="text-[10px] text-success">Copied!</span>
                            )}
                          </div>
                        </div>
                        
                        {}
                        <div className="flex items-start gap-4">
                          <div className="w-24 text-[10px] text-muted uppercase tracking-wider flex-shrink-0 pt-1">
                            Function
                          </div>
                          <div className="flex-1">
                            <code className="text-xs font-mono bg-black/40 px-3 py-1.5 rounded inline-block">
                              {trigger.function_path}
                            </code>
                          </div>
                        </div>
                        
                        {}
                        {trigger.trigger_type === 'api' && (
                          <>
                            <div className="flex items-start gap-4">
                              <div className="w-24 text-[10px] text-muted uppercase tracking-wider flex-shrink-0 pt-1">
                                Endpoint
                              </div>
                              <div className="flex-1 flex items-center gap-2">
                                <code className="text-xs font-mono bg-cyan-500/10 text-cyan-400 px-3 py-1.5 rounded border border-cyan-500/30">
                                  {apiEndpoint}
                                </code>
                                <a 
                                  href={`http://localhost:3111/${(trigger.config as { api_path?: string }).api_path || ''}`}
                                  target="_blank"
                                  rel="noopener noreferrer"
                                  className="p-1.5 hover:bg-dark-gray rounded text-muted hover:text-foreground"
                                  onClick={(e) => e.stopPropagation()}
                                >
                                  <ExternalLink className="w-3 h-3" />
                                </a>
                              </div>
                            </div>
                            <div className="flex items-start gap-4">
                              <div className="w-24 text-[10px] text-muted uppercase tracking-wider flex-shrink-0 pt-1">
                                Full URL
                              </div>
                              <div className="flex-1">
                                <code className="text-[10px] font-mono text-muted bg-black/40 px-3 py-1.5 rounded inline-block">
                                  http://localhost:3111/{(trigger.config as { api_path?: string }).api_path || ''}
                                </code>
                              </div>
                            </div>
                          </>
                        )}
                        
                        {}
                        <div className="flex items-start gap-4">
                          <div className="w-24 text-[10px] text-muted uppercase tracking-wider flex-shrink-0 pt-1">
                            Config
                          </div>
                          <div className="flex-1">
                            <pre className="text-[10px] font-mono bg-black/40 px-3 py-2 rounded overflow-x-auto text-muted">
                              {JSON.stringify(trigger.config, null, 2)}
                            </pre>
                          </div>
                        </div>
                        
                        {}
                        {trigger.worker_id && (
                          <div className="flex items-start gap-4">
                            <div className="w-24 text-[10px] text-muted uppercase tracking-wider flex-shrink-0 pt-1">
                              Worker
                            </div>
                            <div className="flex-1 flex items-center gap-2">
                              <Hash className="w-3 h-3 text-muted" />
                              <code className="text-xs font-mono">
                                {trigger.worker_id}
                              </code>
                            </div>
                          </div>
                        )}
                      </div>
                    )}
                  </div>
                );
              })}
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  );
}

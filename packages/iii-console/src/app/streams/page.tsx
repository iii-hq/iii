'use client';

import { useState, useEffect, useCallback, useMemo, useRef } from 'react';
import { Card, CardContent, CardHeader, CardTitle, Badge, Button, Input } from "@/components/ui/card";
import { 
  Layers, Search, RefreshCw, Radio, Database, Wifi, Activity, Eye, EyeOff,
  ChevronDown, ChevronRight, X, FileCode, Folder, Key, Server, Unlock, Lock,
  Zap, WifiOff, Plus, Send, Trash2, Copy, Check, Clock, ArrowRight, Play, 
  Pause, MoreHorizontal
} from "lucide-react";
import { fetchStreams, StreamInfo, connectToStreams, StreamMessage } from "@/lib/api";

interface GroupInfo {
  id: string;
  count: number;
  loading: boolean;
}

interface StreamItem {
  key: string;
  value: unknown;
  type: string;
  timestamp?: number;
}

interface StreamEntry {
  id: string;
  data: unknown;
  timestamp: number;
}

const DEVTOOLS_API = 'http://localhost:3111/_console';
const STREAMS_WS = 'ws://localhost:31112';

export default function StreamsPage() {
  const [streams, setStreams] = useState<StreamInfo[]>([]);
  const [websocketPort, setWebsocketPort] = useState<number>(31112);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState('');
  const [wsConnected, setWsConnected] = useState(false);
  const [showSystem, setShowSystem] = useState(false);
  const [selectedStreamId, setSelectedStreamId] = useState<string | null>(null);
  const [selectedGroupId, setSelectedGroupId] = useState<string | null>(null);
  const [groups, setGroups] = useState<GroupInfo[]>([]);
  const [loadingGroups, setLoadingGroups] = useState(false);
  const [items, setItems] = useState<StreamItem[]>([]);
  const [loadingItems, setLoadingItems] = useState(false);
  const [expandedGroups, setExpandedGroups] = useState<Set<string>>(new Set(['User Streams']));
  
  const [liveEntries, setLiveEntries] = useState<StreamEntry[]>([]);
  const [isLive, setIsLive] = useState(true);
  const [showPublishModal, setShowPublishModal] = useState(false);
  const [publishKey, setPublishKey] = useState('');
  const [publishValue, setPublishValue] = useState('');
  const [publishing, setPublishing] = useState(false);
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const [expandedEntries, setExpandedEntries] = useState<Set<string>>(new Set());
  
  const wsRef = useRef<WebSocket | null>(null);
  const subscriptionIdRef = useRef<string>(`console-${Date.now()}-${Math.random().toString(36).slice(2)}`);

  const loadData = async () => {
    setLoading(true);
    setError(null);
    try {
      const data = await fetchStreams();
      setStreams(data.streams);
      setWebsocketPort(data.websocket_port);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load streams');
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    loadData();
  }, []);

  useEffect(() => {
    let disconnect: (() => void) | null = null;
    let mounted = true;

    const timer = setTimeout(() => {
      if (!mounted) return;
      disconnect = connectToStreams({
        onConnect: () => mounted && setWsConnected(true),
        onDisconnect: () => mounted && setWsConnected(false),
        onMessage: () => {}
      });
    }, 100);

    return () => {
      mounted = false;
      clearTimeout(timer);
      disconnect?.();
    };
  }, []);

  useEffect(() => {
    if (!selectedStreamId || !selectedGroupId || !isLive) return;

    let ws: WebSocket | null = null;
    let mounted = true;

    const connect = () => {
      if (!mounted) return;

      try {
        ws = new WebSocket(STREAMS_WS);
        wsRef.current = ws;

        ws.onopen = () => {
          if (!mounted || !ws) return;
          
          const joinMessage = {
            type: 'join',
            data: {
              subscriptionId: subscriptionIdRef.current,
              streamName: selectedStreamId,
              groupId: selectedGroupId,
            }
          };
          ws.send(JSON.stringify(joinMessage));
        };

        ws.onmessage = (event) => {
          if (!mounted) return;
          try {
            const message: StreamMessage = JSON.parse(event.data);

            if (message.streamName !== selectedStreamId || message.groupId !== selectedGroupId) {
              return;
            }

            switch (message.event.type) {
              case 'sync':
                if (message.event.data && Array.isArray(message.event.data)) {
                  const entries = message.event.data.map((d: unknown, i: number) => ({
                    id: `${message.timestamp}-${i}`,
                    data: d,
                    timestamp: message.timestamp,
                  }));
                  setLiveEntries(entries);
                }
                break;

              case 'create':
              case 'update':
                if (message.event.data) {
                  const entry: StreamEntry = {
                    id: message.id || `${message.timestamp}-${Math.random().toString(36).slice(2)}`,
                    data: message.event.data,
                    timestamp: message.timestamp,
                  };
                  setLiveEntries(prev => [entry, ...prev].slice(0, 100));
                }
                break;

              case 'delete':
                if (message.id) {
                  setLiveEntries(prev => prev.filter(e => e.id !== message.id));
                }
                break;
            }
          } catch {
          }
        };

        ws.onclose = () => {
          wsRef.current = null;
        };
      } catch {
      }
    };

    const timer = setTimeout(connect, 100);

    return () => {
      mounted = false;
      clearTimeout(timer);
      if (ws && ws.readyState !== WebSocket.CLOSED) {
        try {
          const leaveMessage = {
            type: 'leave',
            data: {
              subscriptionId: subscriptionIdRef.current,
              streamName: selectedStreamId,
              groupId: selectedGroupId,
            }
          };
          if (ws.readyState === WebSocket.OPEN) {
            ws.send(JSON.stringify(leaveMessage));
          }
          ws.close();
        } catch {
        }
      }
      wsRef.current = null;
    };
  }, [selectedStreamId, selectedGroupId, isLive]);

  const groupedStreams = useMemo(() => {
    const filtered = streams.filter(s => {
      if (!showSystem && s.internal) return false;
      if (searchQuery && !s.id.toLowerCase().includes(searchQuery.toLowerCase())) return false;
      return true;
    });

    const grouped: Record<string, StreamInfo[]> = {};
    filtered.forEach(stream => {
      const groupName = stream.internal ? 'Internal' : 'User Streams';
      if (!grouped[groupName]) grouped[groupName] = [];
      grouped[groupName].push(stream);
    });

    return grouped;
  }, [streams, showSystem, searchQuery]);

  const selectedStream = streams.find(s => s.id === selectedStreamId);

  const discoverGroups = useCallback(async (streamName: string) => {
    setLoadingGroups(true);
    setGroups([]);
    setSelectedGroupId(null);
    setLiveEntries([]);
    
    try {
      const response = await fetch(`${DEVTOOLS_API}/streams/groups`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ stream_name: streamName })
      });
      
      if (response.ok) {
        const data = await response.json();
        const apiGroups = data.data?.groups || data.groups || [];
        
        const discoveredGroups: GroupInfo[] = apiGroups.map((g: { id: string; count: number }) => ({
          id: g.id,
          count: g.count || 0,
          loading: false,
        }));
        
        discoveredGroups.sort((a, b) => b.count - a.count);

        setGroups(discoveredGroups);
        setLoadingGroups(false);

        const firstWithItems = discoveredGroups.find(g => g.count > 0);
        if (firstWithItems) {
          setSelectedGroupId(firstWithItems.id);
          loadGroupItems(streamName, firstWithItems.id);
        } else if (discoveredGroups.length > 0) {
          setSelectedGroupId(discoveredGroups[0].id);
        }
      } else {
        setLoadingGroups(false);
      }
    } catch {
      setLoadingGroups(false);
    }
  }, []);

  const loadGroupItems = async (streamName: string, groupId: string) => {
    setLoadingItems(true);
    setLiveEntries([]);
    try {
      const response = await fetch(`${DEVTOOLS_API}/streams/group`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ stream_name: streamName, group_id: groupId })
      });
      if (response.ok) {
        const data = await response.json();
        const rawItems = data.data?.items || data.items || [];
        const mappedItems = rawItems.map((item: unknown, index: number) => ({
          key: (item as { id?: string })?.id || `item-${index}`,
          value: item,
          type: typeof item,
          timestamp: Date.now()
        }));
        setItems(mappedItems);
        
        const entries = rawItems.map((item: unknown, index: number) => ({
          id: (item as { id?: string })?.id || `entry-${index}`,
          data: item,
          timestamp: Date.now()
        }));
        setLiveEntries(entries);
      }
    } catch {
      setItems([]);
      setLiveEntries([]);
    } finally {
      setLoadingItems(false);
    }
  };

  useEffect(() => {
    if (selectedStreamId) {
      discoverGroups(selectedStreamId);
    }
  }, [selectedStreamId, discoverGroups]);

  const handleSelectStream = (streamId: string) => {
    setSelectedStreamId(selectedStreamId === streamId ? null : streamId);
    setLiveEntries([]);
  };

  const handleSelectGroup = (groupId: string) => {
    setSelectedGroupId(groupId);
    setLiveEntries([]);
    if (selectedStreamId) {
      loadGroupItems(selectedStreamId, groupId);
    }
  };

  const toggleGroupExpand = (groupName: string) => {
    setExpandedGroups(prev => {
      const next = new Set(prev);
      if (next.has(groupName)) next.delete(groupName);
      else next.add(groupName);
      return next;
    });
  };

  const handlePublish = async () => {
    if (!selectedStreamId || !selectedGroupId || !publishKey) return;
    
    setPublishing(true);
    try {
      let value: unknown = publishValue;
      try {
        value = JSON.parse(publishValue);
      } catch {
      }

      const response = await fetch(
        `${DEVTOOLS_API}/streams/${encodeURIComponent(selectedStreamId)}/group/${encodeURIComponent(selectedGroupId)}`,
        {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ key: publishKey, value }),
        }
      );

      if (response.ok) {
        setPublishKey('');
        setPublishValue('');
        setShowPublishModal(false);
        loadGroupItems(selectedStreamId, selectedGroupId);
      }
    } catch {
    } finally {
      setPublishing(false);
    }
  };

  const handleDeleteEntry = async (entryKey: string) => {
    if (!selectedStreamId || !selectedGroupId) return;
    
    try {
      const response = await fetch(
        `${DEVTOOLS_API}/streams/${encodeURIComponent(selectedStreamId)}/group/${encodeURIComponent(selectedGroupId)}/${encodeURIComponent(entryKey)}`,
        { method: 'DELETE' }
      );

      if (response.ok) {
        loadGroupItems(selectedStreamId, selectedGroupId);
      }
    } catch {
    }
  };

  const copyToClipboard = async (text: string, id: string) => {
    await navigator.clipboard.writeText(text);
    setCopiedId(id);
    setTimeout(() => setCopiedId(null), 2000);
  };

  const toggleEntryExpand = (entryId: string) => {
    setExpandedEntries(prev => {
      const next = new Set(prev);
      if (next.has(entryId)) next.delete(entryId);
      else next.add(entryId);
      return next;
    });
  };

  const formatTime = (timestamp: number) => {
    const date = new Date(timestamp * 1000);
    return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' });
  };

  const userStreams = streams.filter(s => !s.internal);
  const systemStreams = streams.filter(s => s.internal);

  return (
    <div className="flex flex-col h-full bg-background text-foreground">
      <div className="flex items-center justify-between px-5 py-3 bg-dark-gray/30 border-b border-border">
        <div className="flex items-center gap-4">
          <h1 className="text-base font-semibold flex items-center gap-2">
            <Database className="w-4 h-4 text-purple-400" />
            Streams
          </h1>
          <Badge 
            variant={wsConnected ? 'success' : 'error'}
            className="gap-1.5"
          >
            {wsConnected ? <Wifi className="w-3 h-3" /> : <WifiOff className="w-3 h-3" />}
            {wsConnected ? 'Connected' : 'Disconnected'}
          </Badge>
          <div className="flex items-center gap-2 px-2 py-1 rounded bg-dark-gray/50 text-xs text-muted">
            <Database className="w-3 h-3" />
            <span>{userStreams.length} streams</span>
            {showSystem && systemStreams.length > 0 && (
              <>
                <span className="text-border">|</span>
                <span className="text-muted/60">{systemStreams.length} system</span>
              </>
            )}
          </div>
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
            className="h-7 text-xs text-muted hover:text-foreground"
          >
            <RefreshCw className={`w-3.5 h-3.5 mr-1.5 ${loading ? 'animate-spin' : ''}`} />
            Refresh
          </Button>
        </div>
      </div>

      {error && (
        <div className="mx-4 mt-4 bg-error/10 border border-error/30 rounded px-4 py-3 text-sm text-error">
          {error}
        </div>
      )}

      <div className={`flex-1 grid overflow-hidden ${selectedStreamId ? 'grid-cols-[280px_1fr]' : 'grid-cols-1'}`}>
        <div className="flex flex-col h-full overflow-hidden border-r border-border bg-dark-gray/20">
          <div className="p-3 border-b border-border">
            <div className="relative">
              <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-muted" />
              <Input
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                placeholder="Search streams..."
                className="pl-8 h-8 text-xs"
              />
            </div>
          </div>

          <div className="flex-1 overflow-y-auto py-2">
            {loading ? (
              <div className="flex items-center justify-center h-32">
                <RefreshCw className="w-5 h-5 text-muted animate-spin" />
              </div>
            ) : Object.keys(groupedStreams).length === 0 ? (
              <div className="flex flex-col items-center justify-center h-64 px-4">
                <div className="w-12 h-12 mb-3 rounded-xl bg-dark-gray border border-border flex items-center justify-center">
                  <Database className="w-6 h-6 text-muted" />
                </div>
                <div className="text-sm font-medium mb-1">No streams found</div>
                <div className="text-xs text-muted text-center">
                  Create streams using the SDK to store real-time data
                </div>
              </div>
            ) : (
              Object.entries(groupedStreams)
                .sort(([a], [b]) => (a === 'Internal' ? 1 : b === 'Internal' ? -1 : a.localeCompare(b)))
                .map(([groupName, groupStreams]) => (
                  <div key={groupName} className="mb-1">
                    <button
                      onClick={() => toggleGroupExpand(groupName)}
                      className="w-full flex items-center gap-2 px-3 py-2 text-xs font-semibold text-muted uppercase tracking-wider hover:bg-dark-gray/30 transition-colors"
                    >
                      {expandedGroups.has(groupName) ? (
                        <ChevronDown className="w-3.5 h-3.5" />
                      ) : (
                        <ChevronRight className="w-3.5 h-3.5" />
                      )}
                      <span>{groupName}</span>
                      <span className="ml-auto text-[10px] bg-dark-gray px-1.5 py-0.5 rounded-full">
                        {groupStreams.length}
                      </span>
                    </button>

                    {expandedGroups.has(groupName) && (
                      <div className="space-y-0.5 pb-2">
                        {groupStreams.map(stream => (
                          <button
                            key={stream.id}
                            onClick={() => handleSelectStream(stream.id)}
                            className={`w-full flex items-center gap-2 px-3 py-2 mx-2 text-left transition-colors rounded-md
                              ${selectedStreamId === stream.id 
                                ? 'bg-primary/10 text-primary border-l-2 border-primary' 
                                : 'text-foreground/80 hover:bg-dark-gray/50'
                              }
                            `}
                            style={{ width: 'calc(100% - 16px)' }}
                          >
                            <Database className={`h-4 w-4 shrink-0 ${selectedStreamId === stream.id ? 'text-primary' : 'text-muted'}`} />
                            <div className="flex-1 min-w-0">
                              <div className={`text-sm font-medium truncate ${selectedStreamId === stream.id ? 'text-primary' : ''}`}>
                                {stream.id}
                              </div>
                              {stream.description && (
                                <div className="text-xs text-muted truncate">{stream.description}</div>
                              )}
                            </div>
                            {stream.groups.length > 0 && (
                              <span className="text-[10px] text-muted bg-dark-gray px-1.5 py-0.5 rounded">
                                {stream.groups.length}
                              </span>
                            )}
                          </button>
                        ))}
                      </div>
                    )}
                  </div>
                ))
            )}
          </div>

          <div className="p-3 border-t border-border text-xs text-muted">
            <div className="flex items-center justify-between">
              <span>WebSocket</span>
              <code className="font-mono">:{websocketPort}</code>
            </div>
          </div>
        </div>

        {selectedStream && (
          <div className="flex flex-col h-full overflow-hidden bg-background">
            <div className="flex items-center justify-between px-4 py-3 border-b border-border bg-dark-gray/30">
              <div className="flex items-center gap-3 min-w-0">
                <div className="p-2 rounded-lg bg-primary/10">
                  <Database className="h-5 w-5 text-primary" />
                </div>
                <div className="min-w-0">
                  <h2 className="text-base font-semibold text-foreground truncate">{selectedStream.id}</h2>
                  <div className="flex items-center gap-1.5 text-xs text-muted">
                    <Layers className="h-3 w-3" />
                    <span>{selectedStream.type}</span>
                    {selectedGroupId && (
                      <>
                        <ArrowRight className="h-3 w-3" />
                        <span className="text-primary">{selectedGroupId}</span>
                      </>
                    )}
                  </div>
                </div>
              </div>

              <div className="flex items-center gap-2">
                <Button
                  variant={isLive ? "accent" : "ghost"}
                  size="sm"
                  onClick={() => setIsLive(!isLive)}
                  className="h-7 text-xs gap-1.5"
                >
                  {isLive ? <Pause className="w-3 h-3" /> : <Play className="w-3 h-3" />}
                  {isLive ? 'Live' : 'Paused'}
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => selectedStreamId && discoverGroups(selectedStreamId)}
                  disabled={loadingGroups}
                  className="h-7 w-7 p-0"
                >
                  <RefreshCw className={`w-4 h-4 ${loadingGroups ? 'animate-spin' : ''}`} />
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => setSelectedStreamId(null)}
                  className="h-7 w-7 p-0"
                >
                  <X className="w-4 h-4" />
                </Button>
              </div>
            </div>

            <div className="grid grid-cols-4 gap-3 px-4 py-3 border-b border-border bg-dark-gray/20">
              <div className="flex items-center gap-3 p-3 rounded-lg bg-background border border-border">
                <div className="p-2 rounded-md bg-blue-500/10">
                  <Server className="h-4 w-4 text-blue-400" />
                </div>
                <div>
                  <div className="text-xs text-muted">Storage</div>
                  <div className="text-sm font-medium">Redis</div>
                </div>
              </div>

              <div className="flex items-center gap-3 p-3 rounded-lg bg-background border border-border">
                <div className="p-2 rounded-md bg-purple-500/10">
                  <Layers className="h-4 w-4 text-purple-400" />
                </div>
                <div>
                  <div className="text-xs text-muted">Type</div>
                  <div className="text-sm font-medium capitalize">{selectedStream.type}</div>
                </div>
              </div>

              <div className="flex items-center gap-3 p-3 rounded-lg bg-background border border-border">
                <div className="p-2 rounded-md bg-amber-500/10">
                  <Folder className="h-4 w-4 text-amber-400" />
                </div>
                <div>
                  <div className="text-xs text-muted">Groups</div>
                  <div className="text-sm font-medium">{groups.length}</div>
                </div>
              </div>

              <div className="flex items-center gap-3 p-3 rounded-lg bg-background border border-border">
                <div className={`p-2 rounded-md ${isLive ? 'bg-success/10' : 'bg-dark-gray'}`}>
                  <Activity className={`h-4 w-4 ${isLive ? 'text-success' : 'text-muted'}`} />
                </div>
                <div>
                  <div className="text-xs text-muted">Status</div>
                  <div className={`text-sm font-medium ${isLive ? 'text-success' : 'text-muted'}`}>
                    {isLive ? 'Live' : 'Paused'}
                  </div>
                </div>
              </div>
            </div>

            <div className="px-4 py-3 border-b border-border bg-dark-gray/20">
              <div className="flex items-center justify-between mb-3">
                <div className="flex items-center gap-2">
                  <Folder className="h-4 w-4 text-muted" />
                  <h3 className="text-sm font-medium">Groups</h3>
                  {loadingGroups && <RefreshCw className="w-3.5 h-3.5 text-muted animate-spin" />}
                </div>
                <div className="text-xs text-muted">
                  {groups.filter(g => g.count > 0).length} active / {groups.length} total
                </div>
              </div>

              <div className="flex flex-wrap gap-2">
                {groups.map(group => (
                  <button
                    key={group.id}
                    onClick={() => handleSelectGroup(group.id)}
                    className={`flex items-center gap-2 px-3 py-2 rounded-md text-sm transition-all
                      ${selectedGroupId === group.id
                        ? 'bg-primary text-primary-foreground shadow-md'
                        : group.count > 0
                          ? 'bg-dark-gray text-foreground hover:bg-dark-gray/80'
                          : 'bg-dark-gray/50 text-muted hover:bg-dark-gray/80'
                      }
                    `}
                  >
                    {group.count > 0 && (
                      <Zap className={`w-3 h-3 ${selectedGroupId === group.id ? '' : 'text-primary'}`} />
                    )}
                    <span className="font-medium capitalize">{group.id}</span>
                    <span className={`text-xs px-1.5 py-0.5 rounded-full min-w-[20px] text-center
                      ${selectedGroupId === group.id
                        ? 'bg-primary-foreground/20'
                        : group.count > 0
                          ? 'bg-primary/20 text-primary'
                          : 'bg-background'
                      }
                    `}>
                      {group.count}
                    </span>
                  </button>
                ))}

                {groups.length === 0 && !loadingGroups && (
                  <p className="text-sm text-muted">No groups found. Write data to create groups.</p>
                )}
              </div>
            </div>

            <div className="flex items-center justify-between px-4 py-2 border-b border-border bg-dark-gray/10">
              <div className="flex items-center gap-2 text-xs text-muted">
                <Clock className="w-3.5 h-3.5" />
                <span>
                  {liveEntries.length > 0 
                    ? `${liveEntries.length} live entries` 
                    : items.length > 0 
                      ? `${items.length} entries`
                      : 'No entries'
                  }
                </span>
                {isLive && liveEntries.length > 0 && (
                  <span className="flex items-center gap-1 text-success">
                    <span className="w-1.5 h-1.5 rounded-full bg-success animate-pulse" />
                    Streaming
                  </span>
                )}
              </div>

              <Button
                variant="outline"
                size="sm"
                onClick={() => setShowPublishModal(true)}
                className="h-7 text-xs gap-1.5"
                disabled={!selectedGroupId}
              >
                <Plus className="w-3 h-3" />
                Add Entry
              </Button>
            </div>

            <div className="flex-1 overflow-auto">
              {loadingItems ? (
                <div className="flex items-center justify-center h-32">
                  <RefreshCw className="w-5 h-5 text-muted animate-spin" />
                </div>
              ) : selectedGroupId ? (
                (liveEntries.length > 0 || items.length > 0) ? (
                  <div className="divide-y divide-border/50">
                    {(liveEntries.length > 0 ? liveEntries : items.map((item, i) => ({
                      id: item.key || `item-${i}`,
                      data: { key: item.key, value: item.value, type: item.type },
                      timestamp: item.timestamp || Date.now() / 1000,
                    }))).map((entry) => (
                      <div 
                        key={entry.id} 
                        className="group hover:bg-dark-gray/30 transition-colors"
                      >
                        <div 
                          className="flex items-center gap-3 px-4 py-3 cursor-pointer"
                          onClick={() => toggleEntryExpand(entry.id)}
                        >
                          <div className="flex items-center gap-2 min-w-0 flex-1">
                            {expandedEntries.has(entry.id) ? (
                              <ChevronDown className="w-4 h-4 text-muted shrink-0" />
                            ) : (
                              <ChevronRight className="w-4 h-4 text-muted shrink-0" />
                            )}
                            <Key className="w-4 h-4 text-primary shrink-0" />
                            <span className="font-mono text-sm font-medium truncate">
                              {typeof entry.data === 'object' && entry.data && 'key' in entry.data 
                                ? (entry.data as { key: string }).key 
                                : entry.id
                              }
                            </span>
                          </div>

                          <div className="flex items-center gap-2">
                            <Badge variant="outline" className="text-[10px] shrink-0">
                              {typeof entry.data === 'object' && entry.data && 'type' in entry.data 
                                ? String((entry.data as { type: string }).type)
                                : typeof entry.data
                              }
                            </Badge>
                            <span className="text-xs text-muted tabular-nums shrink-0">
                              {formatTime(entry.timestamp)}
                            </span>
                            <div className="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
                              <button
                                onClick={(e) => {
                                  e.stopPropagation();
                                  copyToClipboard(JSON.stringify(entry.data, null, 2), entry.id);
                                }}
                                className="p-1 rounded hover:bg-dark-gray"
                                title="Copy JSON"
                              >
                                {copiedId === entry.id ? (
                                  <Check className="w-3.5 h-3.5 text-success" />
                                ) : (
                                  <Copy className="w-3.5 h-3.5 text-muted" />
                                )}
                              </button>
                              <button
                                onClick={(e) => {
                                  e.stopPropagation();
                                  const key = typeof entry.data === 'object' && entry.data && 'key' in entry.data 
                                    ? (entry.data as { key: string }).key 
                                    : entry.id;
                                  handleDeleteEntry(key);
                                }}
                                className="p-1 rounded hover:bg-dark-gray"
                                title="Delete entry"
                              >
                                <Trash2 className="w-3.5 h-3.5 text-error" />
                              </button>
                            </div>
                          </div>
                        </div>

                        {expandedEntries.has(entry.id) && (
                          <div className="px-4 pb-3 pl-12">
                            <pre className="p-3 rounded-lg bg-dark-gray text-xs font-mono overflow-x-auto text-muted">
                              {JSON.stringify(
                                typeof entry.data === 'object' && entry.data && 'value' in entry.data 
                                  ? (entry.data as { value: unknown }).value 
                                  : entry.data, 
                                null, 
                                2
                              )}
                            </pre>
                          </div>
                        )}
                      </div>
                    ))}
                  </div>
                ) : (
                  <div className="flex flex-col items-center justify-center h-64">
                    <div className="w-12 h-12 mb-3 rounded-xl bg-dark-gray border border-border flex items-center justify-center">
                      <Folder className="h-6 w-6 text-muted" />
                    </div>
                    <p className="text-sm font-medium mb-1">No entries in this group</p>
                    <p className="text-xs text-muted mb-4">Write data to see it appear here</p>
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => setShowPublishModal(true)}
                      className="gap-1.5"
                    >
                      <Plus className="w-3 h-3" />
                      Add Entry
                    </Button>
                  </div>
                )
              ) : (
                <div className="flex flex-col items-center justify-center h-64">
                  <Folder className="h-12 w-12 text-muted/50 mb-4" />
                  <p className="text-sm text-muted">Select a group to view entries</p>
                </div>
              )}
            </div>
          </div>
        )}
      </div>

      {showPublishModal && (
        <div className="fixed inset-0 bg-background/80 backdrop-blur-sm flex items-center justify-center z-50">
          <div className="bg-background border border-border rounded-lg shadow-xl w-full max-w-md">
            <div className="flex items-center justify-between px-4 py-3 border-b border-border">
              <h3 className="font-semibold">Add Entry to Stream</h3>
              <button
                onClick={() => setShowPublishModal(false)}
                className="p-1 rounded hover:bg-dark-gray"
              >
                <X className="w-4 h-4" />
              </button>
            </div>

            <div className="p-4 space-y-4">
              <div>
                <label className="text-xs text-muted block mb-1.5">Stream / Group</label>
                <div className="flex items-center gap-2 text-sm font-mono bg-dark-gray/50 px-3 py-2 rounded">
                  <Database className="w-4 h-4 text-primary" />
                  <span>{selectedStreamId}</span>
                  <ArrowRight className="w-3 h-3 text-muted" />
                  <span className="text-primary">{selectedGroupId}</span>
                </div>
              </div>

              <div>
                <label className="text-xs text-muted block mb-1.5">Key</label>
                <Input
                  value={publishKey}
                  onChange={(e) => setPublishKey(e.target.value)}
                  placeholder="entry-key"
                  className="font-mono"
                />
              </div>

              <div>
                <label className="text-xs text-muted block mb-1.5">Value (JSON or string)</label>
                <textarea
                  value={publishValue}
                  onChange={(e) => setPublishValue(e.target.value)}
                  placeholder='{"key": "value"}'
                  className="w-full h-32 px-3 py-2 bg-dark-gray border border-border rounded-md font-mono text-sm resize-none focus:outline-none focus:ring-1 focus:ring-primary"
                />
              </div>
            </div>

            <div className="flex items-center justify-end gap-2 px-4 py-3 border-t border-border">
              <Button
                variant="ghost"
                size="sm"
                onClick={() => setShowPublishModal(false)}
              >
                Cancel
              </Button>
              <Button
                variant="accent"
                size="sm"
                onClick={handlePublish}
                disabled={!publishKey || publishing}
                className="gap-1.5"
              >
                {publishing ? (
                  <RefreshCw className="w-3 h-3 animate-spin" />
                ) : (
                  <Send className="w-3 h-3" />
                )}
                Publish
              </Button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

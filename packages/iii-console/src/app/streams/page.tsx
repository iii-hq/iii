'use client';

import { useState, useEffect, useCallback, useMemo, useRef } from 'react';
import { Card, CardContent, CardHeader, CardTitle, Badge, Button, Input } from "@/components/ui/card";
import { 
  Layers, Search, RefreshCw, Wifi, Activity, Eye, EyeOff,
  ChevronDown, ChevronRight, X, Plus, Send, Copy, Check, Clock, 
  Play, Pause, ArrowUpRight, ArrowDownLeft, ArrowLeftRight, Trash2,
  Download, Filter, Zap, WifiOff, Radio, HelpCircle
} from "lucide-react";
import { fetchStreams, StreamInfo, connectToStreams, StreamMessage } from "@/lib/api";
import { Pagination } from "@/components/ui/pagination";
import { JsonViewer } from "@/components/ui/json-viewer";

interface WebSocketMessageEntry {
  id: string;
  timestamp: number;
  direction: 'inbound' | 'outbound' | 'system';
  streamName?: string;
  groupId?: string;
  eventType: string;
  data: unknown;
  size: number;
}

const STREAMS_WS = 'ws://localhost:31112';

const DIRECTION_CONFIG = {
  inbound: {
    icon: ArrowDownLeft,
    color: 'text-cyan-400',
    bg: 'bg-cyan-500/10',
    label: 'Inbound',
    border: 'border-l-cyan-400'
  },
  outbound: {
    icon: ArrowUpRight,
    color: 'text-orange-400',
    bg: 'bg-orange-500/10',
    label: 'Outbound',
    border: 'border-l-orange-400'
  },
  system: {
    icon: Radio,
    color: 'text-muted',
    bg: 'bg-dark-gray/50',
    label: 'System',
    border: 'border-l-muted'
  }
};

// Event type descriptions for user understanding
const EVENT_TYPE_INFO: Record<string, { label: string; description: string; color: string }> = {
  create: {
    label: 'Create',
    description: 'New item added to stream',
    color: 'text-success'
  },
  update: {
    label: 'Update',
    description: 'Existing item modified',
    color: 'text-yellow'
  },
  delete: {
    label: 'Delete',
    description: 'Item removed from stream',
    color: 'text-error'
  },
  sync: {
    label: 'Sync',
    description: 'Initial data sync on subscription',
    color: 'text-purple-400'
  },
  subscribe: {
    label: 'Subscribe',
    description: 'Subscribed to stream updates',
    color: 'text-cyan-400'
  },
  unsubscribe: {
    label: 'Unsubscribe',
    description: 'Unsubscribed from stream',
    color: 'text-muted'
  },
  connected: {
    label: 'Connected',
    description: 'WebSocket connection established',
    color: 'text-success'
  },
  disconnected: {
    label: 'Disconnected',
    description: 'WebSocket connection closed',
    color: 'text-error'
  },
  error: {
    label: 'Error',
    description: 'An error occurred',
    color: 'text-error'
  },
  message: {
    label: 'Message',
    description: 'Generic message',
    color: 'text-foreground'
  },
  sent: {
    label: 'Sent',
    description: 'Message sent to server',
    color: 'text-orange-400'
  }
};

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

function formatTime(timestamp: number): string {
  const date = new Date(timestamp);
  return date.toLocaleTimeString('en-US', { 
    hour12: false, 
    hour: '2-digit', 
    minute: '2-digit', 
    second: '2-digit',
    fractionalSecondDigits: 3
  });
}

export default function StreamsPage() {
  const [streams, setStreams] = useState<StreamInfo[]>([]);
  const [websocketPort, setWebsocketPort] = useState<number>(31112);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState('');
  const [wsConnected, setWsConnected] = useState(false);
  const [showSystem, setShowSystem] = useState(false);
  const [isPaused, setIsPaused] = useState(false);
  
  // Real-time message capture
  const [messages, setMessages] = useState<WebSocketMessageEntry[]>([]);
  const [selectedMessageId, setSelectedMessageId] = useState<string | null>(null);
  const [directionFilter, setDirectionFilter] = useState<'all' | 'inbound' | 'outbound'>('all');
  const [streamFilter, setStreamFilter] = useState<string | null>(null);
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const [autoScroll, setAutoScroll] = useState(true);
  const [showHelp, setShowHelp] = useState(false);
  const [currentPage, setCurrentPage] = useState(1);
  const [pageSize, setPageSize] = useState(50);

  // Stats
  const [stats, setStats] = useState({
    totalMessages: 0,
    inbound: 0,
    outbound: 0,
    totalBytes: 0,
    latency: null as number | null,
    lastPingTime: null as number | null
  });
  
  const wsRef = useRef<WebSocket | null>(null);
  const messagesContainerRef = useRef<HTMLDivElement>(null);
  const messageIdCounter = useRef(0);
  const subscriptionsRef = useRef<Set<string>>(new Set());
  
  // Streams to auto-subscribe to
  const [subscribedStreams, setSubscribedStreams] = useState<string[]>([]);
  const [showSubscribeModal, setShowSubscribeModal] = useState(false);
  const [newStreamName, setNewStreamName] = useState('');

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

  // Connect to WebSocket and capture all messages
  useEffect(() => {
    let ws: WebSocket | null = null;
    let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
    let pingInterval: ReturnType<typeof setInterval> | null = null;
    let mounted = true;

    const connect = () => {
      if (!mounted) return;

      try {
        ws = new WebSocket(STREAMS_WS);
        wsRef.current = ws;

        ws.onopen = () => {
          if (!mounted) return;
          setWsConnected(true);
          
          // Add system message
          addMessage({
            direction: 'system',
            eventType: 'connected',
            data: { message: 'WebSocket connection established', url: STREAMS_WS },
          });

          // Auto-subscribe to default streams
          const defaultStreams = [
            'iii.logs',           // Application logs
            'iii:devtools:state', // DevTools metrics
          ];
          
          defaultStreams.forEach(streamName => {
            subscribeToStreamInternal(ws, streamName, 'all');
          });
          setSubscribedStreams(defaultStreams);

          // Start ping interval for latency measurement
          pingInterval = setInterval(() => {
            if (ws?.readyState === WebSocket.OPEN) {
              setStats(prev => ({ ...prev, lastPingTime: Date.now() }));
              ws.send(JSON.stringify({ type: 'ping', timestamp: Date.now() }));
            }
          }, 5000);
        };
        
        const subscribeToStreamInternal = (socket: WebSocket, streamName: string, groupId: string) => {
          const subscriptionId = `console-${Date.now()}-${Math.random().toString(36).slice(2)}`;
          const message = {
            type: 'join',
            data: { streamName, groupId, subscriptionId }
          };
          socket.send(JSON.stringify(message));
          subscriptionsRef.current.add(streamName);
          
          addMessage({
            direction: 'outbound',
            streamName,
            groupId,
            eventType: 'subscribe',
            data: message,
          });
        };

        ws.onmessage = (event) => {
          if (!mounted || isPaused) return;
          
          try {
            const data = JSON.parse(event.data);
            
            // Handle pong for latency
            if (data.type === 'pong' && stats.lastPingTime) {
              const latency = Date.now() - stats.lastPingTime;
              setStats(prev => ({ ...prev, latency }));
              return;
            }

            // Capture the message
            const message = data as StreamMessage;
            addMessage({
              direction: 'inbound',
              streamName: message.streamName,
              groupId: message.groupId,
              eventType: message.event?.type || 'message',
              data: message,
            });
          } catch {
            // Non-JSON message
            addMessage({
              direction: 'inbound',
              eventType: 'raw',
              data: event.data,
            });
          }
        };

        ws.onclose = () => {
          if (!mounted) return;
          setWsConnected(false);
          
          addMessage({
            direction: 'system',
            eventType: 'disconnected',
            data: { message: 'WebSocket connection closed' },
          });

          if (pingInterval) clearInterval(pingInterval);
          
          // Reconnect after delay
          reconnectTimer = setTimeout(connect, 3000);
        };

        ws.onerror = () => {
          addMessage({
            direction: 'system',
            eventType: 'error',
            data: { message: 'WebSocket error occurred' },
          });
        };
      } catch {
        reconnectTimer = setTimeout(connect, 3000);
      }
    };

    const addMessage = (msg: Omit<WebSocketMessageEntry, 'id' | 'timestamp' | 'size'>) => {
      const id = `msg_${Date.now()}_${messageIdCounter.current++}`;
      const dataStr = typeof msg.data === 'string' ? msg.data : JSON.stringify(msg.data);
      const size = new Blob([dataStr]).size;
      
      const entry: WebSocketMessageEntry = {
        id,
        timestamp: Date.now(),
        size,
        ...msg,
      };

      setMessages(prev => {
        const newMessages = [entry, ...prev].slice(0, 1000); // Keep last 1000
        return newMessages;
      });

      setStats(prev => ({
        ...prev,
        totalMessages: prev.totalMessages + 1,
        inbound: msg.direction === 'inbound' ? prev.inbound + 1 : prev.inbound,
        outbound: msg.direction === 'outbound' ? prev.outbound + 1 : prev.outbound,
        totalBytes: prev.totalBytes + size,
      }));
    };

    connect();

    return () => {
      mounted = false;
      if (reconnectTimer) clearTimeout(reconnectTimer);
      if (pingInterval) clearInterval(pingInterval);
      if (ws) ws.close();
    };
  }, [isPaused]);

  // Auto-scroll to top on new messages
  useEffect(() => {
    if (autoScroll && messagesContainerRef.current) {
      messagesContainerRef.current.scrollTop = 0;
    }
  }, [messages.length, autoScroll]);

  const sendMessage = useCallback((data: unknown) => {
    if (wsRef.current?.readyState === WebSocket.OPEN) {
      const message = typeof data === 'string' ? data : JSON.stringify(data);
      wsRef.current.send(message);
      
      // Track outbound message
      const size = new Blob([message]).size;
      const entry: WebSocketMessageEntry = {
        id: `msg_${Date.now()}_${messageIdCounter.current++}`,
        timestamp: Date.now(),
        direction: 'outbound',
        eventType: 'sent',
        data,
        size,
      };

      setMessages(prev => [entry, ...prev].slice(0, 1000));
      setStats(prev => ({
        ...prev,
        totalMessages: prev.totalMessages + 1,
        outbound: prev.outbound + 1,
        totalBytes: prev.totalBytes + size,
      }));
    }
  }, []);

  const subscribeToStream = useCallback((streamName: string, groupId: string = 'all') => {
    if (wsRef.current?.readyState === WebSocket.OPEN) {
      const subscriptionId = `console-${Date.now()}-${Math.random().toString(36).slice(2)}`;
      const message = {
        type: 'join',
        data: { streamName, groupId, subscriptionId }
      };
      wsRef.current.send(JSON.stringify(message));
      subscriptionsRef.current.add(streamName);
      setSubscribedStreams(prev => [...new Set([...prev, streamName])]);
      
      // Track as outbound message
      const size = new Blob([JSON.stringify(message)]).size;
      const entry: WebSocketMessageEntry = {
        id: `msg_${Date.now()}_${messageIdCounter.current++}`,
        timestamp: Date.now(),
        direction: 'outbound',
        streamName,
        groupId,
        eventType: 'subscribe',
        data: message,
        size,
      };
      setMessages(prev => [entry, ...prev].slice(0, 1000));
      setStats(prev => ({
        ...prev,
        totalMessages: prev.totalMessages + 1,
        outbound: prev.outbound + 1,
        totalBytes: prev.totalBytes + size,
      }));
      
      return subscriptionId;
    }
    return null;
  }, []);
  
  const unsubscribeFromStream = useCallback((streamName: string, groupId: string = 'all') => {
    if (wsRef.current?.readyState === WebSocket.OPEN) {
      const message = {
        type: 'leave',
        data: { streamName, groupId }
      };
      wsRef.current.send(JSON.stringify(message));
      subscriptionsRef.current.delete(streamName);
      setSubscribedStreams(prev => prev.filter(s => s !== streamName));
    }
  }, []);

  const filteredMessages = useMemo(() => {
    return messages.filter(msg => {
      // Direction filter
      if (directionFilter !== 'all' && msg.direction !== directionFilter) return false;
      
      // Stream filter
      if (streamFilter && msg.streamName !== streamFilter) return false;
      
      // System messages filter
      if (!showSystem && msg.direction === 'system') return false;
      
      // Search filter
      if (searchQuery) {
        const dataStr = JSON.stringify(msg.data).toLowerCase();
        const searchLower = searchQuery.toLowerCase();
        return dataStr.includes(searchLower) || 
               msg.streamName?.toLowerCase().includes(searchLower) ||
               msg.eventType.toLowerCase().includes(searchLower);
      }
      
      return true;
    });
  }, [messages, directionFilter, streamFilter, showSystem, searchQuery]);

  // Pagination
  const totalPages = Math.max(1, Math.ceil(filteredMessages.length / pageSize));
  const paginatedMessages = useMemo(() => {
    const start = (currentPage - 1) * pageSize;
    return filteredMessages.slice(start, start + pageSize);
  }, [filteredMessages, currentPage, pageSize]);

  // Reset to page 1 when filters change
  useEffect(() => {
    setCurrentPage(1);
  }, [directionFilter, streamFilter, showSystem, searchQuery, pageSize]);

  const selectedMessage = messages.find(m => m.id === selectedMessageId);

  const uniqueStreams = useMemo(() => {
    const streamSet = new Set<string>();
    messages.forEach(m => {
      if (m.streamName) streamSet.add(m.streamName);
    });
    return Array.from(streamSet);
  }, [messages]);

  const clearMessages = () => {
    setMessages([]);
    setStats({
      totalMessages: 0,
      inbound: 0,
      outbound: 0,
      totalBytes: 0,
      latency: stats.latency,
      lastPingTime: stats.lastPingTime
    });
    setSelectedMessageId(null);
  };

  const copyToClipboard = (text: string, id: string) => {
    navigator.clipboard.writeText(text);
    setCopiedId(id);
    setTimeout(() => setCopiedId(null), 2000);
  };

  const exportMessages = () => {
    const data = filteredMessages.map(m => ({
      timestamp: new Date(m.timestamp).toISOString(),
      direction: m.direction,
      streamName: m.streamName,
      groupId: m.groupId,
      eventType: m.eventType,
      size: m.size,
      data: m.data
    }));
    const blob = new Blob([JSON.stringify(data, null, 2)], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `ws-messages-${new Date().toISOString().slice(0, 19).replace(/:/g, '-')}.json`;
    a.click();
    URL.revokeObjectURL(url);
  };

  const userStreams = streams.filter(s => !s.internal);
  const systemStreams = streams.filter(s => s.internal);

  return (
    <div className="flex flex-col h-full bg-background text-foreground">
      {/* Header - Responsive */}
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-2 px-3 md:px-5 py-2 md:py-3 bg-dark-gray/30 border-b border-border">
        <div className="flex items-center gap-2 md:gap-4 flex-wrap">
          <h1 className="text-sm md:text-base font-semibold flex items-center gap-2">
            <Layers className="w-4 h-4 text-green-400" />
            Streams
          </h1>
          <div className="text-[10px] md:text-xs text-muted bg-dark-gray/50 px-1.5 md:px-2 py-0.5 md:py-1 rounded flex items-center gap-1 hidden sm:flex">
            <ArrowLeftRight className="w-3 h-3" />
            <span className="hidden md:inline">WebSocket Monitor</span>
            <span className="md:hidden">WS</span>
          </div>
          <Badge
            variant={wsConnected ? 'success' : 'error'}
            className="gap-1 text-[10px] md:text-xs"
          >
            {wsConnected ? <Wifi className="w-2.5 h-2.5 md:w-3 md:h-3" /> : <WifiOff className="w-2.5 h-2.5 md:w-3 md:h-3" />}
            <span className="hidden sm:inline">{wsConnected ? 'Connected' : 'Disconnected'}</span>
          </Badge>
          {stats.latency !== null && wsConnected && (
            <div className="flex items-center gap-1 text-[10px] md:text-xs text-muted">
              <Activity className={`w-2.5 h-2.5 md:w-3 md:h-3 ${stats.latency < 100 ? 'text-green-400' : stats.latency < 300 ? 'text-yellow' : 'text-error'}`} />
              <span className="font-mono tabular-nums">{stats.latency}ms</span>
            </div>
          )}
        </div>

        <div className="flex items-center gap-1.5 md:gap-2">
          <Button 
            variant={isPaused ? 'accent' : 'ghost'} 
            size="sm" 
            onClick={() => setIsPaused(!isPaused)}
            className="h-7 text-xs"
          >
            {isPaused ? <Play className="w-3 h-3 mr-1.5" /> : <Pause className="w-3 h-3 mr-1.5" />}
            {isPaused ? 'Resume' : 'Pause'}
          </Button>
          <Button 
            variant="ghost" 
            size="sm" 
            onClick={exportMessages}
            disabled={messages.length === 0}
            className="h-7 text-xs text-muted hover:text-foreground"
          >
            <Download className="w-3 h-3 mr-1.5" />
            Export
          </Button>
          <Button 
            variant="ghost" 
            size="sm" 
            onClick={clearMessages}
            disabled={messages.length === 0}
            className="h-7 text-xs text-muted hover:text-foreground"
          >
            <Trash2 className="w-3 h-3 mr-1.5" />
            Clear
          </Button>
          <Button 
            variant={showSystem ? "accent" : "ghost"} 
            size="sm" 
            onClick={() => setShowSystem(!showSystem)}
            className="h-7 text-xs"
          >
            {showSystem ? <Eye className="w-3 h-3 mr-1.5" /> : <EyeOff className="w-3 h-3 mr-1.5" />}
            <span className={showSystem ? '' : 'line-through opacity-60'}>System</span>
          </Button>
        </div>
      </div>

      {/* Stats Bar */}
      <div className="flex items-center gap-6 px-5 py-2 bg-dark-gray/20 border-b border-border text-xs">
        <div className="flex items-center gap-2">
          <span className="text-muted">Messages:</span>
          <span className="font-bold tabular-nums">{stats.totalMessages}</span>
        </div>
        <div className="flex items-center gap-2">
          <ArrowDownLeft className="w-3 h-3 text-cyan-400" />
          <span className="text-cyan-400 font-medium tabular-nums">{stats.inbound}</span>
          <span className="text-muted">inbound</span>
        </div>
        <div className="flex items-center gap-2">
          <ArrowUpRight className="w-3 h-3 text-orange-400" />
          <span className="text-orange-400 font-medium tabular-nums">{stats.outbound}</span>
          <span className="text-muted">outbound</span>
        </div>
        <div className="flex items-center gap-2">
          <span className="text-muted">Size:</span>
          <span className="font-mono tabular-nums">{formatBytes(stats.totalBytes)}</span>
        </div>
        <div className="flex-1" />
        <div className="text-muted font-mono">
          ws://localhost:{websocketPort}
        </div>
      </div>

      {/* Subscriptions Bar */}
      <div className="flex items-center gap-3 px-5 py-2 bg-dark-gray/20 border-b border-border">
        <span className="text-[11px] font-medium text-muted uppercase tracking-wide">Subscriptions</span>
        
        <div className="flex items-center gap-2">
          {subscribedStreams.map(stream => (
            <div
              key={stream}
              className="flex items-center gap-1.5 px-2 py-1 rounded-md bg-green-500/10 border border-green-500/30 text-xs"
            >
              <span className="w-1.5 h-1.5 rounded-full bg-green-400 animate-pulse" />
              <span className="text-green-400 font-medium">{stream.replace('iii:', '').replace('iii.', '')}</span>
              <button
                onClick={() => unsubscribeFromStream(stream)}
                className="p-0.5 rounded hover:bg-green-500/20 text-green-400/60 hover:text-green-400"
              >
                <X className="w-3 h-3" />
              </button>
            </div>
          ))}
        </div>

        <Button
          variant="outline"
          size="sm"
          onClick={() => setShowSubscribeModal(true)}
          disabled={!wsConnected}
          className="h-6 text-[10px] px-2 border-dashed gap-1"
        >
          <Plus className="w-3 h-3" />
          Subscribe
        </Button>

        {/* Quick subscribe to known streams */}
        {streams.filter(s => !subscribedStreams.includes(s.id)).length > 0 && (
          <div className="flex items-center gap-1 border-l border-border pl-3">
            <span className="text-[10px] text-muted">Quick:</span>
            {streams.filter(s => !subscribedStreams.includes(s.id) && !s.internal).slice(0, 3).map(stream => (
              <Button
                key={stream.id}
                variant="ghost"
                size="sm"
                onClick={() => subscribeToStream(stream.id)}
                disabled={!wsConnected}
                className="h-5 px-2 text-[10px]"
              >
                {stream.id}
              </Button>
            ))}
          </div>
        )}
      </div>

      {/* Filter Bar */}
      <div className="flex items-center gap-3 px-5 py-2 bg-dark-gray/10 border-b border-border">
        <div className="relative flex-1 max-w-sm">
          <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-muted" />
          <Input
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            placeholder="Search..."
            className="pl-9 pr-9 h-9"
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

        <div className="flex items-center gap-1 border-l border-border pl-3">
          <Button
            variant={directionFilter === 'all' ? 'accent' : 'ghost'}
            size="sm"
            onClick={() => setDirectionFilter('all')}
            className="h-7 text-xs px-2"
          >
            All
          </Button>
          <Button
            variant={directionFilter === 'inbound' ? 'accent' : 'ghost'}
            size="sm"
            onClick={() => setDirectionFilter('inbound')}
            className="h-7 text-xs px-2 gap-1"
          >
            <ArrowDownLeft className="w-3 h-3" />
            Inbound
          </Button>
          <Button
            variant={directionFilter === 'outbound' ? 'accent' : 'ghost'}
            size="sm"
            onClick={() => setDirectionFilter('outbound')}
            className="h-7 text-xs px-2 gap-1"
          >
            <ArrowUpRight className="w-3 h-3" />
            Outbound
          </Button>
        </div>

        {uniqueStreams.length > 0 && (
          <div className="flex items-center gap-1 border-l border-border pl-3">
            <span className="text-xs text-muted mr-1">Stream:</span>
            <Button
              variant={streamFilter === null ? 'accent' : 'ghost'}
              size="sm"
              onClick={() => setStreamFilter(null)}
              className="h-6 text-[10px] px-2"
            >
              All
            </Button>
            {uniqueStreams.slice(0, 4).map(stream => (
              <Button
                key={stream}
                variant={streamFilter === stream ? 'accent' : 'ghost'}
                size="sm"
                onClick={() => setStreamFilter(stream)}
                className="h-6 text-[10px] px-2 max-w-[100px] truncate"
              >
                {stream.replace('iii:', '').replace('iii.', '')}
              </Button>
            ))}
            {uniqueStreams.length > 4 && (
              <span className="text-[10px] text-muted">+{uniqueStreams.length - 4} more</span>
            )}
          </div>
        )}

        {/* Help Button */}
        <div className="relative ml-auto">
          <Button
            variant="ghost"
            size="sm"
            onClick={() => setShowHelp(!showHelp)}
            className="h-7 w-7 p-0"
            title="Event Types Legend"
          >
            <HelpCircle className="w-4 h-4" />
          </Button>
          
          {showHelp && (
            <div className="absolute right-0 top-full mt-1 z-50 w-72 bg-card border border-border rounded-lg shadow-xl p-3">
              <div className="flex items-center justify-between mb-2">
                <h4 className="text-xs font-semibold">Event Types</h4>
                <button onClick={() => setShowHelp(false)} className="text-muted hover:text-foreground">
                  <X className="w-3 h-3" />
                </button>
              </div>
              <div className="space-y-1.5 text-[11px]">
                <div className="flex items-center gap-2">
                  <span className="w-16 font-mono text-success">create</span>
                  <span className="text-muted">New item added to stream</span>
                </div>
                <div className="flex items-center gap-2">
                  <span className="w-16 font-mono text-yellow">update</span>
                  <span className="text-muted">Existing item modified</span>
                </div>
                <div className="flex items-center gap-2">
                  <span className="w-16 font-mono text-error">delete</span>
                  <span className="text-muted">Item removed from stream</span>
                </div>
                <div className="flex items-center gap-2">
                  <span className="w-16 font-mono text-purple-400">sync</span>
                  <span className="text-muted">Initial data on subscription</span>
                </div>
                <div className="flex items-center gap-2">
                  <span className="w-16 font-mono text-cyan-400">subscribe</span>
                  <span className="text-muted">Subscribed to stream</span>
                </div>
              </div>
              <div className="mt-2 pt-2 border-t border-border text-[10px] text-muted">
                Hover over event types in the list for descriptions
              </div>
            </div>
          )}
        </div>
      </div>

      {error && (
        <div className="mx-4 mt-4 bg-error/10 border border-error/30 rounded px-4 py-3 text-sm text-error">
          {error}
        </div>
      )}

      {/* Main Content - Message List + Detail Panel */}
      <div className={`flex-1 flex overflow-hidden ${selectedMessage ? '' : ''}`}>
        {/* Message List */}
        <div 
          className={`flex flex-col flex-1 overflow-hidden ${selectedMessage ? 'border-r border-border' : ''}`}
        >
          <div 
            ref={messagesContainerRef}
            className="flex-1 overflow-auto"
          >
            {filteredMessages.length === 0 ? (
              <div className="flex flex-col items-center justify-center h-64 text-muted">
                <Layers className="w-8 h-8 mb-3 opacity-30" />
                <span className="text-sm">
                  {messages.length === 0 ? 'Waiting for WebSocket messages...' : 'No messages match your filters'}
                </span>
                {wsConnected && messages.length === 0 && (
                  <span className="text-xs mt-1 text-green-400">Connected to ws://localhost:{websocketPort}</span>
                )}
              </div>
            ) : (
              <div className="divide-y divide-border/30">
                {paginatedMessages.map((msg) => {
                  const config = DIRECTION_CONFIG[msg.direction];
                  const Icon = config.icon;
                  const isSelected = selectedMessageId === msg.id;
                  
                  return (
                    <div
                      key={msg.id}
                      role="row"
                      className={`flex items-center font-mono cursor-pointer text-[12px] h-8 px-3 border-l-2 transition-colors
                        ${isSelected ? 'bg-primary/10 border-l-primary' : `hover:bg-dark-gray/30 ${config.border}`}
                      `}
                      onClick={() => setSelectedMessageId(isSelected ? null : msg.id)}
                    >
                      <div className="flex items-center gap-2 w-[80px] shrink-0">
                        <Icon className={`w-3 h-3 ${config.color}`} />
                        <span className="text-muted text-[11px]">{formatTime(msg.timestamp)}</span>
                      </div>
                      
                      {msg.streamName && (
                        <div className="shrink-0 ml-2 px-1.5 py-0.5 rounded bg-dark-gray/50 text-[10px] text-muted max-w-[120px] truncate">
                          {msg.streamName.replace('iii:', '').replace('__motia.', '')}
                        </div>
                      )}
                      
                      <div 
                        className={`shrink-0 ml-2 px-1.5 py-0.5 rounded text-[10px] ${config.bg} ${EVENT_TYPE_INFO[msg.eventType]?.color || config.color}`}
                        title={EVENT_TYPE_INFO[msg.eventType]?.description || msg.eventType}
                      >
                        {msg.eventType}
                      </div>
                      
                      <div className="flex-1 ml-3 truncate text-foreground/80">
                        {typeof msg.data === 'string' 
                          ? msg.data.slice(0, 100) 
                          : JSON.stringify(msg.data).slice(0, 100)}
                      </div>
                      
                      <div className="shrink-0 ml-2 text-[10px] text-muted tabular-nums">
                        {formatBytes(msg.size)}
                      </div>
                    </div>
                  );
                })}
              </div>
            )}
          </div>
          
          {/* Pagination - Fixed at bottom */}
          {filteredMessages.length > 0 && (
            <div className="flex-shrink-0 bg-background/95 backdrop-blur border-t border-border px-3 py-2">
              <Pagination
                currentPage={currentPage}
                totalPages={totalPages}
                totalItems={filteredMessages.length}
                pageSize={pageSize}
                onPageChange={setCurrentPage}
                onPageSizeChange={setPageSize}
                pageSizeOptions={[25, 50, 100, 250]}
              />
            </div>
          )}
        </div>

        {/* Detail Panel */}
        {selectedMessage && (
          <div className="w-[400px] flex flex-col overflow-hidden bg-dark-gray/10">
            <div className="p-4 border-b border-border sticky top-0 bg-dark-gray/80 backdrop-blur z-10">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2">
                  {(() => {
                    const config = DIRECTION_CONFIG[selectedMessage.direction];
                    const Icon = config.icon;
                    return (
                      <>
                        <Icon className={`w-4 h-4 ${config.color}`} />
                        <span className={`px-2 py-0.5 rounded text-[10px] font-semibold uppercase ${config.bg} ${config.color}`}>
                          {config.label}
                        </span>
                      </>
                    );
                  })()}
                </div>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => setSelectedMessageId(null)}
                  className="h-6 w-6 p-0"
                >
                  <X className="w-3.5 h-3.5" />
                </Button>
              </div>
              <p className="text-xs text-muted mt-2 font-mono">
                {new Date(selectedMessage.timestamp).toISOString()}
              </p>
            </div>

            <div className="flex-1 overflow-auto p-4 space-y-4">
              <div className="grid grid-cols-2 gap-3">
                <div className="bg-dark-gray/30 rounded p-2">
                  <div className="text-[9px] text-muted uppercase mb-1">Event Type</div>
                  <div className={`text-xs font-medium ${EVENT_TYPE_INFO[selectedMessage.eventType]?.color || ''}`}>
                    {selectedMessage.eventType}
                  </div>
                  {EVENT_TYPE_INFO[selectedMessage.eventType]?.description && (
                    <div className="text-[9px] text-muted mt-0.5">
                      {EVENT_TYPE_INFO[selectedMessage.eventType].description}
                    </div>
                  )}
                </div>
                <div className="bg-dark-gray/30 rounded p-2">
                  <div className="text-[9px] text-muted uppercase mb-1">Size</div>
                  <div className="text-xs font-mono">{formatBytes(selectedMessage.size)}</div>
                </div>
              </div>

              {selectedMessage.streamName && (
                <div>
                  <div className="text-[10px] text-muted uppercase tracking-wider mb-2">Stream</div>
                  <code className="text-xs font-mono bg-dark-gray px-2 py-1.5 rounded block">
                    {selectedMessage.streamName}
                  </code>
                </div>
              )}

              {selectedMessage.groupId && (
                <div>
                  <div className="text-[10px] text-muted uppercase tracking-wider mb-2">Group ID</div>
                  <code className="text-xs font-mono bg-dark-gray px-2 py-1.5 rounded block">
                    {selectedMessage.groupId}
                  </code>
                </div>
              )}

              <div>
                <div className="text-[10px] text-muted uppercase tracking-wider mb-2 flex items-center gap-2">
                  Data
                  <button
                    onClick={() => copyToClipboard(JSON.stringify(selectedMessage.data, null, 2), 'data')}
                    className="p-0.5 rounded hover:bg-dark-gray/50 transition-colors"
                  >
                    {copiedId === 'data' ? (
                      <Check className="w-3 h-3 text-success" />
                    ) : (
                      <Copy className="w-3 h-3" />
                    )}
                  </button>
                </div>
                <div className="bg-dark-gray px-3 py-2 rounded overflow-x-auto max-h-[400px] overflow-y-auto">
                  <JsonViewer data={selectedMessage.data} collapsed={false} maxDepth={6} />
                </div>
              </div>
            </div>
          </div>
        )}
      </div>

      {/* Subscribe Modal */}
      {showSubscribeModal && (
        <div className="fixed inset-0 bg-background/80 backdrop-blur-sm flex items-center justify-center z-50">
          <div className="bg-background border border-border rounded-lg shadow-xl w-full max-w-md">
            <div className="flex items-center justify-between px-4 py-3 border-b border-border">
              <h3 className="font-semibold flex items-center gap-2">
                <Layers className="w-4 h-4 text-green-400" />
                Subscribe to Stream
              </h3>
              <button
                onClick={() => {
                  setShowSubscribeModal(false);
                  setNewStreamName('');
                }}
                className="p-1 rounded hover:bg-dark-gray"
              >
                <X className="w-4 h-4" />
              </button>
            </div>

            <div className="p-4 space-y-4">
              <div>
                <label className="text-xs text-muted block mb-1.5">Stream Name</label>
                <Input
                  value={newStreamName}
                  onChange={(e) => setNewStreamName(e.target.value)}
                  placeholder="e.g., todo, iii.logs, my-stream"
                  className="font-mono"
                  onKeyDown={(e) => {
                    if (e.key === 'Enter' && newStreamName) {
                      subscribeToStream(newStreamName);
                      setShowSubscribeModal(false);
                      setNewStreamName('');
                    }
                  }}
                />
              </div>

              {/* Available streams from API */}
              {streams.length > 0 && (
                <div>
                  <label className="text-xs text-muted block mb-2">Available Streams</label>
                  <div className="flex flex-wrap gap-2 max-h-32 overflow-auto">
                    {streams.filter(s => !subscribedStreams.includes(s.id)).map(stream => (
                      <button
                        key={stream.id}
                        onClick={() => {
                          subscribeToStream(stream.id);
                          setShowSubscribeModal(false);
                          setNewStreamName('');
                        }}
                        className={`px-2 py-1 rounded text-xs font-mono transition-colors ${
                          stream.internal 
                            ? 'bg-dark-gray/50 text-muted hover:bg-dark-gray hover:text-foreground' 
                            : 'bg-green-500/10 text-green-400 hover:bg-green-500/20'
                        }`}
                      >
                        {stream.id}
                      </button>
                    ))}
                  </div>
                </div>
              )}
            </div>

            <div className="flex items-center justify-end gap-2 px-4 py-3 border-t border-border">
              <Button
                variant="ghost"
                size="sm"
                onClick={() => {
                  setShowSubscribeModal(false);
                  setNewStreamName('');
                }}
              >
                Cancel
              </Button>
              <Button
                variant="accent"
                size="sm"
                onClick={() => {
                  if (newStreamName) {
                    subscribeToStream(newStreamName);
                    setShowSubscribeModal(false);
                    setNewStreamName('');
                  }
                }}
                disabled={!newStreamName}
                className="gap-1.5"
              >
                <Zap className="w-3 h-3" />
                Subscribe
              </Button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

'use client';

import { useState, useEffect } from 'react';
import { Card, CardContent, CardHeader, CardTitle, Badge, Table, TableHeader, TableBody, TableRow, TableHead, TableCell, Button, Input } from "@/components/ui/card";
import { Layers, Search, RefreshCw, Radio, Database, Wifi, Activity, Eye, EyeOff } from "lucide-react";
import { fetchStreams, StreamInfo, connectToStreams } from "@/lib/api";

export default function StreamsPage() {
  const [streams, setStreams] = useState<StreamInfo[]>([]);
  const [websocketPort, setWebsocketPort] = useState<number>(31112);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState('');
  const [wsConnected, setWsConnected] = useState(false);
  const [lastMessage, setLastMessage] = useState<string | null>(null);
  const [showSystem, setShowSystem] = useState(false);

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
        onMessage: (message) => {
          if (mounted) {
            setLastMessage(JSON.stringify(message).slice(0, 100));
          }
        }
      });
    }, 100);

    return () => {
      mounted = false;
      clearTimeout(timer);
      disconnect?.();
    };
  }, []);

  const userStreams = streams.filter(s => !s.internal);
  const systemStreams = streams.filter(s => s.internal);

  const filteredStreams = streams.filter(s => {
    if (!showSystem && s.internal) return false;
    if (searchQuery && !s.id.toLowerCase().includes(searchQuery.toLowerCase())) return false;
    return true;
  });

  const getTypeIcon = (type: string) => {
    switch (type) {
      case 'state': return <Database className="w-4 h-4" />;
      case 'events': return <Radio className="w-4 h-4" />;
      default: return <Layers className="w-4 h-4" />;
    }
  };

  return (
    <div className="p-6 space-y-6">
      {}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-xl font-semibold tracking-tight">Streams & Events</h1>
          <p className="text-xs text-muted mt-1 tracking-wide">
            Your real-time message streams and event topics
            {systemStreams.length > 0 && !showSystem && (
              <span className="text-muted/60 ml-2">({systemStreams.length} system hidden)</span>
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
      <div className="grid gap-4 grid-cols-2 lg:grid-cols-4">
        <Card>
          <CardContent className="p-4">
            <div className="flex items-center justify-between">
              <div>
                <div className="text-[10px] text-muted uppercase tracking-wider">Your Streams</div>
                <div className="text-2xl font-semibold mt-1">{userStreams.length}</div>
                {showSystem && systemStreams.length > 0 && (
                  <div className="text-[9px] text-muted mt-0.5">+{systemStreams.length} system</div>
                )}
              </div>
              <Layers className="w-5 h-5 text-muted" />
            </div>
          </CardContent>
        </Card>
        <Card>
          <CardContent className="p-4">
            <div className="flex items-center justify-between">
              <div>
                <div className="text-[10px] text-muted uppercase tracking-wider">WebSocket</div>
                <div className="text-lg font-mono mt-1">:{websocketPort}</div>
              </div>
              <Wifi className="w-5 h-5 text-muted" />
            </div>
          </CardContent>
        </Card>
        <Card>
          <CardContent className="p-4">
            <div className="flex items-center justify-between">
              <div>
                <div className="text-[10px] text-muted uppercase tracking-wider">Connection</div>
                <div className={`text-lg font-semibold mt-1 flex items-center gap-2 ${wsConnected ? 'text-success' : 'text-muted'}`}>
                  <div className={`w-2 h-2 rounded-full ${wsConnected ? 'bg-success animate-pulse' : 'bg-muted'}`} />
                  {wsConnected ? 'Connected' : 'Disconnected'}
                </div>
              </div>
            </div>
          </CardContent>
        </Card>
        <Card>
          <CardContent className="p-4">
            <div className="flex items-center justify-between">
              <div>
                <div className="text-[10px] text-muted uppercase tracking-wider">Status</div>
                <div className="text-lg font-semibold mt-1 text-success flex items-center gap-2">
                  <Activity className="w-4 h-4" />
                  Active
                </div>
              </div>
            </div>
          </CardContent>
        </Card>
      </div>

      {}
      <Card>
        <CardHeader className="flex flex-row items-center justify-between">
          <CardTitle>Registered Streams</CardTitle>
          <div className="flex items-center gap-2">
            <div className="relative">
              <Search className="w-3 h-3 absolute left-3 top-1/2 -translate-y-1/2 text-muted" />
              <Input 
                placeholder="Search streams..."
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                className="pl-8 w-48"
              />
            </div>
          </div>
        </CardHeader>
        <CardContent>
          {loading ? (
            <div className="flex items-center justify-center py-8 text-muted">
              <RefreshCw className="w-4 h-4 animate-spin mr-2" />
              Loading streams...
            </div>
          ) : filteredStreams.length === 0 ? (
            <div className="text-xs text-muted py-8 text-center border border-dashed border-border rounded">
              No streams found
            </div>
          ) : (
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Stream ID</TableHead>
                  <TableHead>Type</TableHead>
                  <TableHead>Groups</TableHead>
                  <TableHead>Description</TableHead>
                  <TableHead>Status</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {filteredStreams.map((stream) => (
                  <TableRow key={stream.id}>
                    <TableCell>
                      <div className="flex items-center gap-2">
                        {getTypeIcon(stream.type)}
                        <code className="font-mono text-yellow">{stream.id}</code>
                      </div>
                    </TableCell>
                    <TableCell>
                      <Badge variant="outline">{stream.type.toUpperCase()}</Badge>
                    </TableCell>
                    <TableCell>
                      <div className="flex flex-wrap gap-1">
                        {stream.groups.map(group => (
                          <Badge key={group} variant="outline" className="text-[10px]">{group}</Badge>
                        ))}
                      </div>
                    </TableCell>
                    <TableCell className="text-muted">{stream.description}</TableCell>
                    <TableCell>
                      <Badge variant={stream.status === 'active' ? 'success' : 'default'}>
                        {stream.status.toUpperCase()}
                      </Badge>
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          )}
        </CardContent>
      </Card>

      {}
      <Card>
        <CardHeader>
          <CardTitle>WebSocket Connection</CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="grid gap-4 md:grid-cols-2">
            <div className="p-3 bg-dark-gray/30 rounded border border-border">
              <div className="text-[10px] text-muted uppercase tracking-wider mb-1">Endpoint</div>
              <code className="text-sm font-mono text-foreground">ws://localhost:{websocketPort}</code>
            </div>
            <div className="p-3 bg-dark-gray/30 rounded border border-border">
              <div className="text-[10px] text-muted uppercase tracking-wider mb-1">Protocol</div>
              <code className="text-sm font-mono text-foreground">iii-streams-v1</code>
            </div>
          </div>
          
          {lastMessage && (
            <div className="p-3 bg-black rounded border border-border">
              <div className="text-[10px] text-muted uppercase tracking-wider mb-2">Last Message</div>
              <code className="text-xs font-mono text-muted">{lastMessage}...</code>
            </div>
          )}

          <div className="text-xs text-muted">
            Connect to the WebSocket to receive real-time updates from iii streams. 
            Use <code className="text-yellow">join</code> messages to subscribe to specific streams.
          </div>
        </CardContent>
      </Card>
    </div>
  );
}

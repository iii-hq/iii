

const DEVTOOLS_API = 'http://localhost:3111/_console';

const MANAGEMENT_API = 'http://localhost:9001/_/api';


export interface SystemStatus {
  status: string;
  uptime_seconds: number;
  uptime_formatted: string;
  workers: number;
  functions: number;
  triggers: number;
  version: string;
  timestamp: number;
}

export interface FunctionInfo {
  path: string;
  description: string | null;
  metadata: Record<string, unknown> | null;
  internal?: boolean;  // True for system/devtools functions
}

export interface TriggerInfo {
  id: string;
  trigger_type: string;
  function_path: string;
  config: Record<string, unknown>;
  worker_id: string | null;
  internal?: boolean;  // True for system/devtools triggers
}

export interface TriggerTypeInfo {
  id: string;
  description: string;
}

export interface WorkerInfo {
  id: string;
  functions: string[];
}

export interface MetricsSnapshot {
  id?: string;
  timestamp: number;
  functions_count: number;
  triggers_count: number;
  workers_count: number;
  uptime_seconds: number;
}

export interface DevToolsConfig {
  devtools: {
    enabled: boolean;
    api_prefix: string;
    metrics_enabled: boolean;
    metrics_interval: number;
    state_stream: string;
    event_topic: string;
  };
  engine: {
    version: string;
  };
}

export interface EventsInfo {
  topic: string;
  stream: string;
  description: string;
}

export interface StreamInfo {
  id: string;
  type: string;
  description: string;
  groups: string[];
  status: string;
  internal?: boolean;  // True for system/devtools streams
}

export interface LogEntry {
  id: string;
  timestamp: number;
  level: string;
  message: string;
  source: string;
  metadata?: Record<string, unknown>;
}

export interface AdapterInfo {
  id: string;
  type: string;
  status: string;
  health: string;
  description?: string;
  count?: number;
  port?: number;
  internal?: boolean;  // True for system/devtools adapters
}


interface WrappedResponse<T> {
  status_code: number;
  headers: [string, string][];
  body: T;
}


async function unwrapResponse<T>(res: Response): Promise<T> {
  const data = await res.json();
  
  if (data && typeof data === 'object' && 'status_code' in data && 'body' in data) {
    const wrapped = data as WrappedResponse<T>;
    if (wrapped.status_code !== 200) {
      throw new Error(`API Error: ${JSON.stringify(wrapped.body)}`);
    }
    return wrapped.body;
  }
  
  return data as T;
}



export async function fetchStatus(): Promise<SystemStatus> {
  try {
    const res = await fetch(`${DEVTOOLS_API}/status`);
    if (res.ok) {
      const data = await unwrapResponse<{ status: SystemStatus }>(res);
      return data.status;
    }
  } catch {
  }
  
  const res = await fetch(`${MANAGEMENT_API}/status`);
  if (!res.ok) throw new Error('Failed to fetch status');
  const data = await res.json();
  
  return {
    status: data.status,
    uptime_seconds: 0,
    uptime_formatted: data.uptime || '—',
    workers: data.workers,
    functions: data.functions,
    triggers: 0,
    version: data.version,
    timestamp: Date.now() / 1000,
  };
}


export async function fetchFunctions(): Promise<{ functions: FunctionInfo[]; count: number }> {
  const res = await fetch(`${DEVTOOLS_API}/functions`);
  if (!res.ok) throw new Error('Failed to fetch functions');
  return unwrapResponse(res);
}


export async function fetchTriggers(): Promise<{ triggers: TriggerInfo[]; count: number }> {
  try {
    const res = await fetch(`${DEVTOOLS_API}/triggers`);
    if (res.ok) {
      return unwrapResponse(res);
    }
  } catch {
  }
  
  const res = await fetch(`${MANAGEMENT_API}/triggers`);
  if (!res.ok) throw new Error('Failed to fetch triggers');
  return res.json();
}


export async function fetchTriggerTypes(): Promise<{ trigger_types: TriggerTypeInfo[]; count: number }> {
  const res = await fetch(`${DEVTOOLS_API}/trigger-types`);
  if (!res.ok) throw new Error('Failed to fetch trigger types');
  return unwrapResponse(res);
}


export async function fetchWorkers(): Promise<{ workers: WorkerInfo[]; count: number }> {
  const res = await fetch(`${DEVTOOLS_API}/workers`);
  if (!res.ok) throw new Error('Failed to fetch workers');
  return unwrapResponse(res);
}


export async function fetchConfig(): Promise<DevToolsConfig> {
  try {
    const res = await fetch(`${DEVTOOLS_API}/config`);
    if (res.ok) {
      return unwrapResponse(res);
    }
  } catch {
  }
  
  const res = await fetch(`${MANAGEMENT_API}/config`);
  if (!res.ok) throw new Error('Failed to fetch config');
  return res.json();
}


export async function fetchMetrics(): Promise<MetricsSnapshot> {
  const res = await fetch(`${DEVTOOLS_API}/metrics`);
  if (!res.ok) throw new Error('Failed to fetch metrics');
  return unwrapResponse(res);
}


export async function fetchMetricsHistory(limit?: number): Promise<{ history: MetricsSnapshot[]; count: number }> {
  const url = limit 
    ? `${DEVTOOLS_API}/metrics/history?limit=${limit}`
    : `${DEVTOOLS_API}/metrics/history`;
  const res = await fetch(url);
  if (!res.ok) throw new Error('Failed to fetch metrics history');
  return unwrapResponse(res);
}


export async function fetchEventsInfo(): Promise<EventsInfo> {
  const res = await fetch(`${DEVTOOLS_API}/events`);
  if (!res.ok) throw new Error('Failed to fetch events info');
  return unwrapResponse(res);
}


export async function healthCheck(): Promise<{ status: string; timestamp: number }> {
  const res = await fetch(`${DEVTOOLS_API}/health`);
  if (!res.ok) throw new Error('Health check failed');
  return unwrapResponse(res);
}


export async function fetchStreams(): Promise<{ streams: StreamInfo[]; count: number; websocket_port: number }> {
  const res = await fetch(`${DEVTOOLS_API}/streams`);
  if (!res.ok) throw new Error('Failed to fetch streams');
  return unwrapResponse(res);
}


export async function fetchLogs(options?: { level?: string; limit?: number; since?: number }): Promise<{
  logs: LogEntry[];
  count: number;
  filter: { level: string; limit: number; since: number };
  info: { message: string; adapters: string[] };
}> {
  const params = new URLSearchParams();
  if (options?.level) params.set('level', options.level);
  if (options?.limit) params.set('limit', options.limit.toString());
  if (options?.since) params.set('since', options.since.toString());
  
  const url = params.toString() 
    ? `${DEVTOOLS_API}/logs?${params}`
    : `${DEVTOOLS_API}/logs`;
  
  const res = await fetch(url);
  if (!res.ok) throw new Error('Failed to fetch logs');
  return unwrapResponse(res);
}


export async function fetchAdapters(): Promise<{ adapters: AdapterInfo[]; count: number }> {
  const res = await fetch(`${DEVTOOLS_API}/adapters`);
  if (!res.ok) throw new Error('Failed to fetch adapters');
  return unwrapResponse(res);
}



export async function isDevToolsAvailable(): Promise<boolean> {
  try {
    const res = await fetch(`${DEVTOOLS_API}/health`);
    return res.ok;
  } catch {
    return false;
  }
}


export async function isManagementApiAvailable(): Promise<boolean> {
  try {
    const res = await fetch(`${MANAGEMENT_API}/status`);
    return res.ok;
  } catch {
    return false;
  }
}


export async function getConnectionStatus(): Promise<{
  devtools: boolean;
  management: boolean;
}> {
  const [devtools, management] = await Promise.all([
    isDevToolsAvailable(),
    isManagementApiAvailable(),
  ]);
  return { devtools, management };
}


const STREAMS_WS = 'ws://localhost:31112';

const DEVTOOLS_STREAM = 'iii:devtools:state';
const DEVTOOLS_METRICS_GROUP = 'metrics';

export interface StreamMessage {
  timestamp: number;
  streamName: string;
  groupId: string;
  id: string | null;
  event: {
    type: 'sync' | 'create' | 'update' | 'delete' | 'unauthorized';
    data?: unknown;
  };
}


export function subscribeToMetricsStream(
  onMetrics: (metrics: MetricsSnapshot) => void,
  onSync?: (allMetrics: MetricsSnapshot[]) => void,
  onError?: (error: Error) => void,
  onConnect?: () => void,
  onDisconnect?: () => void
): () => void {
  let ws: WebSocket | null = null;
  let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  let isActive = true;
  const subscriptionId = `console-${Date.now()}-${Math.random().toString(36).slice(2)}`;

  const connect = () => {
    if (!isActive) return;

    try {
      ws = new WebSocket(STREAMS_WS);

      ws.onopen = () => {
        onConnect?.();
        
        const joinMessage = {
          type: 'join',
          data: {
            subscriptionId,
            streamName: DEVTOOLS_STREAM,
            groupId: DEVTOOLS_METRICS_GROUP,
          }
        };
        ws?.send(JSON.stringify(joinMessage));
      };

      ws.onmessage = (event) => {
        try {
          const message: StreamMessage = JSON.parse(event.data);

          switch (message.event.type) {
            case 'sync':
              if (message.event.data && Array.isArray(message.event.data)) {
                const metrics = message.event.data as MetricsSnapshot[];
                onSync?.(metrics);
              }
              break;

            case 'create':
            case 'update':
              if (message.event.data) {
                const metrics = message.event.data as MetricsSnapshot;
                onMetrics(metrics);
              }
              break;

            case 'delete':
              break;

            case 'unauthorized':
              break;
          }
        } catch {
        }
      };

      ws.onerror = () => {
        onError?.(new Error('WebSocket connection error'));
      };

      ws.onclose = () => {
        onDisconnect?.();
        
        if (isActive) {
          reconnectTimer = setTimeout(connect, 3000);
        }
      };
    } catch (error) {
      onError?.(error instanceof Error ? error : new Error('Connection failed'));
      
      if (isActive) {
        reconnectTimer = setTimeout(connect, 3000);
      }
    }
  };

  connect();

  return () => {
    isActive = false;
    
    if (reconnectTimer) {
      clearTimeout(reconnectTimer);
      reconnectTimer = null;
    }
    
    if (ws && ws.readyState !== WebSocket.CLOSED) {
      const leaveMessage = {
        type: 'leave',
        data: {
          subscriptionId,
          streamName: DEVTOOLS_STREAM,
          groupId: DEVTOOLS_METRICS_GROUP,
        }
      };
      try {
        if (ws.readyState === WebSocket.OPEN) {
          ws.send(JSON.stringify(leaveMessage));
        }
        ws.close();
      } catch {
      }
      ws = null;
    }
  };
}


export function connectToStreams(options: {
  onConnect?: () => void;
  onDisconnect?: () => void;
  onMessage?: (message: StreamMessage) => void;
  onError?: (error: Error) => void;
}): () => void {
  let ws: WebSocket | null = null;
  let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  let isActive = true;

  const connect = () => {
    if (!isActive) return;

    try {
      ws = new WebSocket(STREAMS_WS);

      ws.onopen = () => {
        options.onConnect?.();
      };

      ws.onclose = () => {
        options.onDisconnect?.();
        
        if (isActive) {
          reconnectTimer = setTimeout(connect, 3000);
        }
      };

      ws.onerror = () => {
        options.onError?.(new Error('WebSocket connection error'));
      };

      ws.onmessage = (event) => {
        try {
          const message = JSON.parse(event.data) as StreamMessage;
          options.onMessage?.(message);
        } catch {
        }
      };
    } catch (err) {
      options.onError?.(err instanceof Error ? err : new Error('Failed to connect'));
      
      if (isActive) {
        reconnectTimer = setTimeout(connect, 3000);
      }
    }
  };

  connect();

  return () => {
    isActive = false;
    if (reconnectTimer) {
      clearTimeout(reconnectTimer);
      reconnectTimer = null;
    }
    if (ws && ws.readyState !== WebSocket.CLOSED) {
      try {
        ws.close();
      } catch {
      }
      ws = null;
    }
  };
}

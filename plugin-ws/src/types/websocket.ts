export interface WebSocketConnection {
  id: string
  url: string
  status: 'connecting' | 'connected' | 'disconnected' | 'error'
  createdAt: number
  lastActivity?: number
  messageCount: number
  errorMessage?: string
}

export interface WebSocketMessage {
  id: string
  connectionId: string
  type: 'sent' | 'received'
  data: string | object
  timestamp: number
}

export interface WebSocketStats {
  totalConnections: number
  activeConnections: number
  totalMessages: number
  messagesPerSecond: number
}

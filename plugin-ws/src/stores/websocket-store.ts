import { create } from 'zustand'
import type { WebSocketConnection, WebSocketMessage, WebSocketStats } from '../types/websocket'

const MAX_MESSAGES = 500

export interface ReconnectionState {
  isReconnecting: boolean
  retryCount: number
  nextRetryAt: number | null
  lastError: string | null
}

export interface ConnectionHealth {
  latency: number | null
  lastPingAt: number | null
  lastPongAt: number | null
}

interface WebSocketStore {
  connections: WebSocketConnection[]
  messages: WebSocketMessage[]
  selectedConnectionId: string | null
  stats: WebSocketStats
  searchQuery: string
  reconnectionState: ReconnectionState
  connectionHealth: ConnectionHealth

  setConnections: (connections: WebSocketConnection[]) => void
  addConnection: (connection: WebSocketConnection) => void
  updateConnection: (id: string, updates: Partial<WebSocketConnection>) => void
  removeConnection: (id: string) => void
  setMessages: (messages: WebSocketMessage[]) => void
  addMessage: (message: WebSocketMessage) => void
  clearMessages: (connectionId?: string) => void
  selectConnection: (id: string | null) => void
  updateStats: (stats: Partial<WebSocketStats>) => void
  setSearchQuery: (query: string) => void
  setReconnectionState: (state: Partial<ReconnectionState>) => void
  setConnectionHealth: (health: Partial<ConnectionHealth>) => void
  exportMessages: () => string
}

export const useWebSocketStore = create<WebSocketStore>((set, get) => ({
  connections: [],
  messages: [],
  selectedConnectionId: null,
  stats: {
    totalConnections: 0,
    activeConnections: 0,
    totalMessages: 0,
    messagesPerSecond: 0,
  },
  searchQuery: '',
  reconnectionState: {
    isReconnecting: false,
    retryCount: 0,
    nextRetryAt: null,
    lastError: null,
  },
  connectionHealth: {
    latency: null,
    lastPingAt: null,
    lastPongAt: null,
  },

  setConnections: (connections) =>
    set((state) => ({
      connections,
      stats: {
        ...state.stats,
        totalConnections: connections.length,
        activeConnections: connections.filter((c) => c.status === 'connected').length,
      },
    })),

  addConnection: (connection) =>
    set((state) => {
      const connections = [...state.connections, connection]
      return {
        connections,
        stats: {
          ...state.stats,
          totalConnections: connections.length,
          activeConnections: connections.filter((c) => c.status === 'connected').length,
        },
      }
    }),

  updateConnection: (id, updates) =>
    set((state) => {
      const connections = state.connections.map((c) => (c.id === id ? { ...c, ...updates } : c))
      return {
        connections,
        stats: {
          ...state.stats,
          totalConnections: connections.length,
          activeConnections: connections.filter((c) => c.status === 'connected').length,
        },
      }
    }),

  removeConnection: (id) =>
    set((state) => {
      const connections = state.connections.filter((c) => c.id !== id)
      return {
        connections,
        stats: {
          ...state.stats,
          totalConnections: connections.length,
          activeConnections: connections.filter((c) => c.status === 'connected').length,
        },
      }
    }),

  setMessages: (messages) =>
    set((state) => ({
      messages: messages.slice(-MAX_MESSAGES),
      stats: { ...state.stats, totalMessages: Math.min(messages.length, MAX_MESSAGES) },
    })),

  addMessage: (message) =>
    set((state) => {
      const messages = [...state.messages, message].slice(-MAX_MESSAGES)
      return { messages, stats: { ...state.stats, totalMessages: messages.length } }
    }),

  clearMessages: (connectionId) =>
    set((state) => {
      const messages = connectionId ? state.messages.filter((m) => m.connectionId !== connectionId) : []
      return { messages, stats: { ...state.stats, totalMessages: messages.length } }
    }),

  selectConnection: (id) => set({ selectedConnectionId: id }),
  updateStats: (stats) => set((state) => ({ stats: { ...state.stats, ...stats } })),
  setSearchQuery: (query) => set({ searchQuery: query }),

  setReconnectionState: (reconnectionState) =>
    set((state) => ({ reconnectionState: { ...state.reconnectionState, ...reconnectionState } })),

  setConnectionHealth: (health) => set((state) => ({ connectionHealth: { ...state.connectionHealth, ...health } })),

  exportMessages: () => {
    const state = get()
    const exportData = {
      exportedAt: new Date().toISOString(),
      totalMessages: state.messages.length,
      messages: state.messages.map((msg) => ({
        id: msg.id,
        type: msg.type,
        timestamp: new Date(msg.timestamp).toISOString(),
        data: msg.data,
      })),
    }
    return JSON.stringify(exportData, null, 2)
  },
}))

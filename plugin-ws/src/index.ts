export { WebSocketsPage } from './components/websockets-page'
// Export hooks for external use
export {
  useWebSocketConnections,
  useWebSocketMessages,
} from './hooks/use-websocket-connections'

// Export store for external use
export { useWebSocketStore } from './stores/websocket-store'
// Export types for external use
export type {
  WebSocketConnection,
  WebSocketMessage,
  WebSocketStats,
} from './types/websocket'

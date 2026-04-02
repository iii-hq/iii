import { afterEach } from 'vitest'

const OriginalWebSocket = globalThis.WebSocket

afterEach(() => {
  globalThis.WebSocket = OriginalWebSocket
})

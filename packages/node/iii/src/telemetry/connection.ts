/**
 * Shared WebSocket connection for OpenTelemetry exporters.
 */

import { WebSocket } from 'ws'
import {
  type ConnectionState,
  type ReconnectionConfig,
  DEFAULT_RECONNECTION_CONFIG,
} from './types'

/**
 * Shared WebSocket connection for all OTEL exporters (traces, metrics, logs).
 * Uses a single connection with message prefixes to identify signal type.
 */
export class SharedEngineConnection {
  private static readonly MAX_PENDING_MESSAGES = 1000
  private ws: WebSocket | null = null
  private wsUrl: string
  private connecting = false
  private shuttingDown = false
  private pendingMessages: Array<{ frame: Buffer; callback?: (err?: Error) => void }> = []
  private reconnectAttempt = 0
  private reconnectTimeout: NodeJS.Timeout | null = null
  private config: ReconnectionConfig
  private state: ConnectionState = 'disconnected'
  private onConnectedCallbacks: Array<() => void> = []

  constructor(wsUrl: string, config: Partial<ReconnectionConfig> = {}) {
    this.wsUrl = wsUrl
    this.config = { ...DEFAULT_RECONNECTION_CONFIG, ...config }
    this.connect()
  }

  private connect(): void {
    if (this.connecting || (this.ws && this.ws.readyState === WebSocket.OPEN)) {
      return
    }

    this.connecting = true
    this.state = 'connecting'

    try {
      this.ws = new WebSocket(this.wsUrl)

      this.ws.on('open', () => {
        this.connecting = false
        this.state = 'connected'
        console.log(`[OTel] Connected to engine at ${this.wsUrl}`)

        // Reset reconnection state
        if (this.reconnectAttempt > 0) {
          console.log('[OTel] Successfully reconnected')
        }
        this.reconnectAttempt = 0

        // Clear any pending reconnect timer to prevent race conditions
        if (this.reconnectTimeout) {
          clearTimeout(this.reconnectTimeout)
          this.reconnectTimeout = null
        }

        // Flush pending messages
        const pending = this.pendingMessages.splice(0, this.pendingMessages.length)
        for (const { frame, callback } of pending) {
          this.ws?.send(frame, (err) => callback?.(err))
        }

        // Notify callbacks
        for (const cb of this.onConnectedCallbacks) {
          cb()
        }
      })

      this.ws.on('close', () => {
        this.connecting = false
        this.ws = null
        
        // Skip reconnection if we're shutting down intentionally
        if (this.shuttingDown) {
          this.state = 'disconnected'
          console.log('[OTel] Connection closed during shutdown')
          return
        }
        
        this.state = 'disconnected'
        console.log('[OTel] Disconnected from engine, will reconnect...')
        this.scheduleReconnect()
      })

      this.ws.on('error', (err) => {
        this.connecting = false
        
        // Skip error handling if we're shutting down intentionally
        if (this.shuttingDown) {
          return
        }
        
        console.error('[OTel] WebSocket error:', err.message)
      })
    } catch (err) {
      this.connecting = false
      console.error('[OTel] Connection failed:', err)
      this.scheduleReconnect()
    }
  }

  private scheduleReconnect(): void {
    if (this.config.maxRetries !== -1 && this.reconnectAttempt >= this.config.maxRetries) {
      this.state = 'failed'
      console.error(`[OTel] Max retries (${this.config.maxRetries}) reached, giving up`)

      // Notify pending message callbacks that they won't be delivered
      const pending = this.pendingMessages.splice(0, this.pendingMessages.length)
      const failedError = new Error('Connection failed after max retries')
      for (const { callback } of pending) {
        callback?.(failedError)
      }
      return
    }

    if (this.reconnectTimeout) {
      return // Already scheduled
    }

    const exponentialDelay = this.config.initialDelayMs * Math.pow(this.config.backoffMultiplier, this.reconnectAttempt)
    const cappedDelay = Math.min(exponentialDelay, this.config.maxDelayMs)
    const jitter = cappedDelay * this.config.jitterFactor * (2 * Math.random() - 1)
    const delay = Math.max(0, Math.floor(cappedDelay + jitter))

    this.state = 'reconnecting'
    console.log(`[OTel] Reconnecting in ${delay}ms (attempt ${this.reconnectAttempt + 1})...`)

    this.reconnectTimeout = setTimeout(() => {
      this.reconnectTimeout = null
      this.reconnectAttempt++
      this.connect()
    }, delay)
  }

  /**
   * Send a message with a signal prefix.
   */
  send(prefix: string, data: Uint8Array, callback?: (err?: Error) => void): void {
    const prefixBytes = Buffer.from(prefix, 'utf-8')
    const frame = Buffer.concat([prefixBytes, Buffer.from(data)])

    if (this.ws && this.ws.readyState === WebSocket.OPEN) {
      this.ws.send(frame, callback)
    } else {
      // Queue for later with bounded size to prevent memory exhaustion
      if (this.pendingMessages.length >= SharedEngineConnection.MAX_PENDING_MESSAGES) {
        console.warn('[OTel] Pending message queue full, dropping oldest message')
        const dropped = this.pendingMessages.shift()
        dropped?.callback?.(new Error('Message dropped due to queue overflow'))
      }
      this.pendingMessages.push({ frame, callback })
      this.connect()
    }
  }

  /**
   * Register a callback to be called when connected.
   */
  onConnected(callback: () => void): void {
    this.onConnectedCallbacks.push(callback)
    if (this.state === 'connected') {
      callback()
    }
  }

  /**
   * Get the current connection state.
   */
  getState(): ConnectionState {
    return this.state
  }

  /**
   * Shutdown the connection.
   */
  async shutdown(): Promise<void> {
    // Set shutdown flag to prevent reconnection attempts
    this.shuttingDown = true
    
    if (this.reconnectTimeout) {
      clearTimeout(this.reconnectTimeout)
      this.reconnectTimeout = null
    }

    if (this.ws) {
      this.ws.close()
      this.ws = null
    }

    // Notify pending message callbacks that they won't be delivered
    const pending = this.pendingMessages.splice(0, this.pendingMessages.length)
    const shutdownError = new Error('Connection shutdown before message could be sent')
    for (const { callback } of pending) {
      callback?.(shutdownError)
    }
    this.onConnectedCallbacks = []
    this.state = 'disconnected'
  }
}

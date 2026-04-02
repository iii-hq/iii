import type { SocketAdapter } from '@motiadev/stream-client'
import { type MessageEvent, WebSocket } from 'ws'

export class StreamSocketAdapter implements SocketAdapter {
  private ws: WebSocket

  constructor(
    private address: string,
    protocols?: string | string[] | undefined,
  ) {
    this.ws = new WebSocket(this.address, protocols)
  }

  connect(): void {}

  send(message: string): void {
    this.ws.send(message)
  }

  onMessage(callback: (message: string) => void): void {
    const listener = (message: MessageEvent) => callback(message.data.toString())

    this.ws.addEventListener('message', listener)
  }

  onOpen(callback: () => void): void {
    this.ws.addEventListener('open', callback)
  }

  onClose(callback: () => void): void {
    this.ws.addEventListener('close', callback)
  }

  close(): void {
    this.ws.close()
    this.ws.removeAllListeners()
  }

  isOpen(): boolean {
    return this.ws.readyState === WebSocket.OPEN
  }
}

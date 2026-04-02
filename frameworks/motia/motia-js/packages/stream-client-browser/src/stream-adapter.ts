import type { SocketAdapter } from '@motiadev/stream-client'

export class StreamSocketAdapter implements SocketAdapter {
  private ws: WebSocket

  private onMessageListeners: ((message: MessageEvent<string>) => void)[] = []
  private onOpenListeners: (() => void)[] = []
  private onCloseListeners: (() => void)[] = []

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
    const listener = (message: MessageEvent<string>) => callback(message.data)

    this.ws.addEventListener('message', listener)
    this.onMessageListeners.push(listener)
  }

  onOpen(callback: () => void): void {
    this.ws.addEventListener('open', callback)
    this.onOpenListeners.push(callback)
  }

  onClose(callback: () => void): void {
    this.ws.addEventListener('close', callback)
    this.onCloseListeners.push(callback)
  }

  close(): void {
    this.ws.close()
    this.onMessageListeners.forEach((listener) => this.ws.removeEventListener('message', listener))
    this.onOpenListeners.forEach((listener) => this.ws.removeEventListener('open', listener))
    this.onCloseListeners.forEach((listener) => this.ws.removeEventListener('close', listener))
  }

  isOpen(): boolean {
    return this.ws.readyState === WebSocket.OPEN
  }
}

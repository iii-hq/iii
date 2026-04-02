export interface SocketAdapter {
  connect(): void
  close(): void
  send(message: string): void

  isOpen(): boolean

  onMessage(callback: (message: string) => void): void
  onOpen(callback: () => void): void
  onClose(callback: () => void): void
}

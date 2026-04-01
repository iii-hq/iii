import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { ChannelReader, ChannelWriter } from '../src/channels'
import type { StreamChannelRef } from '../src/iii-types'
import { MockEngine, MockWebSocket } from './mock-websocket'

function makeRef(direction: 'read' | 'write'): StreamChannelRef {
  return {
    channel_id: 'ch-test-123',
    access_key: 'key-abc',
    direction,
  }
}

describe('Channels', () => {
  let engine: MockEngine

  beforeEach(() => {
    engine = new MockEngine({ autoOpen: false })
    engine.install()
  })

  afterEach(() => {
    engine.uninstall()
  })

  describe('ChannelWriter', () => {
    it('should connect and send text message', () => {
      const writer = new ChannelWriter('ws://engine:49135', makeRef('write'))
      writer.sendMessage('hello world')

      const socket = engine.sockets[0]
      expect(socket).toBeDefined()
      expect(socket.url).toContain('/ws/channels/ch-test-123')
      expect(socket.url).toContain('key=key-abc')
      expect(socket.url).toContain('dir=write')

      socket.simulateOpen()

      expect(socket.sentRaw).toContain('hello world')
    })

    it('should send binary data in frames', () => {
      const writer = new ChannelWriter('ws://engine:49135', makeRef('write'))

      const data = new Uint8Array(64 * 1024 + 100)
      data.fill(42)

      writer.sendBinary(data)

      const socket = engine.sockets[0]
      socket.simulateOpen()

      const binarySent = socket.sentRaw.filter((d) => d instanceof ArrayBuffer)
      expect(binarySent.length).toBe(2)
      expect((binarySent[0] as ArrayBuffer).byteLength).toBe(64 * 1024)
      expect((binarySent[1] as ArrayBuffer).byteLength).toBe(100)
    })

    it('should queue messages before open and flush on connect', () => {
      const writer = new ChannelWriter('ws://engine:49135', makeRef('write'))
      writer.sendMessage('msg-1')
      writer.sendMessage('msg-2')

      const socket = engine.sockets[0]
      expect(socket.sentRaw).toHaveLength(0)

      socket.simulateOpen()

      expect(socket.sentRaw).toContain('msg-1')
      expect(socket.sentRaw).toContain('msg-2')
    })

    it('should close with code 1000 and channel_close reason', () => {
      const writer = new ChannelWriter('ws://engine:49135', makeRef('write'))
      writer.sendMessage('init')

      const socket = engine.sockets[0]
      socket.simulateOpen()

      writer.close()

      expect(socket.closeCode).toBe(1000)
      expect(socket.closeReason).toBe('channel_close')
    })
  })

  describe('ChannelReader', () => {
    it('should receive text messages via onMessage', () => {
      const reader = new ChannelReader('ws://engine:49135', makeRef('read'))
      const received: string[] = []
      reader.onMessage((msg) => received.push(msg))

      const socket = engine.sockets[0]
      expect(socket).toBeDefined()
      expect(socket.url).toContain('dir=read')

      socket.simulateOpen()
      socket.simulateMessage('text-message-1')
      socket.simulateMessage('text-message-2')

      expect(received).toEqual(['text-message-1', 'text-message-2'])
    })

    it('should receive binary data via onBinary', () => {
      const reader = new ChannelReader('ws://engine:49135', makeRef('read'))
      const received: Uint8Array[] = []
      reader.onBinary((data) => received.push(data))

      const socket = engine.sockets[0]
      socket.simulateOpen()

      const buffer = new ArrayBuffer(4)
      new Uint8Array(buffer).set([1, 2, 3, 4])
      socket.simulateMessage(buffer)

      expect(received).toHaveLength(1)
      expect(received[0]).toEqual(new Uint8Array([1, 2, 3, 4]))
    })

    it('should collect all binary data with readAll until close', async () => {
      const reader = new ChannelReader('ws://engine:49135', makeRef('read'))
      const readPromise = reader.readAll()

      const socket = engine.sockets[0]
      socket.simulateOpen()

      const buf1 = new ArrayBuffer(3)
      new Uint8Array(buf1).set([10, 20, 30])
      socket.simulateMessage(buf1)

      const buf2 = new ArrayBuffer(2)
      new Uint8Array(buf2).set([40, 50])
      socket.simulateMessage(buf2)

      socket.simulateClose()

      const result = await readPromise
      expect(result).toEqual(new Uint8Array([10, 20, 30, 40, 50]))
    })

    it('should close reader with code 1000', () => {
      const reader = new ChannelReader('ws://engine:49135', makeRef('read'))
      reader.onMessage(() => {})

      const socket = engine.sockets[0]
      socket.simulateOpen()

      reader.close()

      expect(socket.closeCode).toBe(1000)
      expect(socket.closeReason).toBe('channel_close')
    })
  })

  describe('URL construction', () => {
    it('should build correct channel URL', () => {
      const writer = new ChannelWriter('ws://engine:49135', {
        channel_id: 'my-channel',
        access_key: 'secret key&value',
        direction: 'write',
      })
      writer.sendMessage('init')

      const socket = engine.sockets[0]
      expect(socket.url).toBe(
        'ws://engine:49135/ws/channels/my-channel?key=secret%20key%26value&dir=write',
      )
    })
  })
})

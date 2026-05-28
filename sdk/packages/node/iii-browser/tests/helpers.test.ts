import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import {
  ChannelDirection,
  ChannelItem,
  createChannel,
  createStream,
  extractChannelRefs,
  isChannelRef,
} from '../src/helpers'
import { registerWorker } from '../src/iii'
import { MockEngine } from './mock-websocket'

describe('helpers module', () => {
  it('exposes channel utilities and types', () => {
    expect(typeof isChannelRef).toBe('function')
    expect(typeof extractChannelRefs).toBe('function')
    expect(isChannelRef({})).toBe(false)
    expect(extractChannelRefs({})).toEqual([])
    expect(ChannelDirection).toBeDefined()
    expect(ChannelItem).toBeDefined()
  })

  it('exposes free functions taking iii as first arg', () => {
    expect(createChannel.length).toBe(2)
    expect(createStream.length).toBe(3)
  })
})

describe('Browser ISdk public surface', () => {
  let engine: MockEngine

  beforeEach(() => {
    engine = new MockEngine()
    engine.install()
  })

  afterEach(() => {
    engine.uninstall()
  })

  it('no longer exposes relocated methods', async () => {
    const iii = registerWorker('ws://localhost:9') as unknown as Record<string, unknown>
    try {
      expect(iii.createChannel).toBeUndefined()
      expect(iii.createStream).toBeUndefined()
    } finally {
      await (iii as unknown as { shutdown(): Promise<void> }).shutdown()
    }
  })
})

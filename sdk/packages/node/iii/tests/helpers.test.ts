import { describe, expect, it } from 'vitest'

import * as helpers from '../src/helpers'
import { createStream } from '../src/helpers'
import { registerWorker } from '../src/iii'
import { ChannelDirection, ChannelItem, extractChannelRefs, isChannelRef } from '../src/protocol'

describe('helpers module', () => {
  it('exposes free functions taking iii as first arg', () => {
    expect(createStream.length).toBe(3)
  })

  it('no longer exposes channels: createChannel is gone, the wire helpers moved to iii-sdk/protocol', () => {
    const names = helpers as unknown as Record<string, unknown>
    for (const removed of ['createChannel', 'isChannelRef', 'extractChannelRefs', 'ChannelDirection', 'ChannelItem']) {
      expect(names[removed]).toBeUndefined()
    }
  })
})

describe('protocol module', () => {
  it('exposes the channel wire helpers', () => {
    expect(isChannelRef({})).toBe(false)
    expect(isChannelRef({ channel_id: 'c', access_key: 'k', direction: 'read' })).toBe(true)
    expect(extractChannelRefs({})).toEqual([])
    expect(ChannelDirection).toEqual({ Read: 'read', Write: 'write' })
    expect(ChannelItem.Text('hi')).toEqual({ type: 'text', value: 'hi' })
  })
})

describe('IIIClient public surface', () => {
  it('no longer exposes relocated or removed methods', async () => {
    const iii = registerWorker('ws://localhost:9') as unknown as Record<string, unknown>
    try {
      expect(iii.createChannel).toBeUndefined()
      expect(iii.createStream).toBeUndefined()
      expect(iii.__helpers_create_channel).toBeUndefined()
      expect(typeof iii.pipe).toBe('function')
    } finally {
      await (iii as unknown as { shutdown(): Promise<void> }).shutdown()
    }
  })
})

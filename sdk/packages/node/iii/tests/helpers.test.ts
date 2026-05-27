import { describe, expect, it } from 'vitest'

import {
  ChannelDirection,
  ChannelItem,
  createChannel,
  createStream,
  extractChannelRefs,
  isChannelRef,
  registerTriggerType,
  unregisterTriggerType,
} from '../src/helpers'

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
    expect(registerTriggerType.length).toBe(3)
    expect(unregisterTriggerType.length).toBe(2)
  })
})

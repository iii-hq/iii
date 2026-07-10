import { describe, expect, it } from 'vitest'
import { TriggerAction } from '../src/index'

// Contract test for the SDK-owned TriggerAction builders. The queue *behavior*
// (delivery, ordering, DLQ, durable fan-out) is exercised by the standalone
// queue worker's own suite; here we only pin the wire shape the engine's
// `TriggerAction` deserialization depends on, so it needs no live engine.
describe('TriggerAction builders', () => {
  it('Enqueue serializes to { type: "enqueue", queue }', () => {
    expect(TriggerAction.Enqueue({ queue: 'orders' })).toEqual({
      type: 'enqueue',
      queue: 'orders',
    })
  })

  it('Void serializes to { type: "void" }', () => {
    expect(TriggerAction.Void()).toEqual({ type: 'void' })
  })
})

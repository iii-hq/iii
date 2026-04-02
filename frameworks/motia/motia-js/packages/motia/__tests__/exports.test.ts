jest.mock('uuid', () => ({
  v5: jest.fn(() => 'mocked-uuid'),
}))

import * as motia from '../src/index'

describe('public exports', () => {
  // Trigger helpers
  it('exports api trigger helper', () => expect(motia.api).toBeDefined())
  it('exports queue trigger helper', () => expect(motia.queue).toBeDefined())
  it('exports cron trigger helper', () => expect(motia.cron).toBeDefined())
  it('exports state trigger helper', () => expect(motia.state).toBeDefined())
  it('exports stream trigger helper', () => expect(motia.stream).toBeDefined())

  // Step builders
  it('exports multiTriggerStep', () => expect(motia.multiTriggerStep).toBeDefined())
  it('exports step', () => expect(motia.step).toBeDefined())

  // Schema
  it('exports jsonSchema', () => expect(motia.jsonSchema).toBeDefined())

  // Re-exports from iii-sdk
  it('does not export removed getContext helper', () => {
    expect('getContext' in motia).toBe(false)
  })
  it('exports Logger', () => expect(motia.Logger).toBeDefined())

  // Core runtime utilities
  it('exports generateStepId', () => expect(motia.generateStepId).toBeDefined())
  it('exports Motia', () => expect(motia.Motia).toBeDefined())
  it('exports getInstance', () => expect(motia.getInstance).toBeDefined())
  it('exports initIII', () => expect(motia.initIII).toBeDefined())
  it('exports setupStepEndpoint', () => expect(motia.setupStepEndpoint).toBeDefined())
  it('exports StateManager', () => expect(motia.StateManager).toBeDefined())
  it('exports stateManager', () => expect(motia.stateManager).toBeDefined())
  it('exports Stream', () => expect(motia.Stream).toBeDefined())
})

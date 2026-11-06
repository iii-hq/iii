// Derived from sdk/packages/node/iii/tests/observe-deprecation.test.ts by scripts/sync-browser-observe.py; edit those, not this file.
import { afterEach, describe, expect, it, vi } from 'vitest'
import { registerWorker } from '../src/index'
import type { ISdk } from '../src/types'
import * as fake from './observe-fake-engine'

/** CONTRACT §5: the trigger APIs keep working and warn once per process per API. */
const REGISTER_TRIGGER =
  'registerTrigger is deprecated and will be removed in a future release. Use worker.observe(source, target) instead. Migration guide: https://iii.dev/docs/upgrading/from-0-24-x'
const REGISTER_TRIGGER_TYPE =
  'registerTriggerType is deprecated and will be removed in a future release. Register a function with an observe descriptor (registerFunction(id, handler, { observe: { ... } })) instead. Migration guide: https://iii.dev/docs/upgrading/from-0-24-x'

const sdks: ISdk[] = []
afterEach(async () => {
  vi.restoreAllMocks()
  for (const sdk of sdks.splice(0)) await sdk.shutdown()
})

function worker() {
  const sdk = registerWorker('ws://deprecation.test')
  sdks.push(sdk)
  const socket = fake.lastSocket()
  socket.open()
  return { sdk, socket }
}

describe('deprecated trigger APIs', () => {
  it('warn once per process per API with the migration notice, and keep working', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined)
    const { sdk, socket } = worker()
    const handler = { registerTrigger: vi.fn(async () => undefined), unregisterTrigger: vi.fn(async () => undefined) }

    const type = sdk.registerTriggerType({ id: 'webhook', description: 'Webhooks' }, handler)
    sdk.registerTriggerType({ id: 'webhook-2', description: 'Webhooks 2' }, handler)
    const trigger = sdk.registerTrigger({ type: 'webhook', function_id: 'hooks::in', config: { path: '/a' } })
    sdk.registerTrigger({ type: 'webhook', function_id: 'hooks::in', config: { path: '/b' } })
    type.registerTrigger('hooks::in', { path: '/c' })

    // a second worker in the same process does not warn again
    const other = worker()
    other.sdk.registerTrigger({ type: 'webhook', function_id: 'hooks::in', config: {} })
    other.sdk.registerTriggerType({ id: 'webhook-3', description: '' }, handler)

    const messages = warn.mock.calls.map((c) => String(c[0]))
    expect(messages.filter((m) => m.includes(REGISTER_TRIGGER))).toHaveLength(1)
    expect(messages.filter((m) => m.includes(REGISTER_TRIGGER_TYPE))).toHaveLength(1)

    // still working: frames go out, the provider callbacks still run
    expect(socket.framesOf('registertriggertype').map((f) => f.id)).toEqual(['webhook', 'webhook-2'])
    expect(socket.framesOf('registertrigger')).toHaveLength(3)
    socket.deliver({ type: 'registertrigger', trigger_type: 'webhook', id: 't-1', function_id: 'x::y', config: {} })
    await fake.settle()
    expect(handler.registerTrigger).toHaveBeenCalledTimes(1)
    expect(socket.framesOf('triggerregistrationresult')).toHaveLength(1)
    trigger.unregister()
    expect(socket.framesOf('unregistertrigger')).toHaveLength(1)

    // unregisterTriggerType carries the marker only: no runtime warning, and it sends `{ id }`
    type.unregister()
    expect(socket.framesOf('unregistertriggertype')).toEqual([{ type: 'unregistertriggertype', id: 'webhook' }])
    type.registerFunction('hooks::fn', async () => null, { path: '/d' })
    expect(socket.framesOf('registertrigger')).toHaveLength(4)
    const after = warn.mock.calls.map((c) => String(c[0]))
    expect(after.filter((m) => m.includes('deprecated'))).toHaveLength(2)
  })
})

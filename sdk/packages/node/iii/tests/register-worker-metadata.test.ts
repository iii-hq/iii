import * as fs from 'node:fs'
import * as os from 'node:os'
import * as path from 'node:path'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { registerWorker, TriggerAction } from '../src/iii'
import { MessageType } from '../src/iii-types'

type InternalSdk = {
  trigger: ReturnType<typeof vi.fn>
  registerWorkerMetadata: () => void
}

describe('registerWorkerMetadata — isolation field', () => {
  let previous: string | undefined

  beforeEach(() => {
    previous = process.env.III_ISOLATION
    delete process.env.III_ISOLATION
  })

  afterEach(() => {
    if (previous === undefined) {
      delete process.env.III_ISOLATION
    } else {
      process.env.III_ISOLATION = previous
    }
  })

  it('sets isolation to null when III_ISOLATION is unset', () => {
    const sdk = registerWorker('ws://127.0.0.1:0') as unknown as InternalSdk
    sdk.trigger = vi.fn()

    sdk.registerWorkerMetadata()

    expect(sdk.trigger).toHaveBeenCalledOnce()
    const call = sdk.trigger.mock.calls[0][0]
    expect(call.payload.isolation).toBeNull()
  })

  it('forwards the III_ISOLATION value into the payload', () => {
    process.env.III_ISOLATION = 'kubernetes'
    const sdk = registerWorker('ws://127.0.0.1:0') as unknown as InternalSdk
    sdk.trigger = vi.fn()

    sdk.registerWorkerMetadata()

    expect(sdk.trigger).toHaveBeenCalledOnce()
    const call = sdk.trigger.mock.calls[0][0]
    expect(call.payload.isolation).toBe('kubernetes')
  })

  it('maps empty-string III_ISOLATION to null', () => {
    process.env.III_ISOLATION = ''
    const sdk = registerWorker('ws://127.0.0.1:0') as unknown as InternalSdk
    sdk.trigger = vi.fn()

    sdk.registerWorkerMetadata()

    expect(sdk.trigger).toHaveBeenCalledOnce()
    const call = sdk.trigger.mock.calls[0][0]
    expect(call.payload.isolation).toBeNull()
  })
})

describe('registerWorkerMetadata — worker name', () => {
  let previous: string | undefined

  beforeEach(() => {
    previous = process.env.III_WORKER_NAME
    delete process.env.III_WORKER_NAME
  })

  afterEach(() => {
    if (previous === undefined) {
      delete process.env.III_WORKER_NAME
    } else {
      process.env.III_WORKER_NAME = previous
    }
  })

  it('defaults the name from III_WORKER_NAME when no workerName option is set', () => {
    process.env.III_WORKER_NAME = 'managed-worker'
    const sdk = registerWorker('ws://127.0.0.1:0') as unknown as InternalSdk
    sdk.trigger = vi.fn()

    sdk.registerWorkerMetadata()

    const call = sdk.trigger.mock.calls[0][0]
    expect(call.payload.name).toBe('managed-worker')
  })

  it('explicit workerName option wins over III_WORKER_NAME', () => {
    process.env.III_WORKER_NAME = 'managed-worker'
    const sdk = registerWorker('ws://127.0.0.1:0', {
      workerName: 'explicit-name',
    }) as unknown as InternalSdk
    sdk.trigger = vi.fn()

    sdk.registerWorkerMetadata()

    const call = sdk.trigger.mock.calls[0][0]
    expect(call.payload.name).toBe('explicit-name')
  })

  it('falls back to hostname:pid when III_WORKER_NAME is unset', () => {
    const sdk = registerWorker('ws://127.0.0.1:0') as unknown as InternalSdk
    sdk.trigger = vi.fn()

    sdk.registerWorkerMetadata()

    const call = sdk.trigger.mock.calls[0][0]
    expect(call.payload.name).toBe(`${os.hostname()}:${process.pid}`)
  })

  it('ignores an empty III_WORKER_NAME', () => {
    process.env.III_WORKER_NAME = ''
    const sdk = registerWorker('ws://127.0.0.1:0') as unknown as InternalSdk
    sdk.trigger = vi.fn()

    sdk.registerWorkerMetadata()

    const call = sdk.trigger.mock.calls[0][0]
    expect(call.payload.name).toBe(`${os.hostname()}:${process.pid}`)
  })
})

describe('registerWorkerMetadata — project_name auto-detection', () => {
  let tmpDir: string
  let originalCwd: string

  beforeEach(() => {
    originalCwd = process.cwd()
    tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'iii-project-name-'))
    process.chdir(tmpDir)
  })

  afterEach(() => {
    process.chdir(originalCwd)
    fs.rmSync(tmpDir, { recursive: true, force: true })
  })

  it('reads project_name from package.json in cwd', () => {
    fs.writeFileSync(path.join(tmpDir, 'package.json'), JSON.stringify({ name: '@scope/my-app' }))
    const sdk = registerWorker('ws://127.0.0.1:0') as unknown as InternalSdk
    sdk.trigger = vi.fn()

    sdk.registerWorkerMetadata()

    const call = sdk.trigger.mock.calls[0][0]
    expect(call.payload.telemetry.project_name).toBe('@scope/my-app')
  })

  it('falls back to cwd basename when package.json is absent', () => {
    const sdk = registerWorker('ws://127.0.0.1:0') as unknown as InternalSdk
    sdk.trigger = vi.fn()

    sdk.registerWorkerMetadata()

    const call = sdk.trigger.mock.calls[0][0]
    expect(call.payload.telemetry.project_name).toBe(path.basename(tmpDir))
  })

  it('falls back to cwd basename when package.json has no name field', () => {
    fs.writeFileSync(path.join(tmpDir, 'package.json'), JSON.stringify({ version: '1.0.0' }))
    const sdk = registerWorker('ws://127.0.0.1:0') as unknown as InternalSdk
    sdk.trigger = vi.fn()

    sdk.registerWorkerMetadata()

    const call = sdk.trigger.mock.calls[0][0]
    expect(call.payload.telemetry.project_name).toBe(path.basename(tmpDir))
  })

  it('falls back to cwd basename when package.json is malformed', () => {
    fs.writeFileSync(path.join(tmpDir, 'package.json'), '{ not json')
    const sdk = registerWorker('ws://127.0.0.1:0') as unknown as InternalSdk
    sdk.trigger = vi.fn()

    sdk.registerWorkerMetadata()

    const call = sdk.trigger.mock.calls[0][0]
    expect(call.payload.telemetry.project_name).toBe(path.basename(tmpDir))
  })

  it('user-provided telemetry.project_name overrides auto-detection', () => {
    fs.writeFileSync(path.join(tmpDir, 'package.json'), JSON.stringify({ name: 'auto-detected' }))
    const sdk = registerWorker('ws://127.0.0.1:0', {
      telemetry: { project_name: 'explicit-override' },
    }) as unknown as InternalSdk
    sdk.trigger = vi.fn()

    sdk.registerWorkerMetadata()

    const call = sdk.trigger.mock.calls[0][0]
    expect(call.payload.telemetry.project_name).toBe('explicit-override')
  })
})

describe('registerWorkerMetadata — namespace', () => {
  let previous: string | undefined

  beforeEach(() => {
    previous = process.env.III_NAMESPACE
    delete process.env.III_NAMESPACE
  })

  afterEach(() => {
    if (previous === undefined) {
      delete process.env.III_NAMESPACE
    } else {
      process.env.III_NAMESPACE = previous
    }
  })

  it('forwards the III_NAMESPACE value into the payload', () => {
    process.env.III_NAMESPACE = 'orders'
    const sdk = registerWorker('ws://127.0.0.1:0') as unknown as InternalSdk
    sdk.trigger = vi.fn()

    sdk.registerWorkerMetadata()

    const call = sdk.trigger.mock.calls[0][0]
    expect(call.payload.namespace).toBe('orders')
  })

  it('explicit namespace option wins over III_NAMESPACE', () => {
    process.env.III_NAMESPACE = 'orders'
    const sdk = registerWorker('ws://127.0.0.1:0', {
      namespace: 'payments',
    }) as unknown as InternalSdk
    sdk.trigger = vi.fn()

    sdk.registerWorkerMetadata()

    const call = sdk.trigger.mock.calls[0][0]
    expect(call.payload.namespace).toBe('payments')
  })

  it('omits namespace when neither option nor III_NAMESPACE is set', () => {
    const sdk = registerWorker('ws://127.0.0.1:0') as unknown as InternalSdk
    sdk.trigger = vi.fn()

    sdk.registerWorkerMetadata()

    const call = sdk.trigger.mock.calls[0][0]
    expect('namespace' in call.payload).toBe(false)
  })

  it('ignores an empty III_NAMESPACE', () => {
    process.env.III_NAMESPACE = ''
    const sdk = registerWorker('ws://127.0.0.1:0') as unknown as InternalSdk
    sdk.trigger = vi.fn()

    sdk.registerWorkerMetadata()

    const call = sdk.trigger.mock.calls[0][0]
    expect('namespace' in call.payload).toBe(false)
  })
})

describe('trigger — namespace targeting', () => {
  type TriggerSdk = {
    trigger: (req: {
      function_id: string
      payload: unknown
      action?: unknown
      namespace?: string
    }) => Promise<unknown>
    messagesToSend: Record<string, unknown>[]
  }

  it('serializes namespace into the InvokeFunction message', () => {
    const sdk = registerWorker('ws://127.0.0.1:0') as unknown as TriggerSdk

    sdk.trigger({
      function_id: 'orders::get',
      payload: { id: 1 },
      action: TriggerAction.Void(),
      namespace: 'orders',
    })

    const invoke = sdk.messagesToSend.find((m) => m.type === MessageType.InvokeFunction)
    expect(invoke?.namespace).toBe('orders')
  })

  it('omits namespace from the InvokeFunction message when not provided', () => {
    const sdk = registerWorker('ws://127.0.0.1:0') as unknown as TriggerSdk

    sdk.trigger({
      function_id: 'orders::get',
      payload: { id: 1 },
      action: TriggerAction.Void(),
    })

    const invoke = sdk.messagesToSend.find((m) => m.type === MessageType.InvokeFunction)
    expect(invoke).toBeDefined()
    expect('namespace' in (invoke as object)).toBe(false)
  })
})

describe('registrationrejected — fatal, no reconnect', () => {
  type RejectableSdk = {
    trigger: (req: { function_id: string; payload: unknown; timeoutMs?: number }) => Promise<unknown>
    onMessage: (data: string) => void
    isShuttingDown: boolean
    connectionState: string
  }

  it('rejects pending invocations and stops the worker without reconnecting', async () => {
    const sdk = registerWorker('ws://127.0.0.1:0') as unknown as RejectableSdk

    const pending = sdk.trigger({ function_id: 'orders::get', payload: {}, timeoutMs: 5000 })

    sdk.onMessage(
      JSON.stringify({
        type: 'registrationrejected',
        code: 'worker_name_conflict',
        namespace: 'orders',
        worker_name: 'w1',
        owner_worker_id: 'abc',
      }),
    )

    await expect(pending).rejects.toThrow(/worker_name_conflict/)
    expect(sdk.isShuttingDown).toBe(true)
    expect(sdk.connectionState).toBe('failed')
  })
})

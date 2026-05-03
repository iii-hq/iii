import * as fs from 'node:fs'
import * as os from 'node:os'
import * as path from 'node:path'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { registerWorker } from '../src/iii'

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

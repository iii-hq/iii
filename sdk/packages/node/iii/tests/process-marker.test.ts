import * as fs from 'node:fs'
import * as os from 'node:os'
import * as path from 'node:path'
import { describe, expect, it } from 'vitest'
import { announceProcess } from '../src/process-marker'

const read = (dir: string, file: string) =>
  JSON.parse(fs.readFileSync(path.join(dir, file), 'utf8'))

describe('announceProcess', () => {
  it('names this process until the marker is removed', () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'iii-marker-'))

    const remove = announceProcess('ws://127.0.0.1:49134', 'orders', dir)
    const [file] = fs.readdirSync(dir)
    expect(file).toMatch(new RegExp(`^worker-${process.pid}-\\d+\\.json$`))
    expect(read(dir, file)).toMatchObject({
      pid: process.pid,
      url: 'ws://127.0.0.1:49134',
      name: 'orders',
      sdk: 'node',
    })

    remove()
    expect(fs.readdirSync(dir)).toEqual([])
    fs.rmSync(dir, { recursive: true, force: true })
  })

  it('gives each client in a process its own marker', () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'iii-marker-'))

    const orders = announceProcess('ws://127.0.0.1:49134', 'orders', dir)
    const billing = announceProcess('ws://127.0.0.1:49134', 'billing', dir)
    orders()
    const left = fs.readdirSync(dir)
    expect(left.map(file => read(dir, file).name)).toEqual(['billing'])

    billing()
    fs.rmSync(dir, { recursive: true, force: true })
  })

  it('never throws when the directory cannot be written', () => {
    const blocker = path.join(fs.mkdtempSync(path.join(os.tmpdir(), 'iii-marker-')), 'file')
    fs.writeFileSync(blocker, '')
    expect(() => announceProcess('ws://x', 'y', path.join(blocker, 'run'))()).not.toThrow()
  })
})

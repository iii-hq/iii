import { http, queue } from '../src/triggers'
import { generateEnqueuesInterface, generateStreamsInterface, generateTypesString } from '../src/types/generate-types'

describe('generate-types', () => {
  describe('generateStreamsInterface', () => {
    it('generates interface with typed stream entries', () => {
      const streams = {
        users: {
          filePath: 'users.stream.ts',
          config: {
            name: 'users',
            schema: { type: 'object', properties: { id: { type: 'string' }, name: { type: 'string' } } },
            baseConfig: { storageType: 'default' },
          },
        },
      }
      const result = generateStreamsInterface(streams as any)
      expect(result).toContain('interface Streams')
      expect(result).toContain('users: MotiaStream<')
      expect(result).toContain('id')
      expect(result).toContain('name')
    })

    it('filters out hidden streams', () => {
      const streams = {
        visible: {
          filePath: 'v.stream.ts',
          config: { name: 'v', schema: { type: 'string' }, baseConfig: { storageType: 'default' } },
        },
        hidden: {
          filePath: 'h.stream.ts',
          config: { name: 'h', schema: { type: 'string' }, baseConfig: { storageType: 'default' } },
          hidden: true,
        },
      }
      const result = generateStreamsInterface(streams as any)
      expect(result).toContain('visible')
      expect(result).not.toContain('hidden')
    })

    it('returns empty interface when no visible streams', () => {
      const streams = {
        h: {
          filePath: 'h.stream.ts',
          config: { name: 'h', schema: { type: 'string' }, baseConfig: { storageType: 'default' } },
          hidden: true,
        },
      }
      const result = generateStreamsInterface(streams as any)
      expect(result).toBe('  interface Streams {}')
    })
  })

  describe('generateEnqueuesInterface', () => {
    it('collects queue topics from step triggers', () => {
      const inputSchema = { type: 'object' as const, properties: { id: { type: 'string' as const } } }
      const steps = [
        {
          filePath: 's.step.ts',
          config: {
            name: 's',
            triggers: [queue('orders', { input: inputSchema })],
          },
        },
      ]
      const result = generateEnqueuesInterface(steps as any)
      expect(result).toContain("'orders'")
      expect(result).toContain('id')
    })

    it('includes explicit enqueues from config', () => {
      const steps = [
        {
          filePath: 's.step.ts',
          config: {
            name: 's',
            triggers: [],
            enqueues: ['events'],
          },
        },
      ]
      const result = generateEnqueuesInterface(steps as any)
      expect(result).toContain("'events'")
    })

    it('returns empty interface when no enqueues', () => {
      const steps = [
        {
          filePath: 's.step.ts',
          config: { name: 's', triggers: [http('GET', '/x')] },
        },
      ]
      const result = generateEnqueuesInterface(steps as any)
      expect(result).toBe('  interface Enqueues {}')
    })
  })

  describe('generateTypesString', () => {
    it('generates full declaration file with both interfaces', () => {
      const steps = [
        {
          filePath: 's.step.ts',
          config: {
            name: 's',
            triggers: [queue('tasks', { input: { type: 'string' } })],
          },
        },
      ]
      const streams = {
        items: {
          filePath: 'i.stream.ts',
          config: { name: 'items', schema: { type: 'string' }, baseConfig: { storageType: 'default' } },
        },
      }
      const result = generateTypesString(steps as any, streams as any)
      expect(result).toContain('Automatically generated types')
      expect(result).toContain("import { MotiaStream } from 'motia'")
      expect(result).toContain("declare module 'motia'")
      expect(result).toContain('interface Streams')
      expect(result).toContain('interface Enqueues')
      expect(result).toContain('items')
      expect(result).toContain("'tasks'")
    })
  })
})

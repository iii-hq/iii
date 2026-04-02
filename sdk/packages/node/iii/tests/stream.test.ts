import { beforeEach, describe, expect, it } from 'vitest'
import { iii, sleep } from './utils'
import type { StreamSetInput, StreamSetResult } from '../src/stream'

type TestData = {
  name?: string
  value: number
  updated?: boolean
}

describe('Stream Operations', () => {
  const testStreamName = 'test-stream'
  const testGroupId = 'test-group'
  const testItemId = 'test-item'

  beforeEach(async () => {
    await iii
      .trigger({ function_id: 'stream::delete', payload: {
        stream_name: testStreamName,
        group_id: testGroupId,
        item_id: testItemId,
      } })
      .catch(() => void 0)
  })

  describe('stream::set', () => {
    it('should set a new stream item', async () => {
      const testData = {
        name: 'Test Item',
        value: 42,
        metadata: { created: new Date().toISOString() },
      }

      const result = await iii.trigger<StreamSetInput, StreamSetResult<TestData>>({ function_id: 'stream::set', payload: {
        stream_name: testStreamName,
        group_id: testGroupId,
        item_id: testItemId,
        data: testData,
      } })

      expect(result).toBeDefined()
      expect(result).toEqual({ old_value: null, new_value: testData })
    })

    it('should overwrite an existing stream item', async () => {
      const initialData: TestData = { value: 1 }
      const updatedData: TestData = { value: 2, updated: true }

      await iii.trigger({ function_id: 'stream::set', payload: {
        stream_name: testStreamName,
        group_id: testGroupId,
        item_id: testItemId,
        data: initialData,
      } })

      const result: StreamSetResult<TestData> = await iii.trigger({ function_id: 'stream::set', payload: {
        stream_name: testStreamName,
        group_id: testGroupId,
        item_id: testItemId,
        data: updatedData,
      } })

      expect(result.old_value).toEqual(initialData)
      expect(result.new_value).toEqual(updatedData)
    })
  })

  describe('stream::get', () => {
    it('should get an existing stream item', async () => {
      const testData: TestData = { name: 'Test', value: 100 }

      await iii.trigger({ function_id: 'stream::set', payload: {
        stream_name: testStreamName,
        group_id: testGroupId,
        item_id: testItemId,
        data: testData,
      } })

      const result: TestData = await iii.trigger({ function_id: 'stream::get', payload: {
        stream_name: testStreamName,
        group_id: testGroupId,
        item_id: testItemId,
      } })

      expect(result).toBeDefined()
      expect(result).toEqual(testData)
    })

    it('should return null for non-existent item', async () => {
      const result = await iii.trigger({ function_id: 'stream::get', payload: {
        stream_name: testStreamName,
        group_id: testGroupId,
        item_id: 'non-existent-item',
      } })

      expect(result).toBeUndefined()
    })
  })

  describe('stream::delete', () => {
    it('should delete an existing stream item', async () => {
      await iii.trigger({ function_id: 'stream::set', payload: {
        stream_name: testStreamName,
        group_id: testGroupId,
        item_id: testItemId,
        data: { test: true },
      } })

      await iii.trigger({ function_id: 'stream::delete', payload: {
        stream_name: testStreamName,
        group_id: testGroupId,
        item_id: testItemId,
      } })

      const result = await iii.trigger({ function_id: 'stream::get', payload: {
        stream_name: testStreamName,
        group_id: testGroupId,
        item_id: testItemId,
      } })

      expect(result).toBeUndefined()
    })

    it('should handle deleting non-existent item gracefully', async () => {
      await expect(
        iii.trigger({ function_id: 'stream::delete', payload: {
          stream_name: testStreamName,
          group_id: testGroupId,
          item_id: 'non-existent',
        } }),
      ).resolves.not.toThrow()
    })
  })

  describe('stream::list', () => {
    it('should get all items in a group', async () => {
      type TestDataWithId = TestData & { id: string }

      const groupId = `stream-${Date.now()}`
      const items: TestDataWithId[] = [
        { id: 'stream-item1', value: 1 },
        { id: 'stream-item2', value: 2 },
        { id: 'stream-item3', value: 3 },
      ]

      // Set multiple items
      for (const item of items) {
        await iii.trigger({ function_id: 'stream::set', payload: {
          stream_name: testStreamName,
          group_id: groupId,
          item_id: item.id,
          data: item,
        } })
      }

      const result: TestDataWithId[] = await iii.trigger({ function_id: 'stream::list', payload: {
        stream_name: testStreamName,
        group_id: groupId,
      } })
      const sort = (a: TestDataWithId, b: TestDataWithId) => a.id.localeCompare(b.id)

      expect(Array.isArray(result)).toBe(true)
      expect(result.length).toBeGreaterThanOrEqual(items.length)
      expect(result.sort(sort)).toEqual(items.sort(sort))
    })
  })

  describe('stream custom operations', () => {
    it('should perform a custom operation on a stream item', async () => {
      const testStreamName = `test-stream-${Date.now()}`
      const state: Map<string, TestData> = new Map()

      iii.createStream(testStreamName, {
        get: async input => state.get(`${input.group_id}::${input.item_id}`),
        set: async input => {
          const key = `${input.group_id}::${input.item_id}`
          const oldValue = state.get(key)
          state.set(key, input.data)

          return { old_value: oldValue, new_value: input.data }
        },
        delete: async input => {
          const oldValue = state.get(`${input.group_id}::${input.item_id}`)
          state.delete(`${input.group_id}::${input.item_id}`)
          return { old_value: oldValue }
        },
        list: async input => {
          return Array.from(state.keys())
            .filter(key => key.startsWith(`${input.group_id}::`))
            .map(key => state.get(key))
        },
        listGroups: async () => Array.from(state.keys()),
        update: async () => {
          throw new Error('Not implemented')
        },
      })

      await sleep(1_000)

      const testData: TestData = { name: 'Test', value: 100 }
      const getArgs = {
        stream_name: testStreamName,
        group_id: testGroupId,
        item_id: testItemId,
      }

      await iii.trigger({ function_id: 'stream::set', payload: { ...getArgs, data: testData } })

      expect(state.get(`${testGroupId}::${testItemId}`)).toEqual(testData)

      await expect(iii.trigger({ function_id: 'stream::get', payload: getArgs })).resolves.toEqual(testData)
      await iii.trigger({ function_id: 'stream::delete', payload: getArgs })
      await expect(iii.trigger({ function_id: 'stream::get', payload: getArgs })).resolves.toEqual(undefined)
    })
  })
})

import { beforeEach, describe, expect, it } from 'vitest'
import type { StreamSetInput, StreamSetResult } from '../src/stream'
import { bridgeIII } from './bridge-utils'

type TestData = {
  name?: string
  value: number
  updated?: boolean
}

describe('Bridge Stream Operations', () => {
  const testStreamName = 'bridge-test-stream'
  const testGroupId = 'bridge-test-group'
  const testItemId = 'bridge-test-item'

  beforeEach(async () => {
    await bridgeIII
      .trigger({
        function_id: 'stream::delete',
        payload: { stream_name: testStreamName, group_id: testGroupId, item_id: testItemId },
      })
      .catch(() => void 0)
  })

  describe('stream::set via bridge', () => {
    it('should set a new stream item', async () => {
      const testData = { name: 'Test Item', value: 42 }

      const result = await bridgeIII.trigger<StreamSetInput, StreamSetResult<TestData>>({
        function_id: 'stream::set',
        payload: { stream_name: testStreamName, group_id: testGroupId, item_id: testItemId, data: testData },
      })

      expect(result).toBeDefined()
      expect(result).toEqual({ old_value: null, new_value: testData })
    })

    it('should overwrite an existing stream item', async () => {
      const initialData: TestData = { value: 1 }
      const updatedData: TestData = { value: 2, updated: true }

      await bridgeIII.trigger({
        function_id: 'stream::set',
        payload: { stream_name: testStreamName, group_id: testGroupId, item_id: testItemId, data: initialData },
      })

      const result: StreamSetResult<TestData> = await bridgeIII.trigger({
        function_id: 'stream::set',
        payload: { stream_name: testStreamName, group_id: testGroupId, item_id: testItemId, data: updatedData },
      })

      expect(result.old_value).toEqual(initialData)
      expect(result.new_value).toEqual(updatedData)
    })
  })

  describe('stream::get via bridge', () => {
    it('should get an existing stream item', async () => {
      const testData: TestData = { name: 'Test', value: 100 }

      await bridgeIII.trigger({
        function_id: 'stream::set',
        payload: { stream_name: testStreamName, group_id: testGroupId, item_id: testItemId, data: testData },
      })

      const result: TestData = await bridgeIII.trigger({
        function_id: 'stream::get',
        payload: { stream_name: testStreamName, group_id: testGroupId, item_id: testItemId },
      })

      expect(result).toBeDefined()
      expect(result).toEqual(testData)
    })

    it('should return undefined for non-existent item', async () => {
      const result = await bridgeIII.trigger({
        function_id: 'stream::get',
        payload: { stream_name: testStreamName, group_id: testGroupId, item_id: 'non-existent-item' },
      })

      expect(result).toBeUndefined()
    })
  })

  describe('stream::delete via bridge', () => {
    it('should delete an existing stream item', async () => {
      await bridgeIII.trigger({
        function_id: 'stream::set',
        payload: { stream_name: testStreamName, group_id: testGroupId, item_id: testItemId, data: { test: true } },
      })

      await bridgeIII.trigger({
        function_id: 'stream::delete',
        payload: { stream_name: testStreamName, group_id: testGroupId, item_id: testItemId },
      })

      const result = await bridgeIII.trigger({
        function_id: 'stream::get',
        payload: { stream_name: testStreamName, group_id: testGroupId, item_id: testItemId },
      })

      expect(result).toBeUndefined()
    })

    it('should handle deleting non-existent item gracefully', async () => {
      await expect(
        bridgeIII.trigger({
          function_id: 'stream::delete',
          payload: { stream_name: testStreamName, group_id: testGroupId, item_id: 'non-existent' },
        }),
      ).resolves.not.toThrow()
    })
  })

  describe('stream::list via bridge', () => {
    it('should get all items in a group', async () => {
      type TestDataWithId = TestData & { id: string }

      const groupId = `bridge-stream-${Date.now()}`
      const items: TestDataWithId[] = [
        { id: 'stream-item1', value: 1 },
        { id: 'stream-item2', value: 2 },
        { id: 'stream-item3', value: 3 },
      ]

      for (const item of items) {
        await bridgeIII.trigger({
          function_id: 'stream::set',
          payload: { stream_name: testStreamName, group_id: groupId, item_id: item.id, data: item },
        })
      }

      const result: TestDataWithId[] = await bridgeIII.trigger({
        function_id: 'stream::list',
        payload: { stream_name: testStreamName, group_id: groupId },
      })
      const sort = (a: TestDataWithId, b: TestDataWithId) => a.id.localeCompare(b.id)

      expect(Array.isArray(result)).toBe(true)
      expect(result.length).toBe(items.length)
      expect(result.sort(sort)).toEqual(items.sort(sort))
    })
  })
})

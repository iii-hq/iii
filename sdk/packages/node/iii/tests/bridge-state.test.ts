import { beforeEach, describe, expect, it } from 'vitest'
import type { StateSetResult } from '../src/state'
import { bridgeIII } from './bridge-utils'

type TestData = {
  name?: string
  value: number
  updated?: boolean
}

describe('Bridge State Operations', () => {
  const scope = 'bridge-test-scope'
  const key = 'bridge-test-item'

  beforeEach(async () => {
    await bridgeIII.trigger({ function_id: 'state::delete', payload: { scope, key } }).catch(() => void 0)
  })

  describe('state::set via bridge', () => {
    it('should set a new state item', async () => {
      const testData = { name: 'Test Item', value: 42 }

      const result = await bridgeIII.trigger({ function_id: 'state::set', payload: { scope, key, value: testData } })

      expect(result).toBeDefined()
      expect(result).toEqual({ old_value: null, new_value: testData })
    })

    it('should overwrite an existing state item', async () => {
      const initialData: TestData = { value: 1 }
      const updatedData: TestData = { value: 2, updated: true }

      await bridgeIII.trigger({ function_id: 'state::set', payload: { scope, key, value: initialData } })

      const result: StateSetResult<TestData> = await bridgeIII.trigger({
        function_id: 'state::set',
        payload: { scope, key, value: updatedData },
      })

      expect(result.old_value).toEqual(initialData)
      expect(result.new_value).toEqual(updatedData)
    })
  })

  describe('state::get via bridge', () => {
    it('should get an existing state item', async () => {
      const data: TestData = { name: 'Test', value: 100 }

      await bridgeIII.trigger({ function_id: 'state::set', payload: { scope, key, value: data } })

      const result: TestData = await bridgeIII.trigger({ function_id: 'state::get', payload: { scope, key } })

      expect(result).toBeDefined()
      expect(result).toEqual(data)
    })

    it('should return undefined for non-existent item', async () => {
      const result = await bridgeIII.trigger({
        function_id: 'state::get',
        payload: { scope, key: 'non-existent-item' },
      })

      expect(result).toBeUndefined()
    })
  })

  describe('state::delete via bridge', () => {
    it('should delete an existing state item', async () => {
      await bridgeIII.trigger({ function_id: 'state::set', payload: { scope, key, value: { test: true } } })
      await bridgeIII.trigger({ function_id: 'state::delete', payload: { scope, key } })
      await expect(bridgeIII.trigger({ function_id: 'state::get', payload: { scope, key } })).resolves.toBeUndefined()
    })

    it('should handle deleting non-existent item gracefully', async () => {
      await expect(
        bridgeIII.trigger({ function_id: 'state::delete', payload: { scope, key: 'non-existent' } }),
      ).resolves.not.toThrow()
    })
  })

  describe('state::list via bridge', () => {
    it('should get all items in a scope', async () => {
      type TestDataWithId = TestData & { id: string }

      const bridgeScope = `bridge-state-${Date.now()}`
      const items: TestDataWithId[] = [
        { id: 'state-item1', value: 1 },
        { id: 'state-item2', value: 2 },
        { id: 'state-item3', value: 3 },
      ]

      for (const item of items) {
        await bridgeIII.trigger({
          function_id: 'state::set',
          payload: { scope: bridgeScope, key: item.id, value: item },
        })
      }

      const result: TestDataWithId[] = await bridgeIII.trigger({
        function_id: 'state::list',
        payload: { scope: bridgeScope },
      })
      const sort = (a: TestDataWithId, b: TestDataWithId) => a.id.localeCompare(b.id)

      expect(Array.isArray(result)).toBe(true)
      expect(result.length).toBe(items.length)
      expect(result.sort(sort)).toEqual(items.sort(sort))
    })
  })
})

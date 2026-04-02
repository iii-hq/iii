import { beforeEach, describe, expect, it } from 'vitest'
import { StateEventType, type StateEventData, type StateSetResult } from '../src/state'
import type { FunctionRef, Trigger } from '../src/types'
import { execute, iii, logger } from './utils'

type TestData = {
  name?: string
  value: number
  updated?: boolean
}

describe('State Operations', () => {
  const scope = 'test-scope'
  const key = 'test-item'

  beforeEach(async () => {
    await iii.trigger({ function_id: 'state::delete', payload: { scope, key } }).catch(() => void 0)
  })

  describe('state::set', () => {
    it('should set a new state item', async () => {
      const testData = {
        name: 'Test Item',
        value: 42,
        metadata: { created: new Date().toISOString() },
      }

      const result = await iii.trigger({ function_id: 'state::set', payload: {
        scope,
        key,
        value: testData,
      } })

      expect(result).toBeDefined()
      expect(result).toEqual({ old_value: null, new_value: testData })
    })

    it('should overwrite an existing state item', async () => {
      const initialData: TestData = { value: 1 }
      const updatedData: TestData = { value: 2, updated: true }

      await iii.trigger({ function_id: 'state::set', payload: { scope, key, value: initialData } })

      const result: StateSetResult<TestData> = await iii.trigger({ function_id: 'state::set', payload: {
        scope,
        key,
        value: updatedData,
      } })

      expect(result.old_value).toEqual(initialData)
      expect(result.new_value).toEqual(updatedData)
    })
  })

  describe('state::get', () => {
    it('should get an existing state item', async () => {
      const data: TestData = { name: 'Test', value: 100 }

      await iii.trigger({ function_id: 'state::set', payload: { scope, key, value: data } })

      const result: TestData = await iii.trigger({ function_id: 'state::get', payload: { scope, key } })

      expect(result).toBeDefined()
      expect(result).toEqual(data)
    })

    it('should return null for non-existent item', async () => {
      const result = await iii.trigger({ function_id: 'state::get', payload: { scope, key: 'non-existent-item' } })

      expect(result).toBeUndefined()
    })
  })

  describe('state::delete', () => {
    it('should delete an existing state item', async () => {
      await iii.trigger({ function_id: 'state::set', payload: { scope, key, value: { test: true } } })
      await iii.trigger({ function_id: 'state::delete', payload: { scope, key } })
      await expect(iii.trigger({ function_id: 'state::get', payload: { scope, key } })).resolves.toBeUndefined()
    })

    it('should handle deleting non-existent item gracefully', async () => {
      await expect(iii.trigger({ function_id: 'state::delete', payload: { scope, key: 'non-existent' } })).resolves.not.toThrow()
    })
  })

  describe('state::list', () => {
    it('should get all items in a scope', async () => {
      type TestDataWithId = TestData & { id: string }

      const scope = `state-${Date.now()}`
      const items: TestDataWithId[] = [
        { id: 'state-item1', value: 1 },
        { id: 'state-item2', value: 2 },
        { id: 'state-item3', value: 3 },
      ]

      // Set multiple items
      for (const item of items) {
        await iii.trigger({ function_id: 'state::set', payload: { scope, key: item.id, value: item } })
      }

      const result: TestDataWithId[] = await iii.trigger({ function_id: 'state::list', payload: { scope } })
      const sort = (a: TestDataWithId, b: TestDataWithId) => a.id.localeCompare(b.id)

      expect(Array.isArray(result)).toBe(true)
      expect(result.length).toBeGreaterThanOrEqual(items.length)
      expect(result.sort(sort)).toEqual(items.sort(sort))
    })
  })

  describe('reactive state', () => {
    it('should update the state when the state is updated', async () => {
      const data: TestData = { name: 'Test', value: 100 }
      const updatedData: TestData = { name: 'New Test Data', value: 200 }
      const reactiveResult: { data?: TestData; called: boolean } = { called: false }

      await iii.trigger({ function_id: 'state::set', payload: { scope, key, value: data } })

      let trigger: Trigger | undefined
      let stateUpdatedFunction: FunctionRef | undefined

      try {
        stateUpdatedFunction = iii.registerFunction(
          { id: 'state.updated' },
          async (event: StateEventData<TestData>) => {
            logger.info('State updated', { event })

            if (event.type === 'state' && event.event_type === StateEventType.Updated) {
              reactiveResult.data = event.new_value
              reactiveResult.called = true
            }
          },
        )

        trigger = iii.registerTrigger({
          type: 'state',
          function_id: stateUpdatedFunction.id,
          config: { scope, key },
        })

        await iii.trigger({ function_id: 'state::set', payload: { scope, key, value: updatedData } })
        await execute(async () => {
          expect(reactiveResult.called).toBe(true)
          expect(reactiveResult.data).toEqual(updatedData)
        })
      } finally {
        trigger?.unregister()
        stateUpdatedFunction?.unregister()
      }
    })
  })
})

import type { StreamSetResult, UpdateOp } from 'iii-sdk/stream'
import { SpanStatusCode, withSpan } from 'iii-sdk/telemetry'
import type { InternalStateManager } from '../types'
import { getInstance } from './iii'

export class StateManager implements InternalStateManager {
  async get<T>(scope: string, key: string): Promise<T | null> {
    return withSpan('state::get', {}, async (span) => {
      span.setAttribute('motia.state.scope', scope)
      span.setAttribute('motia.state.key', key)
      try {
        return (await getInstance().trigger({ function_id: 'state::get', payload: { scope, key } })) as T | null
      } catch (err) {
        span.setStatus({ code: SpanStatusCode.ERROR, message: String(err) })
        span.recordException(err as Error)
        throw err
      }
    })
  }

  async set<T>(scope: string, key: string, value: T): Promise<StreamSetResult<T> | null> {
    return withSpan('state::set', {}, async (span) => {
      span.setAttribute('motia.state.scope', scope)
      span.setAttribute('motia.state.key', key)
      try {
        return (await getInstance().trigger({
          function_id: 'state::set',
          payload: { scope, key, value },
        })) as StreamSetResult<T> | null
      } catch (err) {
        span.setStatus({ code: SpanStatusCode.ERROR, message: String(err) })
        span.recordException(err as Error)
        throw err
      }
    })
  }

  async delete<T>(scope: string, key: string): Promise<T | null> {
    return withSpan('state::delete', {}, async (span) => {
      span.setAttribute('motia.state.scope', scope)
      span.setAttribute('motia.state.key', key)
      try {
        return (await getInstance().trigger({ function_id: 'state::delete', payload: { scope, key } })) as T | null
      } catch (err) {
        span.setStatus({ code: SpanStatusCode.ERROR, message: String(err) })
        span.recordException(err as Error)
        throw err
      }
    })
  }

  async list<T>(scope: string): Promise<T[]> {
    return withSpan('state::list', {}, async (span) => {
      span.setAttribute('motia.state.scope', scope)
      try {
        return (await getInstance().trigger({ function_id: 'state::list', payload: { scope } })) as T[]
      } catch (err) {
        span.setStatus({ code: SpanStatusCode.ERROR, message: String(err) })
        span.recordException(err as Error)
        throw err
      }
    })
  }

  async listGroups(): Promise<string[]> {
    return withSpan('state::list_groups', {}, async (span) => {
      try {
        return (await getInstance().trigger({ function_id: 'state::list_groups', payload: {} })) as string[]
      } catch (err) {
        span.setStatus({ code: SpanStatusCode.ERROR, message: String(err) })
        span.recordException(err as Error)
        throw err
      }
    })
  }

  async update<T>(scope: string, key: string, ops: UpdateOp[]): Promise<StreamSetResult<T> | null> {
    return withSpan('state::update', {}, async (span) => {
      span.setAttribute('motia.state.scope', scope)
      span.setAttribute('motia.state.key', key)
      try {
        return (await getInstance().trigger({ function_id: 'state::update', payload: { scope, key, ops } })) as StreamSetResult<T> | null
      } catch (err) {
        span.setStatus({ code: SpanStatusCode.ERROR, message: String(err) })
        span.recordException(err as Error)
        throw err
      }
    })
  }

  async clear(scope: string): Promise<void> {
    return withSpan('state::clear', {}, async (span) => {
      span.setAttribute('motia.state.scope', scope)
      try {
        const items = await this.list<{ id: string }>(scope)

        for (const item of items) {
          await this.delete(scope, item.id)
        }
      } catch (err) {
        span.setStatus({ code: SpanStatusCode.ERROR, message: String(err) })
        span.recordException(err as Error)
        throw err
      }
    })
  }
}

export const stateManager = new StateManager()

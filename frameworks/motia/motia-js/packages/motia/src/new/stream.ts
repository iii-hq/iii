import type { StreamSetResult, UpdateOp } from 'iii-sdk/stream'
import { SpanStatusCode, withSpan } from 'iii-sdk/telemetry'
import type { InferSchema } from '../types'
import type { StateStreamEvent, StateStreamEventChannel, StreamConfig } from '../types-stream'
import { getInstance } from './iii'

type InferStreamData<TConfig extends StreamConfig> = StreamConfig extends TConfig
  ? unknown
  : InferSchema<TConfig['schema']>

export class Stream<TConfig extends StreamConfig = StreamConfig> {
  constructor(readonly config: TConfig) {}

  async get(groupId: string, itemId: string): Promise<InferStreamData<TConfig> | null> {
    return withSpan('stream::get', {}, async (span) => {
      span.setAttribute('motia.stream.name', this.config.name)
      span.setAttribute('motia.stream.group_id', groupId)
      span.setAttribute('motia.stream.item_id', itemId)
      try {
        return (await getInstance().trigger({
          function_id: 'stream::get',
          payload: { stream_name: this.config.name, group_id: groupId, item_id: itemId },
        })) as InferStreamData<TConfig> | null
      } catch (err) {
        span.setStatus({ code: SpanStatusCode.ERROR, message: String(err) })
        span.recordException(err as Error)
        throw err
      }
    })
  }

  async set(
    groupId: string,
    itemId: string,
    data: InferStreamData<TConfig>,
  ): Promise<StreamSetResult<InferStreamData<TConfig>>> {
    return withSpan('stream::set', {}, async (span) => {
      span.setAttribute('motia.stream.name', this.config.name)
      span.setAttribute('motia.stream.group_id', groupId)
      span.setAttribute('motia.stream.item_id', itemId)
      try {
        return (await getInstance().trigger({
          function_id: 'stream::set',
          payload: { stream_name: this.config.name, group_id: groupId, item_id: itemId, data },
        })) as StreamSetResult<InferStreamData<TConfig>>
      } catch (err) {
        span.setStatus({ code: SpanStatusCode.ERROR, message: String(err) })
        span.recordException(err as Error)
        throw err
      }
    })
  }

  async delete(groupId: string, itemId: string): Promise<void> {
    return withSpan('stream::delete', {}, async (span) => {
      span.setAttribute('motia.stream.name', this.config.name)
      span.setAttribute('motia.stream.group_id', groupId)
      span.setAttribute('motia.stream.item_id', itemId)
      try {
        return (await getInstance().trigger({
          function_id: 'stream::delete',
          payload: { stream_name: this.config.name, group_id: groupId, item_id: itemId },
        })) as void
      } catch (err) {
        span.setStatus({ code: SpanStatusCode.ERROR, message: String(err) })
        span.recordException(err as Error)
        throw err
      }
    })
  }

  async list(groupId: string): Promise<InferStreamData<TConfig>[]> {
    return withSpan('stream::list', {}, async (span) => {
      span.setAttribute('motia.stream.name', this.config.name)
      span.setAttribute('motia.stream.group_id', groupId)
      try {
        return (await getInstance().trigger({
          function_id: 'stream::list',
          payload: { stream_name: this.config.name, group_id: groupId },
        })) as InferStreamData<TConfig>[]
      } catch (err) {
        span.setStatus({ code: SpanStatusCode.ERROR, message: String(err) })
        span.recordException(err as Error)
        throw err
      }
    })
  }

  async update(groupId: string, itemId: string, ops: UpdateOp[]): Promise<StreamSetResult<InferStreamData<TConfig>>> {
    return withSpan('stream::update', {}, async (span) => {
      span.setAttribute('motia.stream.name', this.config.name)
      span.setAttribute('motia.stream.group_id', groupId)
      span.setAttribute('motia.stream.item_id', itemId)
      try {
        return (await getInstance().trigger({
          function_id: 'stream::update',
          payload: { stream_name: this.config.name, group_id: groupId, item_id: itemId, ops },
        })) as StreamSetResult<InferStreamData<TConfig>>
      } catch (err) {
        span.setStatus({ code: SpanStatusCode.ERROR, message: String(err) })
        span.recordException(err as Error)
        throw err
      }
    })
  }

  async listGroups(): Promise<string[]> {
    return withSpan('stream::list_groups', {}, async (span) => {
      span.setAttribute('motia.stream.name', this.config.name)
      try {
        return (await getInstance().trigger({
          function_id: 'stream::list_groups',
          payload: { stream_name: this.config.name },
        })) as string[]
      } catch (err) {
        span.setStatus({ code: SpanStatusCode.ERROR, message: String(err) })
        span.recordException(err as Error)
        throw err
      }
    })
  }

  async send<T>(channel: StateStreamEventChannel, event: StateStreamEvent<T>): Promise<void> {
    return withSpan('stream::send', {}, async (span) => {
      span.setAttribute('motia.stream.name', this.config.name)
      span.setAttribute('motia.stream.group_id', channel.groupId)
      if (channel.id) span.setAttribute('motia.stream.item_id', channel.id)
      try {
        await getInstance().trigger({
          function_id: 'stream::send',
          payload: {
            stream_name: this.config.name,
            group_id: channel.groupId,
            id: channel.id,
            type: event.type,
            data: event.data,
          },
        })
      } catch (err) {
        span.setStatus({ code: SpanStatusCode.ERROR, message: String(err) })
        span.recordException(err as Error)
        throw err
      }
    })
  }
}

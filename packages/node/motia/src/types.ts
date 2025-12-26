import type { ZodArray, ZodObject } from 'zod'
import type { Logger } from '@iii-dev/sdk'
import type { JsonSchema } from './types/schema.types'

export type ZodInput = ZodObject<any> | ZodArray<any>

export type StepSchemaInput = ZodInput | JsonSchema

export type InternalStateManager = {
  get<T>(groupId: string, key: string): Promise<T | null>
  set<T>(groupId: string, key: string, value: T): Promise<T>
  delete<T>(groupId: string, key: string): Promise<T | null>
  getGroup<T>(groupId: string): Promise<T[]>
  clear(groupId: string): Promise<void>
}

export type EmitData = { topic: ''; data: unknown; messageGroupId?: string }
export type Emitter<TData> = (event: TData) => Promise<void>

export interface FlowContext<TEmitData = never> {
  emit: Emitter<TEmitData>
  traceId: string
  state: InternalStateManager
  logger: Logger
  streams: Streams
}

export type EventHandler<TInput, TEmitData> = (input: TInput, ctx: FlowContext<TEmitData>) => Promise<void>

export type Emit = string | { topic: string; label?: string; conditional?: boolean }

export type HandlerConfig = {
  ram: number
  cpu?: number
  timeout: number
}

export type QueueConfig = {
  type: 'fifo' | 'standard'
  maxRetries: number
  visibilityTimeout: number
  delaySeconds: number
}

export type InfrastructureConfig = {
  handler?: Partial<HandlerConfig>
  queue?: Partial<QueueConfig>
}

export type EventConfig = {
  type: 'event'
  name: string
  description?: string
  subscribes: readonly string[]
  emits: readonly Emit[]
  virtualEmits?: readonly Emit[]
  virtualSubscribes?: readonly string[]
  input?: StepSchemaInput
  flows?: readonly string[]
  includeFiles?: readonly string[]
  infrastructure?: Partial<InfrastructureConfig>
}

export type NoopConfig = {
  type: 'noop'
  name: string
  description?: string
  virtualEmits: readonly Emit[]
  virtualSubscribes: readonly string[]
  flows?: readonly string[]
}

export type ApiRouteMethod = 'GET' | 'POST' | 'PUT' | 'DELETE' | 'PATCH' | 'OPTIONS' | 'HEAD'

export type ApiMiddleware<TBody = unknown, TEmitData = never, TResult = unknown> = (
  req: ApiRequest<TBody>,
  ctx: FlowContext<TEmitData>,
  next: () => Promise<ApiResponse<number, TResult>>,
) => Promise<ApiResponse<number, TResult>>

export interface QueryParam {
  name: string
  description: string
}

export interface ApiRouteConfig {
  type: 'api'
  name: string
  description?: string
  path: string
  method: ApiRouteMethod
  emits: readonly Emit[]
  virtualEmits?: readonly Emit[]
  virtualSubscribes?: readonly string[]
  flows?: readonly string[]
  middleware?: readonly ApiMiddleware<any, any, any>[]
  bodySchema?: StepSchemaInput
  responseSchema?: Record<number, StepSchemaInput>
  queryParams?: readonly QueryParam[]
  includeFiles?: readonly string[]
}

export interface ApiRequest<TBody = unknown> {
  pathParams: Record<string, string>
  queryParams: Record<string, string | string[]>
  body: TBody
  headers: Record<string, string | string[]>
}

export type ApiResponse<TStatus extends number = number, TBody = string | Buffer | Record<string, unknown>> = {
  status: TStatus
  headers?: Record<string, string>
  body: TBody
}

export type ApiRouteHandler<
  TRequestBody = unknown,
  TResponseBody extends ApiResponse<number, unknown> = ApiResponse<number, unknown>,
  TEmitData = never,
> = (req: ApiRequest<TRequestBody>, ctx: FlowContext<TEmitData>) => Promise<TResponseBody>

export type CronConfig = {
  type: 'cron'
  name: string
  description?: string
  cron: string
  virtualEmits?: readonly Emit[]
  virtualSubscribes?: readonly string[]
  emits: readonly Emit[]
  flows?: readonly string[]
  includeFiles?: readonly string[]
}

export type CronHandler<TEmitData = never> = (ctx: FlowContext<TEmitData>) => Promise<void>

/**
 * @deprecated Use `Handlers` instead.
 */
export type StepHandler<T> = T extends EventConfig
  ? EventHandler<unknown, { topic: string; data: any }>
  : T extends ApiRouteConfig
  ? ApiRouteHandler<any, ApiResponse<number, any>, { topic: string; data: any }>
  : T extends CronConfig
  ? CronHandler<{ topic: string; data: any }>
  : never

export type Event<TData = unknown> = {
  topic: string
  data: TData
  traceId: string
  flows?: string[]
  logger: Logger
  messageGroupId?: string
}

export type Handler<TData = unknown> = (event: Event<TData>) => Promise<void>

export type SubscribeConfig<TData> = {
  event: string
  handlerName: string
  filePath: string
  handler: Handler<TData>
}

export type UnsubscribeConfig = {
  filePath: string
  event: string
}

export type StepConfig = EventConfig | NoopConfig | ApiRouteConfig | CronConfig

export type Step<TConfig extends StepConfig = StepConfig> = { filePath: string; config: TConfig }

export type PluginStep<TConfig extends StepConfig = ApiRouteConfig> = Step<TConfig> & {
  handler?: ApiRouteHandler<any, any, any>
}

export type Flow = {
  name: string
  description?: string
  steps: Step[]
}

// biome-ignore lint/suspicious/noEmptyInterface: we need to define this interface to avoid type errors
export interface Streams {}

// biome-ignore lint/suspicious/noEmptyInterface: we need to define this interface to avoid type errors
export interface Emits {}

type HasOutput = { _output: unknown }

type InferSchema<T, TFallback = unknown> =
  T extends HasOutput ? T['_output'] :
  [T] extends [object] ? T :
  TFallback

type InferBody<T> =
  T extends { bodySchema: infer B }
    ? InferSchema<B, Record<string, unknown>>
    : Record<string, unknown>

type InferInput<T> =
  T extends { input: infer I } ? InferSchema<I> : unknown

type EmitTopic<T extends string> =
  T extends keyof Emits ? Emits[T] : unknown

type NormalizeEmit<E> =
  E extends string ? { topic: E } :
  E extends { topic: infer T extends string } ? { topic: T } :
  never

type EmitElement<E> =
  NormalizeEmit<E> extends { topic: infer T extends string }
    ? { topic: T; data: EmitTopic<T> }
    : never

type InferEmits<T> =
  T extends { emits: readonly unknown[] }
    ? EmitElement<T['emits'][number]>
    : never

type StatusCode<T> = Extract<keyof T, number>

type InferResponse<T> =
  T extends { responseSchema: infer R extends Record<number, unknown> }
    ? { [K in StatusCode<R>]: ApiResponse<K, InferSchema<R[K]>> }[StatusCode<R>]
    : ApiResponse<number, unknown>

type InvalidConfigType = {
  __brand: 'InvalidConfigType'
  message: 'Config type must be "api", "event", or "cron"'
}

export type Handlers<TConfig> =
  TConfig extends { type: 'api' }
    ? ApiRouteHandler<InferBody<TConfig>, InferResponse<TConfig>, InferEmits<TConfig>>
    : TConfig extends { type: 'event' }
      ? EventHandler<InferInput<TConfig>, InferEmits<TConfig>>
      : TConfig extends { type: 'cron' }
        ? CronHandler<InferEmits<TConfig>>
        : InvalidConfigType
import type { ChannelReader, Logger } from 'iii-sdk'
import type { StreamSetResult, UpdateOp } from 'iii-sdk/stream'
import type { FromSchema } from 'json-schema-to-ts'
import type { ZodType } from 'zod'
import * as z from 'zod'
import type { JsonSchema } from './types/schema.types'

// biome-ignore lint/suspicious/noExplicitAny: we need to define this type to avoid type errors
export type ZodInput = ZodType<any, any, any>

export type TypedJsonSchema<T = unknown> = JsonSchema & { readonly __phantomType?: T }

export type StepSchemaInput = ZodInput | JsonSchema | TypedJsonSchema<unknown>

export function jsonSchema<T extends z.ZodType>(schema: T): TypedJsonSchema<z.output<T>> {
  return z.toJSONSchema(schema, { target: 'draft-7' }) as TypedJsonSchema<z.output<T>>
}

export type InternalStateManager = {
  get<T>(groupId: string, key: string): Promise<T | null>
  set<T>(groupId: string, key: string, value: T): Promise<StreamSetResult<T> | null>
  update<T>(groupId: string, key: string, ops: UpdateOp[]): Promise<StreamSetResult<T> | null>
  delete<T>(groupId: string, key: string): Promise<T | null>
  list<T>(groupId: string): Promise<T[]>
  clear(groupId: string): Promise<void>
}

export type EnqueueData<T = unknown> = { topic: string; data: T; messageGroupId?: string }
export type Enqueuer<TData> = (event: TData) => Promise<void>

export type ExtractQueueInput<TInput> = Exclude<Exclude<Exclude<TInput, ApiRequest>, MotiaHttpArgs>, undefined>
export type ExtractApiInput<TInput> = Extract<TInput, ApiRequest | MotiaHttpArgs>
export type ExtractStateInput<TInput> = Extract<TInput, StateTriggerInput<unknown>>
export type ExtractStreamInput<TInput> = Extract<TInput, StreamTriggerInput<unknown>>
export type ExtractDataPayload<TInput> =
  TInput extends ApiRequest<infer TBody>
    ? TBody
    : TInput extends MotiaHttpArgs<infer TBody>
      ? TBody
      : TInput extends undefined
        ? undefined
        : TInput

export type MatchHandlers<TInput, _TEnqueueData, TResult> = {
  queue?: (input: ExtractQueueInput<TInput>) => Promise<void>

  http?: (request: ExtractApiInput<TInput>) => Promise<TResult>

  cron?: () => Promise<void>

  state?: (input: ExtractStateInput<TInput>) => Promise<TResult>

  stream?: (input: ExtractStreamInput<TInput>) => Promise<TResult>

  default?: (input: TInput) => Promise<TResult | undefined>
}

export interface FlowContext<TEnqueueData = never, TInput = unknown> {
  traceId: string
  trigger: TriggerInfo

  is: {
    queue: (input: TInput) => input is ExtractQueueInput<TInput>
    http: (input: TInput) => input is ExtractApiInput<TInput>
    cron: (input: TInput) => input is never
    state: (input: TInput) => input is ExtractStateInput<TInput>
    stream: (input: TInput) => input is ExtractStreamInput<TInput>
  }

  /**
   * Extracts the data payload from the input, regardless of trigger type.
   * Useful when multiple triggers (e.g., queue and API) share the same data schema.
   *
   * - For API triggers: returns `request.body`
   * - For queue triggers: returns the queue data directly
   * - For cron triggers: returns `undefined`
   *
   * @example
   * ```ts
   * // When queue and API triggers have the same schema
   * const orderData = ctx.getData() // Works for both triggers
   * ```
   */
  getData: () => ExtractDataPayload<TInput>

  // biome-ignore lint/suspicious/noExplicitAny: we need to define this type to avoid type errors
  match: <TResult = any>(handlers: MatchHandlers<TInput, TEnqueueData, TResult>) => Promise<TResult | undefined>
}

export type Enqueue = string | { topic: string; label?: string; conditional?: boolean }

type TriggerType = 'http' | 'queue' | 'cron' | 'state' | 'stream'

export type TriggerInfo = {
  type: TriggerType
  index?: number
  path?: string
  method?: string
  topic?: string
  expression?: string
}

type QueueTriggerInput<T> = T

type ApiTriggerInput<T> = ApiRequest<T>

type CronTriggerInput = undefined

export type StateTriggerInput<T> = {
  type: 'state'
  group_id: string
  item_id: string
  old_value?: T
  new_value?: T
}

export type StreamEvent<TData> =
  | { type: 'create'; data: TData }
  | { type: 'update'; data: TData }
  | { type: 'delete'; data: TData }

export type StreamTriggerInput<T> = {
  type: 'stream'
  timestamp: number
  streamName: string
  groupId: string
  id: string
  event: StreamEvent<T>
}

export type TriggerInput<T> =
  | QueueTriggerInput<T>
  | ApiTriggerInput<T>
  | CronTriggerInput
  | StateTriggerInput<T>
  | StreamTriggerInput<T>

export type TriggerCondition<TInput = unknown> = (
  input: TriggerInput<TInput>,
  ctx: FlowContext<never, TriggerInput<TInput>>,
) => boolean | Promise<boolean>

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
  concurrency?: number
  backoffType?: string
  backoffDelayMs?: number
}

export type ApiRouteMethod = 'GET' | 'POST' | 'PUT' | 'DELETE' | 'PATCH' | 'OPTIONS' | 'HEAD'

export type ApiMiddleware<TBody = unknown, TEnqueueData = never> = (
  req: MotiaHttpArgs<TBody>,
  ctx: FlowContext<TEnqueueData, MotiaHttpArgs<TBody>>,
  next: () => Promise<ApiResponse | void>,
) => Promise<ApiResponse | void>

export interface QueryParam {
  name: string
  description: string
}

// biome-ignore lint/suspicious/noExplicitAny: we need any to allow trigger assignment to TriggerConfig
export type StateTrigger<TSchema extends StepSchemaInput | undefined = any> = {
  type: 'state'
  condition?: TriggerCondition<InferSchema<TSchema>>
}

// biome-ignore lint/suspicious/noExplicitAny: we need any to allow trigger assignment to TriggerConfig
export type StreamTrigger<TSchema extends StepSchemaInput | undefined = any> = {
  type: 'stream'
  streamName: string
  groupId?: string
  itemId?: string
  condition?: TriggerCondition<InferSchema<TSchema>>
}

// biome-ignore lint/suspicious/noExplicitAny: we need any to allow trigger assignment to TriggerConfig
export type QueueTrigger<TSchema extends StepSchemaInput | undefined = any> = {
  type: 'queue'
  topic: string
  input?: TSchema
  condition?: TriggerCondition<TSchema extends ZodInput ? z.infer<TSchema> : unknown>
  config?: Partial<QueueConfig>
}

// biome-ignore lint/suspicious/noExplicitAny: we need any to allow trigger assignment to TriggerConfig
export type ApiTrigger<TSchema extends StepSchemaInput | undefined = any> = {
  type: 'http'
  path: string
  method: ApiRouteMethod
  bodySchema?: TSchema
  responseSchema?: Record<number, StepSchemaInput>
  queryParams?: readonly QueryParam[]
  // biome-ignore lint/suspicious/noExplicitAny: we need to define this type to avoid type errors
  middleware?: readonly ApiMiddleware<any, any>[]
  condition?: TriggerCondition<TSchema extends ZodInput ? z.infer<TSchema> : unknown>
}

export type CronTrigger = {
  type: 'cron'
  expression: string
  input?: never
  condition?: TriggerCondition
}

export type TriggerConfig = QueueTrigger | ApiTrigger | CronTrigger | StateTrigger | StreamTrigger

export type StepConfig = {
  name: string
  description?: string
  triggers: readonly TriggerConfig[]
  enqueues?: readonly Enqueue[]
  virtualEnqueues?: readonly Enqueue[]
  virtualSubscribes?: readonly string[]
  flows?: readonly string[]
  includeFiles?: readonly string[]
}

export type MotiaHttpResponse = {
  status: (statusCode: number) => void
  headers: (headers: Record<string, string>) => void
  stream: NodeJS.WritableStream
  close: () => void
}

export interface ApiRequest<TBody = unknown> {
  pathParams: Record<string, string>
  queryParams: Record<string, string | string[]>
  body: TBody
  headers: Record<string, string | string[]>
}

export interface MotiaHttpRequest<TBody = unknown> {
  pathParams: Record<string, string>
  queryParams: Record<string, string | string[]>
  body: TBody
  headers: Record<string, string | string[]>
  method: string
  requestBody: ChannelReader
}

export interface MotiaHttpArgs<TBody = unknown> {
  request: MotiaHttpRequest<TBody>
  response: MotiaHttpResponse
}

// biome-ignore lint/suspicious/noExplicitAny: we need to define this type to avoid type errors
export type ApiResponse<TStatus extends number = number, TBody = any> = {
  status: TStatus
  headers?: Record<string, string>
  body: TBody
}

// biome-ignore lint/suspicious/noExplicitAny: we need to define this type to avoid type errors
export type StepHandler<TInput = any, TEnqueueData = never> = (
  input: TriggerInput<TInput>,
  ctx: FlowContext<TEnqueueData, TriggerInput<TInput>>,
) => Promise<ApiResponse | void>

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
  queue: string
  handlerName: string
  filePath: string
  handler: Handler<TData>
}

export type UnsubscribeConfig = {
  filePath: string
  queue: string
}

export type Step = { filePath: string; config: StepConfig }

export type Flow = {
  name: string
  description?: string
  steps: Step[]
}

// biome-ignore lint/suspicious/noEmptyInterface: we need to define this interface to avoid type errors
export interface Streams {}

// biome-ignore lint/suspicious/noEmptyInterface: we need to define this interface to avoid type errors
export interface Enqueues {}

export type InferSchema<T, TFallback = unknown> =
  T extends TypedJsonSchema<infer O>
    ? O
    : T extends ZodInput
      ? z.infer<T>
      : T extends { readonly type: string }
        ? FromSchema<T & { type: any }>
        : T extends { readonly anyOf: readonly any[] }
          ? FromSchema<T & { anyOf: any }>
          : T extends { readonly allOf: readonly any[] }
            ? FromSchema<T & { allOf: any }>
            : T extends { readonly oneOf: readonly any[] }
              ? FromSchema<T & { oneOf: any }>
              : T extends undefined
                ? unknown
                : TFallback

type InferBodySchema<S> = S extends ZodInput ? z.infer<S> : S extends StepSchemaInput ? InferSchema<S> : unknown

type TriggerToInput<TTrigger> = TTrigger extends { type: 'queue'; input?: infer S }
  ? S extends ZodInput
    ? z.infer<S>
    : S extends StepSchemaInput
      ? InferSchema<S>
      : unknown
  : TTrigger extends { type: 'http'; bodySchema?: infer S }
    ? MotiaHttpArgs<InferBodySchema<S>>
    : TTrigger extends { type: 'state' }
      ? StateTriggerInput<unknown>
      : TTrigger extends { type: 'stream' }
        ? StreamTriggerInput<unknown>
        : TTrigger extends { type: 'cron' }
          ? undefined
          : never

type InferHandlerInput<TConfig extends StepConfig> = TriggerToInput<TConfig['triggers'][number]>

// biome-ignore lint/suspicious/noConfusingVoidType: we need to define this type to avoid type errors
type InferReturnType = Promise<ApiResponse | void>

type EnqueueTopic<T extends string> = T extends keyof Enqueues ? Enqueues[T] : unknown

type NormalizeEnqueue<E> = E extends string
  ? { topic: E }
  : E extends { topic: infer T extends string }
    ? { topic: T }
    : never

type EnqueueElement<E> =
  NormalizeEnqueue<E> extends { topic: infer T extends string } ? { topic: T; data: EnqueueTopic<T> } : never

type InferEnqueues<T> = T extends { enqueues: readonly unknown[] } ? EnqueueElement<T['enqueues'][number]> : never

export type Handlers<TConfig extends StepConfig> = (
  input: InferHandlerInput<TConfig>,
  ctx: FlowContext<InferEnqueues<TConfig>, InferHandlerInput<TConfig>>,
) => InferReturnType

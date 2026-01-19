import { type Data, WebSocket } from 'ws'
import {
  type BridgeMessage,
  type FunctionInfo,
  type InvocationResultMessage,
  type InvokeFunctionMessage,
  MessageType,
  type RegisterFunctionMessage,
  type RegisterServiceMessage,
  type RegisterTriggerMessage,
  type RegisterTriggerTypeMessage,
  type WorkerInfo,
} from './bridge-types'
import { withContext } from './context'
import { Logger, type LoggerParams } from './logger'
import type { IStream } from './streams'
import type { TriggerHandler } from './triggers'
import type {
  BridgeClient,
  Invocation,
  RemoteFunctionData,
  RemoteFunctionHandler,
  RemoteTriggerTypeData,
  Trigger,
} from './types'

export type FunctionsAvailableCallback = (functions: FunctionInfo[]) => void
export type WorkersAvailableCallback = (workers: WorkerInfo[]) => void

import * as os from 'os'

const SDK_VERSION = '0.1.0'

function getOsInfo(): string {
  const platform = os.platform()
  const release = os.release()
  const arch = os.arch()
  return `${platform} ${release} (${arch})`
}

function getWorkerName(): string {
  const hostname = os.hostname()
  const pid = process.pid
  return `${hostname}:${pid}`
}

export class Bridge implements BridgeClient {
  private ws?: WebSocket
  private functions = new Map<string, RemoteFunctionData>()
  private services = new Map<string, Omit<RegisterServiceMessage, 'functions'>>()
  private invocations = new Map<string, Invocation>()
  private triggers = new Map<string, RegisterTriggerMessage>()
  private triggerTypes = new Map<string, RemoteTriggerTypeData>()
  private messagesToSend: BridgeMessage[] = []
  private functionsAvailableCallbacks: FunctionsAvailableCallback[] = []
  private workersAvailableCallbacks: WorkersAvailableCallback[] = []

  private interval?: NodeJS.Timeout

  constructor(private readonly address: string) {
    this.connect()
  }

  registerTriggerType<TConfig>(
    triggerType: Omit<RegisterTriggerTypeMessage, 'type'>,
    handler: TriggerHandler<TConfig>,
  ) {
    this.sendMessage(MessageType.RegisterTriggerType, triggerType, true)
    this.triggerTypes.set(triggerType.id, {
      message: { ...triggerType, type: MessageType.RegisterTriggerType },
      handler,
    })
  }

  on(event: string, callback: (arg?: unknown) => void) {
    this.ws?.on(event, callback)
  }

  unregisterTriggerType(triggerType: Omit<RegisterTriggerTypeMessage, 'type'>) {
    this.sendMessage(MessageType.UnregisterTriggerType, triggerType, true)
    this.triggerTypes.delete(triggerType.id)
  }

  registerTrigger(trigger: Omit<RegisterTriggerMessage, 'type' | 'id'>): Trigger {
    const id = crypto.randomUUID()
    this.sendMessage(MessageType.RegisterTrigger, { ...trigger, id }, true)
    this.triggers.set(id, { ...trigger, id, type: MessageType.RegisterTrigger })

    return {
      unregister: () => {
        this.sendMessage(MessageType.UnregisterTrigger, { id, type: MessageType.UnregisterTrigger })
        this.triggers.delete(id)
      },
    }
  }

  registerFunction(message: Omit<RegisterFunctionMessage, 'type'>, handler: RemoteFunctionHandler) {
    this.sendMessage(MessageType.RegisterFunction, message, true)
    this.functions.set(message.function_path, {
      message: { ...message, type: MessageType.RegisterFunction },
      handler: async (input) => {
        const invoke = (function_path: string, params: LoggerParams) => this.invokeFunctionAsync(function_path, params)
        const logger = new Logger(invoke, crypto.randomUUID(), message.function_path)
        const context = { logger }

        return withContext(async () => await handler(input), context)
      },
    })
  }

  registerService(message: Omit<RegisterServiceMessage, 'type'>) {
    this.sendMessage(MessageType.RegisterService, message, true)
    this.services.set(message.id, { ...message, type: MessageType.RegisterService })
  }

  async invokeFunction<TInput, TOutput>(function_path: string, data: TInput): Promise<TOutput> {
    const invocation_id = crypto.randomUUID()

    return new Promise<TOutput>((resolve, reject) => {
      this.sendMessage(MessageType.InvokeFunction, { invocation_id, function_path, data })
      this.invocations.set(invocation_id, { resolve, reject })
    })
  }

  invokeFunctionAsync<TInput>(function_path: string, data: TInput): void {
    this.sendMessage(MessageType.InvokeFunction, { function_path, data })
  }

  onFunctionsAvailable(callback: FunctionsAvailableCallback): () => void {
    this.functionsAvailableCallbacks.push(callback)
    return () => {
      const index = this.functionsAvailableCallbacks.indexOf(callback)
      if (index !== -1) {
        this.functionsAvailableCallbacks.splice(index, 1)
      }
    }
  }

  async listFunctions(): Promise<FunctionInfo[]> {
    const result = await this.invokeFunction<{}, { functions: FunctionInfo[] }>('engine.functions.list', {})
    return result.functions
  }

  async listWorkers(): Promise<WorkerInfo[]> {
    const result = await this.invokeFunction<{}, { workers: WorkerInfo[] }>('engine.workers.list', {})
    return result.workers
  }

  onWorkersAvailable(callback: WorkersAvailableCallback): () => void {
    this.workersAvailableCallbacks.push(callback)
    return () => {
      const index = this.workersAvailableCallbacks.indexOf(callback)
      if (index !== -1) {
        this.workersAvailableCallbacks.splice(index, 1)
      }
    }
  }

  private registerWorkerMetadata(): void {
    this.invokeFunctionAsync('engine.workers.register', {
      runtime: 'node',
      version: SDK_VERSION,
      name: getWorkerName(),
      os: getOsInfo(),
    })
  }

  createStream<TData>(streamName: string, stream: IStream<TData>): void {
    this.registerFunction({ function_path: `streams.get(${streamName})` }, stream.get.bind(stream))
    this.registerFunction({ function_path: `streams.set(${streamName})` }, stream.set.bind(stream))
    this.registerFunction({ function_path: `streams.delete(${streamName})` }, stream.delete.bind(stream))
    this.registerFunction({ function_path: `streams.getGroup(${streamName})` }, stream.getGroup.bind(stream))
    this.registerFunction({ function_path: `streams.listGroups(${streamName})` }, stream.listGroups.bind(stream))
  }

  // private methods

  private connect() {
    this.ws = new WebSocket(this.address)
    this.ws.on('open', this.onSocketOpen.bind(this))
    this.ws.on('close', this.onSocketClose.bind(this))
  }

  private clearInterval() {
    clearInterval(this.interval)
    this.interval = undefined
  }

  private onSocketClose() {
    this.ws?.removeAllListeners()
    this.ws?.terminate()
    this.ws = undefined

    this.clearInterval()
    this.interval = setInterval(() => this.connect(), 2000)
  }

  private onSocketOpen() {
    this.clearInterval()
    this.ws?.on('message', this.onMessage.bind(this))

    this.triggerTypes.forEach(({ message }) => this.sendMessage(MessageType.RegisterTriggerType, message, true))
    this.services.forEach((service) => this.sendMessage(MessageType.RegisterService, service, true))
    this.functions.forEach(({ message }) => this.sendMessage(MessageType.RegisterFunction, message, true))
    this.triggers.forEach((trigger) => this.sendMessage(MessageType.RegisterTrigger, trigger, true))
    this.messagesToSend
      .splice(0, this.messagesToSend.length)
      .forEach((message) => this.ws?.send(JSON.stringify(message)))

    this.registerWorkerMetadata()
  }

  private isOpen() {
    return this.ws?.readyState === WebSocket.OPEN
  }

  private sendMessage(type: MessageType, message: Omit<BridgeMessage, 'type'>, skipIfClosed = false) {
    if (this.isOpen()) {
      this.ws?.send(JSON.stringify({ ...message, type }))
    } else if (!skipIfClosed) {
      this.messagesToSend.push({ ...message, type } as BridgeMessage)
    }
  }

  private onInvocationResult(invocation_id: string, result: unknown, error: unknown) {
    const invocation = this.invocations.get(invocation_id)

    if (invocation) {
      error ? invocation.reject(error) : invocation.resolve(result)
    }

    this.invocations.delete(invocation_id)
  }

  private async onInvokeFunction<TInput>(invocation_id: string | undefined, function_path: string, input: TInput) {
    const fn = this.functions.get(function_path)

    if (fn) {
      if (!invocation_id) {
        try {
          return fn.handler(input)
        } catch (error) {
          console.error({
            message: 'Error invoking function',
            error: error,
            function_path,
            input,
          })
        }
      }

      try {
        const result = await fn.handler(input)
        this.sendMessage(MessageType.InvocationResult, { invocation_id, function_path, result })
      } catch (error) {
        this.sendMessage(MessageType.InvocationResult, {
          invocation_id,
          function_path,
          error: { code: 'invocation_failed', message: (error as Error).message },
        })
      }
    } else {
      this.sendMessage(MessageType.InvocationResult, {
        invocation_id,
        function_path,
        error: { code: 'function_not_found', message: 'Function not found' },
      })
    }
  }

  private async onRegisterTrigger(message: RegisterTriggerMessage) {
    const triggerTypeData = this.triggerTypes.get(message.trigger_type)
    const { id, trigger_type, function_path, config } = message

    if (triggerTypeData) {
      try {
        await triggerTypeData.handler.registerTrigger({ id, function_path, config })
        this.sendMessage(MessageType.TriggerRegistrationResult, { id, trigger_type, function_path })
      } catch (error) {
        this.sendMessage(MessageType.TriggerRegistrationResult, {
          id,
          trigger_type,
          function_path,
          error: { code: 'trigger_registration_failed', message: (error as Error).message },
        })
      }
    } else {
      this.sendMessage(MessageType.TriggerRegistrationResult, {
        id,
        trigger_type,
        function_path,
        error: { code: 'trigger_type_not_found', message: 'Trigger type not found' },
      })
    }
  }

  private onMessage(socketMessage: Data) {
    const { type, ...message }: BridgeMessage = JSON.parse(socketMessage.toString())

    if (type === MessageType.InvocationResult) {
      const { invocation_id, result, error } = message as InvocationResultMessage
      this.onInvocationResult(invocation_id, result, error)
    } else if (type === MessageType.InvokeFunction) {
      const { invocation_id, function_path, data } = message as InvokeFunctionMessage
      this.onInvokeFunction(invocation_id, function_path, data)
    } else if (type === MessageType.RegisterTrigger) {
      this.onRegisterTrigger(message as RegisterTriggerMessage)
    }
  }
}

import { WebSocket } from 'ws'
import {
  BridgeMessage,
  InvocationResultMessage,
  InvokeFunctionMessage,
  MessageType,
  RegisterFunctionMessage,
  RegisterServiceMessage,
  RegisterTriggerMessage,
  RegisterTriggerTypeMessage,
} from './bridge-types'
import {
  BridgeClient,
  Invocation,
  RemoteFunctionData,
  RemoteFunctionHandler,
  RemoteTriggerTypeData,
  Trigger,
} from './types'
import { TriggerHandler } from './triggers'
import { withContext } from './context'
import { Logger, LoggerParams } from './logger'

export class Bridge implements BridgeClient {
  private ws: WebSocket
  private functions = new Map<string, RemoteFunctionData>()
  private services = new Map<string, Omit<RegisterServiceMessage, 'functions'>>()
  private invocations = new Map<string, Invocation>()
  private triggers = new Map<string, RegisterTriggerMessage>()
  private triggerTypes = new Map<string, RemoteTriggerTypeData>()
  private messagesToSend: BridgeMessage[] = []

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

  unregisterTriggerType(triggerType: Omit<RegisterTriggerTypeMessage, 'type'>) {
    this.sendMessage(MessageType.UnregisterTriggerType, triggerType, true)
    this.triggerTypes.delete(triggerType.id)
  }

  registerTrigger(trigger: Omit<RegisterTriggerMessage, 'type' | 'id'>): Trigger {
    const id = crypto.randomUUID()
    this.sendMessage(MessageType.RegisterTrigger, trigger, true)
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
    this.functions.set(message.functionPath, {
      message: { ...message, type: MessageType.RegisterFunction },
      handler: async (input) => {
        const invoke = (functionPath: string, params: LoggerParams) => this.invokeFunctionAsync(functionPath, params)
        const logger = new Logger(invoke, crypto.randomUUID(), message.functionPath)
        const context = { logger }

        return withContext(async () => await handler(input), context)
      },
    })
  }

  registerService(message: Omit<RegisterServiceMessage, 'type'>) {
    this.sendMessage(MessageType.RegisterService, message, true)
    this.services.set(message.id, { ...message, type: MessageType.RegisterService })
  }

  async invokeFunction<TInput, TOutput>(functionPath: string, data: TInput): Promise<TOutput> {
    const invocationId = crypto.randomUUID()

    return new Promise<TOutput>((resolve, reject) => {
      this.sendMessage(MessageType.InvokeFunction, { invocationId, functionPath, data })
      this.invocations.set(invocationId, { resolve, reject })
    })
  }

  invokeFunctionAsync<TInput>(functionPath: string, data: TInput): void {
    this.sendMessage(MessageType.InvokeFunction, { functionPath, data })
  }

  // private methods

  private connect() {
    this.ws = new WebSocket(this.address)
    this.ws.on('open', this.onSocketOpen.bind(this))
  }

  private onSocketOpen() {
    this.ws.on('message', this.onMessage.bind(this))

    this.triggerTypes.forEach(({ message }) => this.sendMessage(MessageType.RegisterTriggerType, message, true))
    this.services.forEach((service) => this.sendMessage(MessageType.RegisterService, service, true))
    this.functions.forEach(({ message }) => this.sendMessage(MessageType.RegisterFunction, message, true))
    this.triggers.forEach((trigger) => this.sendMessage(MessageType.RegisterTrigger, trigger, true))
    this.messagesToSend
      .splice(0, this.messagesToSend.length)
      .forEach((message) => this.ws.send(JSON.stringify(message)))
  }

  private isOpen() {
    return this.ws.readyState === WebSocket.OPEN
  }

  private sendMessage(type: MessageType, message: Omit<BridgeMessage, 'type'>, skipIfClosed = false) {
    if (this.isOpen()) {
      this.ws.send(JSON.stringify({ ...message, type }))
    } else if (!skipIfClosed) {
      this.messagesToSend.push({ ...message, type } as BridgeMessage)
    }
  }

  private onInvocationResult(invocationId: string, result: any, error: any) {
    const invocation = this.invocations.get(invocationId)

    if (invocation) {
      error ? invocation.reject(error) : invocation.resolve(result)
    }

    this.invocations.delete(invocationId)
  }

  private async onInvokeFunction<TInput>(invocationId: string | undefined, functionPath: string, input: TInput) {
    const fn = this.functions.get(functionPath)

    if (fn) {
      if (!invocationId) {
        return fn.handler(input) // no need to wait on anything
      }

      try {
        const result = await fn.handler(input)
        this.sendMessage(MessageType.InvocationResult, { invocationId, functionPath, result })
      } catch (error) {
        this.sendMessage(MessageType.InvocationResult, { invocationId, functionPath, error })
      }
    } else {
      this.sendMessage(MessageType.InvocationResult, { invocationId, functionPath, error: 'Function not found' })
    }
  }

  private async onRegisterTrigger(message: RegisterTriggerMessage) {
    const triggerTypeData = this.triggerTypes.get(message.triggerType)
    const { id, triggerType, functionPath, config } = message

    if (triggerTypeData) {
      try {
        await triggerTypeData.handler.registerTrigger({ id, functionPath, config })
        this.sendMessage(MessageType.TriggerRegistrationResult, { id, triggerType, functionPath })
      } catch (error) {
        this.sendMessage(MessageType.TriggerRegistrationResult, {
          id,
          triggerType,
          functionPath,
          error: { code: 'trigger_registration_failed', message: error.message },
        })
      }
    }
  }

  private onMessage(socketMessage: WebSocket.Data) {
    const { type, ...message }: BridgeMessage = JSON.parse(socketMessage.toString())

    if (type === MessageType.InvocationResult) {
      const { invocationId, result, error } = message as InvocationResultMessage
      this.onInvocationResult(invocationId, result, error)
    } else if (type === MessageType.InvokeFunction) {
      const { invocationId, functionPath, data } = message as InvokeFunctionMessage
      this.onInvokeFunction(invocationId, functionPath, data)
    } else if (type === MessageType.RegisterTrigger) {
      this.onRegisterTrigger(message as RegisterTriggerMessage)
    }
  }
}

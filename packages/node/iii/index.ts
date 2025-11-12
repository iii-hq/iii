import { WebSocket } from 'ws'
import {
  BridgeMessage,
  InvocationResultMessage,
  InvokeFunctionMessage,
  MessageType,
  RegisterFunctionMessage,
  RegisterServiceMessage,
  RegisterTriggerMessage,
} from './bridge-types'
import { BridgeClient, Invocation, RegisterServiceInput, RemoteFunctionData, RemoteFunctionHandler } from './types'

export class Bridge implements BridgeClient {
  private ws: WebSocket
  private functions = new Map<string, RemoteFunctionData>()
  private services = new Map<string, Omit<RegisterServiceMessage, 'functions'>>()
  private invocations = new Map<string, Invocation>()
  private triggers = new Map<string, RegisterTriggerMessage>()
  private messagesToSend: BridgeMessage[] = []

  constructor(private readonly address: string) {
    this.connect()
  }

  registerTrigger(trigger: Omit<RegisterTriggerMessage, 'type'>) {
    this.sendMessage(MessageType.RegisterTrigger, trigger, true)
    this.triggers.set(trigger.id, { ...trigger, type: MessageType.RegisterTrigger })
  }

  registerFunction(message: Omit<RegisterFunctionMessage, 'type'>, handler: RemoteFunctionHandler) {
    this.sendMessage(MessageType.RegisterFunction, message, true)
    this.functions.set(message.functionPath, { message: { ...message, type: MessageType.RegisterFunction }, handler })
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
    console.log('sendMessage', type, message)
    if (this.isOpen()) {
      this.ws.send(JSON.stringify({ ...message, type }))
    } else if (!skipIfClosed) {
      this.messagesToSend.push({ ...message, type } as BridgeMessage)
    }
  }

  private onInvocationResult(invocationId: string, data: any, error: any) {
    const invocation = this.invocations.get(invocationId)

    if (invocation) {
      error ? invocation.reject(error) : invocation.resolve(data)
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
        const data = await fn.handler(input)
        this.sendMessage(MessageType.InvocationResult, { invocationId, functionPath, data })
      } catch (error) {
        this.sendMessage(MessageType.InvocationResult, { invocationId, functionPath, error })
      }
    } else {
      this.sendMessage(MessageType.InvocationResult, { invocationId, functionPath, error: 'Function not found' })
    }
  }

  private onMessage(socketMessage: WebSocket.Data) {
    const { type, ...message }: BridgeMessage = JSON.parse(socketMessage.toString())

    if (type === MessageType.InvocationResult) {
      const { invocationId, data, error } = message as InvocationResultMessage
      this.onInvocationResult(invocationId, data, error)
    } else if (type === MessageType.InvokeFunction) {
      const { invocationId, functionPath, data } = message as InvokeFunctionMessage
      this.onInvokeFunction(invocationId, functionPath, data)
    }
  }
}

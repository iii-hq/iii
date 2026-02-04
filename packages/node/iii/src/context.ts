import { AsyncLocalStorage } from 'node:async_hooks'
import type { Span } from '@opentelemetry/api'
import { context as otelContext } from '@opentelemetry/api'
import { Logger } from './logger'

export type Context = {
  logger: Logger
  /** The active OpenTelemetry span for adding custom attributes, events, etc. */
  trace?: Span
}

const globalStorage = new AsyncLocalStorage<Context>()

export const withContext = async <T>(fn: (context: Context) => Promise<T>, ctx: Context): Promise<T> => {
  // Capture the current OTel context before entering AsyncLocalStorage.run()
  // This preserves the OpenTelemetry trace context across async boundaries
  const currentOtelContext = otelContext.active()
  
  return globalStorage.run(ctx, async () => {
    // Restore the OTel context inside the run() scope
    // This ensures trace propagation works when handlers call bridge.invokeFunction
    return otelContext.with(currentOtelContext, async () => await fn(ctx))
  })
}

export const getContext = (): Context => {
  const store = globalStorage.getStore()
  if (store) {
    return store
  }

  const logger = new Logger()

  return { logger }
}

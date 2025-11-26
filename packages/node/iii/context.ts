import { AsyncLocalStorage } from 'async_hooks'
import { Logger } from './logger'

export type Context = {
  logger: Logger
}

const globalStorage = new AsyncLocalStorage<Context>()

export const withContext = async <T>(fn: (context: Context) => Promise<T>, context: Context): Promise<T> => {
  return globalStorage.run(context, async () => await fn(context))
}

export const getContext = (): Context => {
  const store = globalStorage.getStore()
  if (store) {
    return store
  }

  const logger = new Logger()

  return { logger }
}

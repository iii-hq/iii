export type LoggerParams = {
  message: string
  trace_id?: string
  function_name?: string
  data?: any
}
export type LoggerInvoker = (function_path: string, params: LoggerParams) => Promise<void> | void

export class Logger {
  constructor(
    private readonly invoker?: LoggerInvoker,
    private readonly traceId?: string,
    private readonly functionName?: string,
  ) {}

  info(message: string, data?: any) {
    this.invoker?.('logger.info', {
      message,
      data,
      trace_id: this.traceId,
      function_name: this.functionName,
    })
  }

  warn(message: string, data?: any) {
    this.invoker?.('logger.warn', {
      message,
      data,
      trace_id: this.traceId ?? '',
      function_name: this.functionName ?? '',
    })
  }

  error(message: string, data?: any) {
    this.invoker?.('logger.error', {
      message,
      data,
      trace_id: this.traceId,
      function_name: this.functionName,
    })
  }
}

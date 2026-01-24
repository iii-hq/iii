import { SeverityNumber } from '@opentelemetry/api-logs'
import { getLogger as getOtelLogger } from './telemetry'
import { safeStringify } from './utils'

export type LoggerParams = {
  message: string
  trace_id?: string
  span_id?: string
  service_name?: string
  data?: any
  /** @deprecated Use service_name instead */
  function_name?: string
}

/**
 * @deprecated Use OpenTelemetry Logger directly via getLogger() from telemetry module.
 * This invoker pattern is maintained for backward compatibility only.
 */
export type LoggerInvoker = (function_path: string, params: LoggerParams) => Promise<void> | void

export class Logger {
  private _otelLogger: ReturnType<typeof getOtelLogger> | null = null

  private get otelLogger() {
    // Lazy initialization: re-fetch logger if not yet available
    if (!this._otelLogger) {
      this._otelLogger = getOtelLogger()
    }
    return this._otelLogger
  }

  constructor(
    /**
     * @deprecated This parameter is ignored. Logger now uses OpenTelemetry internally.
     */
    private readonly invoker?: LoggerInvoker,
    private readonly traceId?: string,
    private readonly serviceName?: string,
    private readonly spanId?: string,
  ) {}

  private emit(message: string, severity: SeverityNumber, data?: any): void {
    const attributes: Record<string, string | undefined> = {}

    if (this.traceId) {
      attributes.trace_id = this.traceId
    }
    if (this.spanId) {
      attributes.span_id = this.spanId
    }
    if (this.serviceName) {
      attributes['service.name'] = this.serviceName
    }
    if (data !== undefined) {
      attributes['log.data'] = typeof data === 'string' ? data : safeStringify(data)
    }

    if (this.otelLogger) {
      this.otelLogger.emit({
        severityNumber: severity,
        body: message,
        attributes: Object.keys(attributes).length > 0 ? attributes : undefined,
      })
    } else {
      // Fallback to console when OTEL is not available
      switch (severity) {
        case SeverityNumber.DEBUG:
          console.debug(message, data)
          break
        case SeverityNumber.INFO:
          console.info(message, data)
          break
        case SeverityNumber.WARN:
          console.warn(message, data)
          break
        case SeverityNumber.ERROR:
          console.error(message, data)
          break
        default:
          console.log(message, data)
      }
    }
  }

  info(message: string, data?: any): void {
    this.emit(message, SeverityNumber.INFO, data)
  }

  warn(message: string, data?: any): void {
    this.emit(message, SeverityNumber.WARN, data)
  }

  error(message: string, data?: any): void {
    this.emit(message, SeverityNumber.ERROR, data)
  }

  debug(message: string, data?: any): void {
    this.emit(message, SeverityNumber.DEBUG, data)
  }
}

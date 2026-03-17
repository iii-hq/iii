import { AnyValue, AnyValueMap, SeverityNumber } from '@opentelemetry/api-logs'
import {
  currentSpanId,
  currentTraceId,
  getLogger as getOtelLogger,
} from './telemetry-system'

/** @internal */
export type LoggerParams = {
  message: string
  trace_id?: string
  span_id?: string
  service_name?: string
  data?: unknown
  /** @deprecated Use service_name instead */
  function_name?: string
}

/**
 * Structured logger that emits logs as OpenTelemetry LogRecords.
 *
 * Every log call automatically captures the active trace and span context,
 * correlating your logs with distributed traces without any manual wiring.
 * When OTel is not initialized, Logger gracefully falls back to `console.*`.
 *
 * Pass structured data as the second argument to any log method. Using an
 * object of key-value pairs (instead of string interpolation) lets you
 * filter, aggregate, and build dashboards in your observability backend.
 *
 * @example
 * ```typescript
 * import { Logger } from 'iii-sdk'
 *
 * const logger = new Logger()
 *
 * // Basic logging — trace context is injected automatically
 * logger.info('Worker connected')
 *
 * // Structured context for dashboards and alerting
 * logger.info('Order processed', { orderId: 'ord_123', amount: 49.99, currency: 'USD' })
 * logger.warn('Retry attempt', { attempt: 3, maxRetries: 5, endpoint: '/api/charge' })
 * logger.error('Payment failed', { orderId: 'ord_123', gateway: 'stripe', errorCode: 'card_declined' })
 * ```
 */
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
    private readonly traceId?: string,
    private readonly serviceName?: string,
    private readonly spanId?: string,
  ) {}

  private emit(message: string, severity: SeverityNumber, data?: unknown): void {
    const attributes: AnyValueMap = {}
    const traceId = this.traceId ?? currentTraceId()
    const spanId = this.spanId ?? currentSpanId()

    if (traceId) {
      attributes.trace_id = traceId
    }
    if (spanId) {
      attributes.span_id = spanId
    }
    if (this.serviceName) {
      attributes['service.name'] = this.serviceName
    }
    if (data !== undefined) {
      attributes['log.data'] = data as AnyValue
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

  /**
   * Log an info-level message.
   *
   * @param message - Human-readable log message.
   * @param data - Structured context attached as OTel log attributes.
   *   Use key-value objects to enable filtering and aggregation in your
   *   observability backend (e.g. Grafana, Datadog, New Relic).
   *
   * @example
   * ```typescript
   * logger.info('Order processed', { orderId: 'ord_123', status: 'completed' })
   * ```
   */
  info(message: string, data?: unknown): void {
    this.emit(message, SeverityNumber.INFO, data)
  }

  /**
   * Log a warning-level message.
   *
   * @param message - Human-readable log message.
   * @param data - Structured context attached as OTel log attributes.
   *   Use key-value objects to enable filtering and aggregation in your
   *   observability backend (e.g. Grafana, Datadog, New Relic).
   *
   * @example
   * ```typescript
   * logger.warn('Retry attempt', { attempt: 3, maxRetries: 5, endpoint: '/api/charge' })
   * ```
   */
  warn(message: string, data?: unknown): void {
    this.emit(message, SeverityNumber.WARN, data)
  }

  /**
   * Log an error-level message.
   *
   * @param message - Human-readable log message.
   * @param data - Structured context attached as OTel log attributes.
   *   Use key-value objects to enable filtering and aggregation in your
   *   observability backend (e.g. Grafana, Datadog, New Relic).
   *
   * @example
   * ```typescript
   * logger.error('Payment failed', { orderId: 'ord_123', gateway: 'stripe', errorCode: 'card_declined' })
   * ```
   */
  error(message: string, data?: unknown): void {
    this.emit(message, SeverityNumber.ERROR, data)
  }

  /**
   * Log a debug-level message.
   *
   * @param message - Human-readable log message.
   * @param data - Structured context attached as OTel log attributes.
   *   Use key-value objects to enable filtering and aggregation in your
   *   observability backend (e.g. Grafana, Datadog, New Relic).
   *
   * @example
   * ```typescript
   * logger.debug('Cache lookup', { key: 'user:42', hit: false })
   * ```
   */
  debug(message: string, data?: unknown): void {
    this.emit(message, SeverityNumber.DEBUG, data)
  }
}

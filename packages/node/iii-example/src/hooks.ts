import { type ApiRequest, type ApiResponse, type Context, type FunctionMessage, getContext, getLogger, safeStringify, SeverityNumber } from '@iii-dev/sdk'
import { bridge } from './bridge'

/** OTEL-compatible attribute value types */
type OtelAttributeValue = string | number | boolean | string[] | number[] | boolean[]

/**
 * Sanitize attributes for OTEL compatibility.
 * OTEL attributes only support primitive values (string, number, boolean) or arrays of primitives.
 * Complex objects are automatically JSON-stringified.
 */
function sanitizeAttributes(attrs: Record<string, unknown>): Record<string, OtelAttributeValue> {
  const result: Record<string, OtelAttributeValue> = {}
  
  for (const [key, value] of Object.entries(attrs)) {
    if (value === null || value === undefined) {
      continue // Skip null/undefined values
    }
    
    if (typeof value === 'string' || typeof value === 'number' || typeof value === 'boolean') {
      result[key] = value
    } else if (Array.isArray(value)) {
      // Check if it's an array of primitives
      if (value.every(v => typeof v === 'string')) {
        result[key] = value as string[]
      } else if (value.every(v => typeof v === 'number')) {
        result[key] = value as number[]
      } else if (value.every(v => typeof v === 'boolean')) {
        result[key] = value as boolean[]
      } else {
        // Mixed or complex array - stringify
        result[key] = safeStringify(value)
      }
    } else {
      // Complex object - stringify
      result[key] = safeStringify(value)
    }
  }
  
  return result
}

/**
 * Helper for OTEL-native logging with clean syntax.
 * 
 * This uses the OpenTelemetry logger instead of the legacy ctx.logger.
 * Logs are automatically correlated with traces and spans.
 * 
 * Complex objects in attributes are automatically JSON-stringified since
 * OTEL only supports primitive attribute values.
 * 
 * @example
 * ```typescript
 * otelLog.info('User created', { userId: '123', email: 'user@example.com' })
 * otelLog.error('Database error', { error: err.message, code: err.code })
 * otelLog.info('Request received', { body: req.body }) // Objects auto-stringified
 * ```
 */
export const otelLog = {
  info: (message: string, attributes?: Record<string, unknown>) => {
    const sanitized = attributes ? sanitizeAttributes(attributes) : undefined
    getLogger()?.emit({ body: message, severityNumber: SeverityNumber.INFO, attributes: sanitized })
  },
  warn: (message: string, attributes?: Record<string, unknown>) => {
    const sanitized = attributes ? sanitizeAttributes(attributes) : undefined
    getLogger()?.emit({ body: message, severityNumber: SeverityNumber.WARN, attributes: sanitized })
  },
  error: (message: string, attributes?: Record<string, unknown>) => {
    const sanitized = attributes ? sanitizeAttributes(attributes) : undefined
    getLogger()?.emit({ body: message, severityNumber: SeverityNumber.ERROR, attributes: sanitized })
  },
  debug: (message: string, attributes?: Record<string, unknown>) => {
    const sanitized = attributes ? sanitizeAttributes(attributes) : undefined
    getLogger()?.emit({ body: message, severityNumber: SeverityNumber.DEBUG, attributes: sanitized })
  },
}

export const useApi = (
  config: { api_path: string; http_method: string; description?: string; metadata?: Record<string, any> },
  handler: (req: ApiRequest<any>, context: Context) => Promise<ApiResponse>,
) => {
  const function_path = `api.${config.http_method.toLowerCase()}.${config.api_path}`

  bridge.registerFunction({ function_path, metadata: config.metadata }, (req) => handler(req, getContext()))
  bridge.registerTrigger({
    trigger_type: 'api',
    function_path,
    config: {
      api_path: config.api_path,
      http_method: config.http_method,
      description: config.description,
      metadata: config.metadata,
    },
  })
}

export const useFunctionsAvailable = (callback: (functions: FunctionMessage[]) => void): (() => void) => {
  return bridge.onFunctionsAvailable(callback)
}

/**
 * OTEL Log Event - matches the StoredLog structure from the engine.
 * This is the new OTEL-compliant log format.
 */
export type OtelLogEvent = {
  /** Timestamp in Unix nanoseconds */
  timestamp_unix_nano: number
  /** Observed timestamp in Unix nanoseconds */
  observed_timestamp_unix_nano: number
  /** OTEL severity number (1-24): TRACE=1-4, DEBUG=5-8, INFO=9-12, WARN=13-16, ERROR=17-20, FATAL=21-24 */
  severity_number: number
  /** Severity text (e.g., "INFO", "WARN", "ERROR") */
  severity_text: string
  /** Log message body */
  body: string
  /** Structured attributes */
  attributes: Record<string, unknown>
  /** Trace ID for correlation (if available) */
  trace_id?: string
  /** Span ID for correlation (if available) */
  span_id?: string
  /** Resource attributes from the emitting service */
  resource: Record<string, string>
  /** Service name that emitted the log */
  service_name: string
  /** Instrumentation scope name (if available) */
  instrumentation_scope_name?: string
  /** Instrumentation scope version (if available) */
  instrumentation_scope_version?: string
}

/**
 * @deprecated Use OtelLogEvent instead. This type is kept for backward compatibility.
 */
export type LogEvent = {
  id: string
  data: Record<string, unknown>
  function_name: string
  message: string
  time: number
  trace_id: string
  level: 'info' | 'warn' | 'error'
}

/** OTEL Severity levels */
export type OtelSeverity = 'trace' | 'debug' | 'info' | 'warn' | 'error' | 'fatal'

/** Legacy log levels - kept for backward compatibility */
export type LogLevel = 'info' | 'warn' | 'error' | 'all'

/** Severity level for log triggers - supports both OTEL and legacy formats */
export type LogSeverityLevel = OtelSeverity | LogLevel

type LogTriggerConfig = { 
  /** Minimum severity level to trigger on */
  level: LogSeverityLevel
  /** Optional description for the trigger */
  description?: string 
}

type OtelLogTriggerHandler = (log: OtelLogEvent, context: Context) => Promise<void>


/**
 * Convert severity text to minimum OTEL severity number
 */
const severityTextToMinNumber = (level: LogSeverityLevel): number => {
  switch (level) {
    case 'trace': return 1
    case 'debug': return 5
    case 'info': return 9
    case 'warn': return 13
    case 'error': return 17
    case 'fatal': return 21
    case 'all': return 0
    default: return 0
  }
}

/**
 * Register a handler for OTEL log events.
 * 
 * This hook uses the new OTEL logging system. Logs are stored in OTEL format
 * and can be queried via the /api/logs REST endpoint or the logs.list function.
 * 
 * @example
 * ```typescript
 * useOnOtelLog({ level: 'error' }, async (log, ctx) => {
 *   console.log(`[${log.severity_text}] ${log.body}`)
 *   if (log.trace_id) {
 *     console.log(`  Trace: ${log.trace_id}`)
 *   }
 * })
 * ```
 */
let otelLogCounter = 0

export const useOnOtelLog = (config: LogTriggerConfig, handler: OtelLogTriggerHandler) => {
  const function_path = `onOtelLog.${config.level}.${++otelLogCounter}`
  const minSeverity = severityTextToMinNumber(config.level)

  bridge.registerFunction({ function_path, description: config.description, metadata: {} }, (log: OtelLogEvent) => {
    // Filter by severity if not 'all'
    if (config.level !== 'all' && log.severity_number < minSeverity) {
      return Promise.resolve()
    }
    return handler(log, getContext())
  })
  
  bridge.registerTrigger({
    trigger_type: 'otel_log',
    function_path,
    config: {
      level: config.level,
      severity_min: minSeverity,
      description: config.description,
    },
  })
}
/** OTEL Log Event from the engine */
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

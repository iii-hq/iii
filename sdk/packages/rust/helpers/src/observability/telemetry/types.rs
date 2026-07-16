/// Magic prefixes for binary frames over WebSocket
pub const PREFIX_TRACES: &[u8] = b"OTLP";
pub const PREFIX_METRICS: &[u8] = b"MTRC";
pub const PREFIX_LOGS: &[u8] = b"LOGS";

/// Connection state for the shared WebSocket
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
    Failed,
}

/// Configuration for WebSocket reconnection behavior
#[derive(Debug, Clone)]
pub struct ReconnectionConfig {
    /// Starting delay in milliseconds (default: 1000).
    pub initial_delay_ms: u64,
    /// Maximum delay cap in milliseconds (default: 30000).
    pub max_delay_ms: u64,
    /// Exponential backoff multiplier (default: 2).
    pub backoff_multiplier: f64,
    /// Random jitter factor, 0-1 (default: 0.3).
    pub jitter_factor: f64,
    /// Maximum retry attempts; `None` for infinite (default: `None`).
    pub max_retries: Option<u64>, // None for infinite
    /// Maximum messages preserved across reconnects. Messages beyond this limit
    /// are dropped to prevent delivering stale data after a long disconnect.
    /// This is intentionally smaller than `OtelConfig::channel_capacity` (the
    /// in-flight buffer between exporters and the WebSocket loop).
    pub max_pending_messages: usize,
}

impl Default for ReconnectionConfig {
    fn default() -> Self {
        Self {
            initial_delay_ms: 1000,
            max_delay_ms: 30000,
            backoff_multiplier: 2.0,
            jitter_factor: 0.3,
            max_retries: None,
            max_pending_messages: 1000,
        }
    }
}

impl ReconnectionConfig {
    /// Returns initial_delay_ms, clamped to a minimum of 1ms to prevent division by zero.
    pub fn effective_initial_delay_ms(&self) -> u64 {
        self.initial_delay_ms.max(1)
    }
}

/// Configuration for OpenTelemetry initialization
#[derive(Debug, Clone, Default)]
pub struct OtelConfig {
    /// Whether OpenTelemetry export is enabled. Defaults to true. Set to false or `OTEL_ENABLED=false/0/no/off` to disable.
    pub enabled: Option<bool>,
    /// The service name to report. Defaults to the `OTEL_SERVICE_NAME` env var.
    pub service_name: Option<String>,
    /// The service version to report. Defaults to the `SERVICE_VERSION` env var or "unknown".
    pub service_version: Option<String>,
    /// The service namespace to report. Defaults to the `SERVICE_NAMESPACE` env var.
    pub service_namespace: Option<String>,
    /// The service instance ID to report. Defaults to the `SERVICE_INSTANCE_ID` env var or an auto-generated UUID.
    pub service_instance_id: Option<String>,
    /// III Engine WebSocket URL. Defaults to the `III_URL` env var or "ws://localhost:49134".
    pub engine_ws_url: Option<String>,
    /// Whether metrics export is enabled. Defaults to true. Set to false or `OTEL_METRICS_ENABLED=false/0/no/off` to disable.
    pub metrics_enabled: Option<bool>,
    /// Metrics export interval in milliseconds. Defaults to 60000 (60 seconds).
    pub metrics_export_interval_ms: Option<u64>,
    /// Optional reconnection configuration for the WebSocket connection.
    pub reconnection_config: Option<ReconnectionConfig>,
    /// Timeout in milliseconds for the shutdown sequence (default: 10,000)
    pub shutdown_timeout_ms: Option<u64>,
    /// Capacity of the internal telemetry message channel (default: 10,000).
    /// This controls the in-flight message buffer between exporters and the
    /// WebSocket connection loop. Intentionally larger than
    /// `ReconnectionConfig::max_pending_messages` to absorb bursts during
    /// normal operation while limiting stale data across reconnects.
    pub channel_capacity: Option<usize>,
    /// Span processor flush delay in milliseconds. Defaults to 100ms when not
    /// set. The OpenTelemetry default of 5000ms is what makes traces appear
    /// seconds after the action. Env override: OTEL_SPANS_FLUSH_INTERVAL_MS.
    pub spans_flush_interval_ms: Option<u64>,
    /// Whether to enable the log exporter (default: true)
    pub logs_enabled: Option<bool>,
    /// Log processor flush delay in milliseconds. Defaults to 100ms when not set.
    pub logs_flush_interval_ms: Option<u64>,
    /// Maximum number of log records exported per batch. Defaults to 1 when not set.
    pub logs_batch_size: Option<usize>,
    /// Whether to auto-instrument outgoing HTTP calls.
    /// When `Some(true)` (default), `execute_traced_request()` can be used to
    /// create CLIENT spans for reqwest requests. Set `Some(false)` to opt out.
    /// `None` is treated as `true`.
    pub fetch_instrumentation_enabled: Option<bool>,
    /// Announce span STARTS to the engine as zero-end OTLP snapshots so live
    /// trace views render in-progress work (`LiveSpanStartProcessor`). One
    /// extra frame per span; the engine stores it as `pending` (or drops it
    /// when its live-span storage is off) and the final span replaces it in
    /// place. Default: enabled. Env override: `OTEL_LIVE_SPANS`, the same
    /// switch the engine uses for its own start mirroring.
    pub live_spans: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reconnection_config_defaults() {
        let config = ReconnectionConfig::default();
        assert_eq!(config.initial_delay_ms, 1000);
        assert_eq!(config.max_delay_ms, 30000);
        assert_eq!(config.backoff_multiplier, 2.0);
        assert_eq!(config.jitter_factor, 0.3);
        assert_eq!(config.max_retries, None);
        assert_eq!(config.max_pending_messages, 1000);
    }

    #[test]
    fn test_otel_config_defaults() {
        let config = OtelConfig::default();
        assert!(config.enabled.is_none());
        assert!(config.service_name.is_none());
        assert!(config.engine_ws_url.is_none());
        assert!(config.metrics_enabled.is_none());
        assert!(config.reconnection_config.is_none());
    }

    #[test]
    fn test_otel_config_has_fetch_instrumentation_enabled() {
        let config = OtelConfig::default();
        assert!(config.fetch_instrumentation_enabled.is_none());

        let config_disabled = OtelConfig {
            fetch_instrumentation_enabled: Some(false),
            ..Default::default()
        };
        assert_eq!(config_disabled.fetch_instrumentation_enabled, Some(false));
    }

    #[test]
    fn test_reconnection_config_zero_delay_clamped() {
        let config = ReconnectionConfig {
            initial_delay_ms: 0,
            ..Default::default()
        };
        assert_eq!(config.effective_initial_delay_ms(), 1);
    }

    #[test]
    fn test_otel_config_logs_batch_defaults() {
        let config = OtelConfig::default();
        assert!(config.logs_flush_interval_ms.is_none());
        assert!(config.logs_batch_size.is_none());
    }

    #[test]
    fn test_otel_config_logs_batch_explicit() {
        let config = OtelConfig {
            logs_flush_interval_ms: Some(200),
            logs_batch_size: Some(5),
            ..Default::default()
        };
        assert_eq!(config.logs_flush_interval_ms, Some(200));
        assert_eq!(config.logs_batch_size, Some(5));
    }
}

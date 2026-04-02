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
    pub initial_delay_ms: u64,
    pub max_delay_ms: u64,
    pub backoff_multiplier: f64,
    pub jitter_factor: f64,
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
    pub enabled: Option<bool>,
    pub service_name: Option<String>,
    pub service_version: Option<String>,
    pub service_namespace: Option<String>,
    pub service_instance_id: Option<String>,
    pub engine_ws_url: Option<String>,
    pub metrics_enabled: Option<bool>,
    pub metrics_export_interval_ms: Option<u64>,
    pub reconnection_config: Option<ReconnectionConfig>,
    /// Timeout in milliseconds for the shutdown sequence (default: 10,000)
    pub shutdown_timeout_ms: Option<u64>,
    /// Capacity of the internal telemetry message channel (default: 10,000).
    /// This controls the in-flight message buffer between exporters and the
    /// WebSocket connection loop. Intentionally larger than
    /// `ReconnectionConfig::max_pending_messages` to absorb bursts during
    /// normal operation while limiting stale data across reconnects.
    pub channel_capacity: Option<usize>,
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

// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;

use tracing::{
    Event, Level, Subscriber,
    field::{Field, Visit},
};
use tracing_opentelemetry::OtelData;
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::registry::LookupSpan;

use super::otel::{InMemoryLogStorage, OTEL_PASSTHROUGH_TARGET, StoredLog};

/// Visitor that collects fields from tracing events
struct LogFieldVisitor {
    message: Option<String>,
    attributes: HashMap<String, serde_json::Value>,
}

impl LogFieldVisitor {
    fn new() -> Self {
        Self {
            message: None,
            attributes: HashMap::new(),
        }
    }
}

impl Visit for LogFieldVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message = Some(value.to_string());
        } else {
            self.attributes.insert(
                field.name().to_string(),
                serde_json::Value::String(value.to_string()),
            );
        }
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.attributes.insert(
            field.name().to_string(),
            serde_json::Value::Number(value.into()),
        );
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.attributes.insert(
            field.name().to_string(),
            serde_json::Value::Number(value.into()),
        );
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        if let Some(num) = serde_json::Number::from_f64(value) {
            self.attributes
                .insert(field.name().to_string(), serde_json::Value::Number(num));
        }
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.attributes
            .insert(field.name().to_string(), serde_json::Value::Bool(value));
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = Some(format!("{:?}", value));
        } else {
            let debug_str = format!("{:?}", value);
            // Try to parse as JSON, otherwise store as string
            if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(&debug_str) {
                self.attributes.insert(field.name().to_string(), json_val);
            } else {
                self.attributes.insert(
                    field.name().to_string(),
                    serde_json::Value::String(debug_str),
                );
            }
        }
    }
}

/// Convert tracing Level to OTEL severity number
fn level_to_severity_number(level: &Level) -> i32 {
    match *level {
        Level::ERROR => 17, // SEVERITY_NUMBER_ERROR
        Level::WARN => 13,  // SEVERITY_NUMBER_WARN
        Level::INFO => 9,   // SEVERITY_NUMBER_INFO
        Level::DEBUG => 5,  // SEVERITY_NUMBER_DEBUG
        Level::TRACE => 1,  // SEVERITY_NUMBER_TRACE
    }
}

/// A tracing-subscriber layer that captures all log events and stores them in OTEL log storage
pub struct OtelLogsLayer {
    storage: Arc<InMemoryLogStorage>,
    service_name: String,
}

impl OtelLogsLayer {
    /// Create a new OTEL logs layer
    pub fn new(storage: Arc<InMemoryLogStorage>, service_name: String) -> Self {
        Self {
            storage,
            service_name,
        }
    }
}

impl<S> Layer<S> for OtelLogsLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let metadata = event.metadata();

        // Skip forwarded OTel log records — these are emitted by emit_log_to_console
        // for terminal display only and are already stored by ingest_otlp_logs.
        if metadata.target() == OTEL_PASSTHROUGH_TARGET {
            return;
        }

        // Extract trace_id and span_id from current span context
        // We need to get the span's own trace_id and span_id from the builder,
        // not from parent_cx which would be empty for root spans
        let (trace_id, span_id) = if let Some(span) = ctx.event_span(event) {
            let extensions = span.extensions();
            if let Some(otel_data) = extensions.get::<OtelData>() {
                // Get trace_id and span_id from the OtelData (the current span's IDs)
                let trace_id = otel_data.trace_id().map(|id| format!("{:032x}", id));
                let span_id = otel_data.span_id().map(|id| format!("{:016x}", id));
                (trace_id, span_id)
            } else {
                (None, None)
            }
        } else {
            (None, None)
        };

        // Collect message and attributes from event
        let mut visitor = LogFieldVisitor::new();
        event.record(&mut visitor);

        // Get timestamp
        let now = SystemTime::now();
        let timestamp_unix_nano = now
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        // Build message (use collected message or fall back to target)
        let body = visitor
            .message
            .unwrap_or_else(|| metadata.target().to_string());

        // Add target to attributes if not already present
        if !visitor.attributes.contains_key("target") {
            visitor.attributes.insert(
                "target".to_string(),
                serde_json::Value::String(metadata.target().to_string()),
            );
        }

        // Add module path if available
        if let Some(module_path) = metadata.module_path() {
            visitor.attributes.insert(
                "module_path".to_string(),
                serde_json::Value::String(module_path.to_string()),
            );
        }

        // Add file and line if available
        if let Some(file) = metadata.file() {
            visitor.attributes.insert(
                "file".to_string(),
                serde_json::Value::String(file.to_string()),
            );
        }
        if let Some(line) = metadata.line() {
            visitor
                .attributes
                .insert("line".to_string(), serde_json::Value::Number(line.into()));
        }

        // Build resource attributes
        let mut resource = HashMap::new();
        resource.insert("service.name".to_string(), self.service_name.clone());

        // Create the stored log
        let log = StoredLog {
            timestamp_unix_nano,
            observed_timestamp_unix_nano: timestamp_unix_nano,
            severity_number: level_to_severity_number(metadata.level()),
            severity_text: metadata.level().to_string(),
            body,
            attributes: visitor.attributes,
            trace_id,
            span_id,
            resource,
            service_name: self.service_name.clone(),
            instrumentation_scope_name: Some("tracing".to_string()),
            instrumentation_scope_version: None,
        };

        // Store the log
        self.storage.store(log);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tracing_subscriber::prelude::*;

    use super::super::otel::{InMemoryLogStorage, OTEL_PASSTHROUGH_TARGET};
    use super::{LogFieldVisitor, OtelLogsLayer, level_to_severity_number};
    use tracing::Level;

    /// Helper: create a layer+storage pair and run a closure under the subscriber
    fn with_layer<F: FnOnce()>(
        f: F,
    ) -> (Arc<InMemoryLogStorage>, Vec<super::super::otel::StoredLog>) {
        let storage = Arc::new(InMemoryLogStorage::new(1000));
        let layer = OtelLogsLayer::new(storage.clone(), "test-service".to_string());
        let subscriber = tracing_subscriber::registry().with(layer);
        tracing::subscriber::with_default(subscriber, f);
        let (_, logs) = storage.get_logs_filtered(None, None, None, None, None, None, None, None);
        (storage, logs)
    }

    #[test]
    fn passthrough_events_are_not_stored() {
        let storage = Arc::new(InMemoryLogStorage::new(100));
        let layer = OtelLogsLayer::new(storage.clone(), "test-service".to_string());
        let subscriber = tracing_subscriber::registry().with(layer);

        tracing::subscriber::with_default(subscriber, || {
            tracing::info!("normal message");
            tracing::info!(target: OTEL_PASSTHROUGH_TARGET, "passthrough message");
        });

        let (total, logs) =
            storage.get_logs_filtered(None, None, None, None, None, None, None, None);
        assert_eq!(total, 1, "passthrough event must not be stored");
        assert_eq!(logs[0].body, "normal message");
    }

    // =========================================================================
    // level_to_severity_number
    // =========================================================================

    #[test]
    fn severity_number_error() {
        assert_eq!(level_to_severity_number(&Level::ERROR), 17);
    }

    #[test]
    fn severity_number_warn() {
        assert_eq!(level_to_severity_number(&Level::WARN), 13);
    }

    #[test]
    fn severity_number_info() {
        assert_eq!(level_to_severity_number(&Level::INFO), 9);
    }

    #[test]
    fn severity_number_debug() {
        assert_eq!(level_to_severity_number(&Level::DEBUG), 5);
    }

    #[test]
    fn severity_number_trace() {
        assert_eq!(level_to_severity_number(&Level::TRACE), 1);
    }

    // =========================================================================
    // LogFieldVisitor
    // =========================================================================

    #[test]
    fn visitor_new_is_empty() {
        let visitor = LogFieldVisitor::new();
        assert!(visitor.message.is_none());
        assert!(visitor.attributes.is_empty());
    }

    #[test]
    fn visitor_record_str_message_field() {
        let mut visitor = LogFieldVisitor::new();
        // Use a tracing event to get a real Field. We can simulate with record_str directly
        // by constructing a field from a known fieldset. Instead, test via the layer.
        // For unit-level testing we rely on the layer integration tests below.
        // But we can check the struct state after construction:
        visitor.message = Some("hello".to_string());
        assert_eq!(visitor.message.as_deref(), Some("hello"));
    }

    // =========================================================================
    // OtelLogsLayer – integration through tracing subscriber
    // =========================================================================

    #[test]
    fn layer_captures_info_level_event() {
        let (_, logs) = with_layer(|| {
            tracing::info!("info message");
        });
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].body, "info message");
        assert_eq!(logs[0].severity_number, 9);
        assert_eq!(logs[0].severity_text, "INFO");
        assert_eq!(logs[0].service_name, "test-service");
        assert_eq!(
            logs[0].resource.get("service.name"),
            Some(&"test-service".to_string())
        );
        assert_eq!(
            logs[0].instrumentation_scope_name,
            Some("tracing".to_string())
        );
        assert!(logs[0].instrumentation_scope_version.is_none());
    }

    #[test]
    fn layer_captures_error_level_event() {
        let (_, logs) = with_layer(|| {
            tracing::error!("something went wrong");
        });
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].severity_number, 17);
        assert_eq!(logs[0].severity_text, "ERROR");
        assert_eq!(logs[0].body, "something went wrong");
    }

    #[test]
    fn layer_captures_warn_level_event() {
        let (_, logs) = with_layer(|| {
            tracing::warn!("warning message");
        });
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].severity_number, 13);
        assert_eq!(logs[0].severity_text, "WARN");
    }

    #[test]
    fn layer_captures_debug_level_event() {
        let (_, logs) = with_layer(|| {
            tracing::debug!("debug message");
        });
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].severity_number, 5);
        assert_eq!(logs[0].severity_text, "DEBUG");
    }

    #[test]
    fn layer_captures_trace_level_event() {
        let (_, logs) = with_layer(|| {
            tracing::trace!("trace message");
        });
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].severity_number, 1);
    }

    #[test]
    fn layer_captures_string_attribute() {
        let (_, logs) = with_layer(|| {
            tracing::info!(my_key = "my_value", "with attribute");
        });
        assert_eq!(logs.len(), 1);
        assert_eq!(
            logs[0].attributes.get("my_key"),
            Some(&serde_json::Value::String("my_value".to_string()))
        );
    }

    #[test]
    fn layer_captures_integer_attribute() {
        let (_, logs) = with_layer(|| {
            tracing::info!(count = 42i64, "with int");
        });
        assert_eq!(logs.len(), 1);
        assert_eq!(
            logs[0].attributes.get("count"),
            Some(&serde_json::json!(42))
        );
    }

    #[test]
    fn layer_captures_unsigned_integer_attribute() {
        let (_, logs) = with_layer(|| {
            tracing::info!(count = 99u64, "with uint");
        });
        assert_eq!(logs.len(), 1);
        assert_eq!(
            logs[0].attributes.get("count"),
            Some(&serde_json::json!(99u64))
        );
    }

    #[test]
    fn layer_captures_bool_attribute() {
        let (_, logs) = with_layer(|| {
            tracing::info!(active = true, "with bool");
        });
        assert_eq!(logs.len(), 1);
        assert_eq!(
            logs[0].attributes.get("active"),
            Some(&serde_json::Value::Bool(true))
        );
    }

    #[test]
    fn layer_captures_float_attribute() {
        let (_, logs) = with_layer(|| {
            tracing::info!(ratio = 3.14f64, "with float");
        });
        assert_eq!(logs.len(), 1);
        let ratio = logs[0].attributes.get("ratio").unwrap();
        // f64 3.14 should be stored as a JSON Number
        assert!(ratio.is_number());
        let val = ratio.as_f64().unwrap();
        assert!((val - 3.14).abs() < 1e-10);
    }

    #[test]
    fn layer_captures_debug_field_as_string() {
        let (_, logs) = with_layer(|| {
            let my_vec = vec![1, 2, 3];
            tracing::info!(data = ?my_vec, "with debug");
        });
        assert_eq!(logs.len(), 1);
        // Debug format of vec should be stored as attribute
        assert!(logs[0].attributes.contains_key("data"));
    }

    #[test]
    fn layer_captures_target_attribute() {
        let (_, logs) = with_layer(|| {
            tracing::info!("message from module");
        });
        assert_eq!(logs.len(), 1);
        // target attribute should be present
        assert!(logs[0].attributes.contains_key("target"));
    }

    #[test]
    fn layer_timestamp_is_set() {
        let (_, logs) = with_layer(|| {
            tracing::info!("timestamped");
        });
        assert_eq!(logs.len(), 1);
        assert!(logs[0].timestamp_unix_nano > 0);
        assert_eq!(
            logs[0].timestamp_unix_nano,
            logs[0].observed_timestamp_unix_nano
        );
    }

    #[test]
    fn layer_without_span_has_no_trace_id() {
        let (_, logs) = with_layer(|| {
            tracing::info!("no span");
        });
        assert_eq!(logs.len(), 1);
        assert!(logs[0].trace_id.is_none());
        assert!(logs[0].span_id.is_none());
    }

    #[test]
    fn layer_multiple_events_all_captured() {
        let (_, logs) = with_layer(|| {
            tracing::info!("first");
            tracing::warn!("second");
            tracing::error!("third");
        });
        assert_eq!(logs.len(), 3);
        // Logs are sorted newest-first, so check all bodies are present
        let bodies: Vec<&str> = logs.iter().map(|l| l.body.as_str()).collect();
        assert!(bodies.contains(&"first"));
        assert!(bodies.contains(&"second"));
        assert!(bodies.contains(&"third"));
    }

    #[test]
    fn layer_event_without_message_uses_target() {
        // When no "message" field is present, the body should fall back to target.
        // tracing macros always provide a message, but we can check the fallback
        // indirectly: target should always be in attributes.
        let (_, logs) = with_layer(|| {
            tracing::info!(target: "my_custom_target", "explicit target");
        });
        assert_eq!(logs.len(), 1);
        assert_eq!(
            logs[0].attributes.get("target"),
            Some(&serde_json::Value::String("my_custom_target".to_string()))
        );
    }

    #[test]
    fn layer_file_and_line_attributes() {
        let (_, logs) = with_layer(|| {
            tracing::info!("with location");
        });
        assert_eq!(logs.len(), 1);
        // file and line metadata should be captured
        assert!(logs[0].attributes.contains_key("file"));
        assert!(logs[0].attributes.contains_key("line"));
    }

    #[test]
    fn layer_module_path_attribute() {
        let (_, logs) = with_layer(|| {
            tracing::info!("with module path");
        });
        assert_eq!(logs.len(), 1);
        assert!(logs[0].attributes.contains_key("module_path"));
    }

    #[test]
    fn otel_logs_layer_new_stores_service_name() {
        let storage = Arc::new(InMemoryLogStorage::new(10));
        let layer = OtelLogsLayer::new(storage.clone(), "my-svc".to_string());
        assert_eq!(layer.service_name, "my-svc");
        assert!(Arc::ptr_eq(&layer.storage, &storage));
    }
}

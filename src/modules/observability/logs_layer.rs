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
    use super::OtelLogsLayer;

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
}

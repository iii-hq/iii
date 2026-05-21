use super::connection::SharedEngineConnection;
use super::json_serializer::{
    attrs_to_json, bytes_to_hex, resource_attrs_to_json, system_time_to_nanos_string,
};
use super::types::PREFIX_TRACES;
use opentelemetry::trace::{SpanId, SpanKind, Status};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::error::OTelSdkResult;
use opentelemetry_sdk::trace::{SpanData, SpanExporter};
use serde_json::json;
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};

/// Custom span exporter that sends OTLP JSON over a shared WebSocket connection.
///
/// Uses a hand-built JSON serializer (not opentelemetry-proto serde) to match
/// the format the III Engine expects: camelCase field names, integer attribute
/// values as JSON numbers, and hex-encoded trace/span IDs.
pub struct EngineSpanExporter {
    connection: Arc<SharedEngineConnection>,
    resource: Mutex<Option<Resource>>,
}

impl EngineSpanExporter {
    pub fn new(connection: Arc<SharedEngineConnection>) -> Self {
        Self {
            connection,
            resource: Mutex::new(None),
        }
    }
}

impl fmt::Debug for EngineSpanExporter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EngineSpanExporter")
            .field("resource", &self.resource)
            .finish()
    }
}

fn span_kind_to_int(kind: SpanKind) -> u32 {
    match kind {
        SpanKind::Internal => 1,
        SpanKind::Server => 2,
        SpanKind::Client => 3,
        SpanKind::Producer => 4,
        SpanKind::Consumer => 5,
    }
}

fn status_to_json(status: &Status) -> serde_json::Value {
    match status {
        Status::Unset => json!({ "code": 0, "message": "" }),
        Status::Ok => json!({ "code": 1, "message": "" }),
        Status::Error { description } => json!({ "code": 2, "message": description.as_ref() }),
    }
}

/// Serialize a batch of SpanData to OTLP JSON matching the III Engine format.
fn serialize_spans(batch: &[SpanData], resource: &Resource) -> Vec<u8> {
    // Group spans by instrumentation scope
    let mut scope_map: HashMap<(String, String), Vec<serde_json::Value>> = HashMap::new();

    for span in batch {
        let scope_name = span.instrumentation_scope.name().to_string();
        let scope_version = span
            .instrumentation_scope
            .version()
            .map(|v| v.to_string())
            .unwrap_or_default();

        let trace_id = bytes_to_hex(&span.span_context.trace_id().to_bytes());
        let span_id = bytes_to_hex(&span.span_context.span_id().to_bytes());

        let mut span_json = json!({
            "traceId": trace_id,
            "spanId": span_id,
            "name": span.name.as_ref(),
            "kind": span_kind_to_int(span.span_kind.clone()),
            "startTimeUnixNano": system_time_to_nanos_string(span.start_time),
            "endTimeUnixNano": system_time_to_nanos_string(span.end_time),
            "status": status_to_json(&span.status),
            "attributes": attrs_to_json(&span.attributes),
            "events": span.events.iter().map(|e| {
                json!({
                    "name": e.name.as_ref(),
                    "timeUnixNano": system_time_to_nanos_string(e.timestamp),
                    "attributes": attrs_to_json(&e.attributes),
                })
            }).collect::<Vec<_>>(),
            "links": span.links.iter().map(|l| {
                json!({
                    "traceId": bytes_to_hex(&l.span_context.trace_id().to_bytes()),
                    "spanId": bytes_to_hex(&l.span_context.span_id().to_bytes()),
                    "attributes": attrs_to_json(&l.attributes),
                })
            }).collect::<Vec<_>>(),
        });

        if span.parent_span_id != SpanId::INVALID {
            span_json.as_object_mut().unwrap().insert(
                "parentSpanId".to_string(),
                json!(bytes_to_hex(&span.parent_span_id.to_bytes())),
            );
        }

        scope_map
            .entry((scope_name, scope_version))
            .or_default()
            .push(span_json);
    }

    let resource_attrs = resource_attrs_to_json(resource.iter());

    let scope_spans: Vec<serde_json::Value> = scope_map
        .into_iter()
        .map(|((name, version), spans)| {
            json!({
                "scope": { "name": name, "version": version },
                "spans": spans,
            })
        })
        .collect();

    let result = json!({
        "resourceSpans": [{
            "resource": { "attributes": resource_attrs },
            "scopeSpans": scope_spans,
        }]
    });

    serde_json::to_vec(&result).unwrap_or_default()
}

impl SpanExporter for EngineSpanExporter {
    fn export(
        &self,
        batch: Vec<SpanData>,
    ) -> impl futures_util::Future<Output = OTelSdkResult> + Send {
        let is_empty = batch.is_empty();

        let resource = self
            .resource
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let connection = self.connection.clone();

        async move {
            if is_empty {
                return Ok(());
            }

            let default_resource = Resource::builder_empty().build();
            let res = resource.as_ref().unwrap_or(&default_resource);
            let json = serialize_spans(&batch, res);

            connection
                .send(PREFIX_TRACES, json)
                .map_err(opentelemetry_sdk::error::OTelSdkError::InternalFailure)
        }
    }

    fn shutdown(&mut self) -> OTelSdkResult {
        Ok(())
    }

    /// No-op: the synchronous SpanExporter trait cannot perform async I/O.
    /// Use `flush_otel()` for a full async flush of the connection layer.
    fn force_flush(&mut self) -> OTelSdkResult {
        Ok(())
    }

    fn set_resource(&mut self, resource: &Resource) {
        *self.resource.lock().unwrap_or_else(|e| e.into_inner()) = Some(resource.clone());
    }
}

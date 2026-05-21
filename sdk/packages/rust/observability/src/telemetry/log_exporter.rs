use super::connection::SharedEngineConnection;
use super::json_serializer::{anyvalue_to_json, bytes_to_hex, resource_attrs_to_json};
use super::types::PREFIX_LOGS;
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::error::OTelSdkResult;
use opentelemetry_sdk::logs::{LogBatch, LogExporter};
use serde_json::json;
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};

/// Custom log exporter that sends OTLP JSON over a shared WebSocket connection.
///
/// Uses a hand-built JSON serializer to match the III Engine's expected format.
pub struct EngineLogExporter {
    connection: Arc<SharedEngineConnection>,
    resource: Mutex<Option<Resource>>,
}

impl EngineLogExporter {
    pub fn new(connection: Arc<SharedEngineConnection>) -> Self {
        Self {
            connection,
            resource: Mutex::new(None),
        }
    }
}

impl fmt::Debug for EngineLogExporter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EngineLogExporter")
            .field("resource", &self.resource)
            .finish()
    }
}

impl LogExporter for EngineLogExporter {
    async fn export(&self, batch: LogBatch<'_>) -> OTelSdkResult {
        let records: Vec<_> = batch.iter().collect();
        if records.is_empty() {
            return Ok(());
        }

        let default_resource = Resource::builder().build();
        let resource_guard = self.resource.lock().unwrap_or_else(|e| e.into_inner());
        let resource = resource_guard.as_ref().unwrap_or(&default_resource);

        // Group log records by scope
        let mut scope_map: HashMap<(String, String), Vec<serde_json::Value>> = HashMap::new();

        for (record, scope) in &records {
            let scope_name = scope.name().to_string();
            let scope_version = scope.version().map(|v| v.to_string()).unwrap_or_default();

            let trace_id = record
                .trace_context()
                .map(|tc| bytes_to_hex(&tc.trace_id.to_bytes()))
                .unwrap_or_default();
            let span_id = record
                .trace_context()
                .map(|tc| bytes_to_hex(&tc.span_id.to_bytes()))
                .unwrap_or_default();

            let timestamp = record
                .timestamp()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_nanos().to_string())
                .unwrap_or_else(|| "0".to_string());

            let observed_timestamp = record
                .observed_timestamp()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_nanos().to_string())
                .unwrap_or_else(|| "0".to_string());

            let severity_number = record.severity_number().map(|s| s as i32).unwrap_or(0);
            let severity_text = record
                .severity_text()
                .map(|s| s.to_string())
                .unwrap_or_default();

            let body = record
                .body()
                .map(anyvalue_to_json)
                .unwrap_or_else(|| serde_json::json!({ "stringValue": "" }));

            let attributes: Vec<serde_json::Value> = record
                .attributes_iter()
                .map(|(k, v)| {
                    json!({
                        "key": k.as_str(),
                        "value": anyvalue_to_json(v)
                    })
                })
                .collect();

            let mut log_json = json!({
                "timeUnixNano": timestamp,
                "observedTimeUnixNano": observed_timestamp,
                "severityNumber": severity_number,
                "severityText": severity_text,
                "body": body,
                "attributes": attributes,
            });

            if !trace_id.is_empty() && trace_id != "00000000000000000000000000000000" {
                log_json
                    .as_object_mut()
                    .unwrap()
                    .insert("traceId".to_string(), json!(trace_id));
            }
            if !span_id.is_empty() && span_id != "0000000000000000" {
                log_json
                    .as_object_mut()
                    .unwrap()
                    .insert("spanId".to_string(), json!(span_id));
            }

            scope_map
                .entry((scope_name, scope_version))
                .or_default()
                .push(log_json);
        }

        let resource_attrs = resource_attrs_to_json(resource.iter());

        let scope_logs: Vec<serde_json::Value> = scope_map
            .into_iter()
            .map(|((name, version), log_records)| {
                json!({
                    "scope": { "name": name, "version": version },
                    "logRecords": log_records,
                })
            })
            .collect();

        let result = json!({
            "resourceLogs": [{
                "resource": { "attributes": resource_attrs },
                "scopeLogs": scope_logs,
            }]
        });

        let json_bytes = serde_json::to_vec(&result)
            .map_err(|e| opentelemetry_sdk::error::OTelSdkError::InternalFailure(e.to_string()))?;

        self.connection
            .send(PREFIX_LOGS, json_bytes)
            .map_err(opentelemetry_sdk::error::OTelSdkError::InternalFailure)
    }

    fn shutdown(&self) -> OTelSdkResult {
        Ok(())
    }

    fn set_resource(&mut self, resource: &Resource) {
        *self.resource.lock().unwrap_or_else(|e| e.into_inner()) = Some(resource.clone());
    }
}

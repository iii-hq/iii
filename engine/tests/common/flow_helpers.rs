#![allow(dead_code)]

use std::sync::Once;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail};
use iii::telemetry::{ExporterType, OtelConfig, get_span_storage, init_otel};
use iii::workers::observability::otel::StoredSpan;
use serde::Serialize;
use serde_json::Value;
use tokio::time::sleep;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

static FLOW_TRACING_INIT: Once = Once::new();

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FlowGraphNode {
    pub id: String,
    pub kind: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FlowGraphEdge {
    pub id: String,
    pub kind: String,
    pub source: String,
    pub target: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ValidatedFlowGraphAsset {
    pub id: String,
    pub name: String,
    pub nodes: Vec<FlowGraphNode>,
    pub edges: Vec<FlowGraphEdge>,
}

#[derive(Debug, Clone)]
pub struct HttpQueueStatePathSpec {
    pub id: String,
    pub name: String,
    pub method: String,
    pub path: String,
    pub entry_function_id: String,
    pub queue_topic: String,
    pub queue_consumer_function_id: String,
    pub state_scope: String,
    pub state_key: String,
    pub state_observer_function_id: String,
}

impl HttpQueueStatePathSpec {
    pub async fn wait_for_validated_asset(
        &self,
        state_event: &Value,
        timeout: Duration,
    ) -> anyhow::Result<ValidatedFlowGraphAsset> {
        let deadline = Instant::now() + timeout;
        let mut last_error: Option<anyhow::Error> = None;

        loop {
            match self.try_build_asset(state_event) {
                Ok(asset) => return Ok(asset),
                Err(err) if Instant::now() < deadline => {
                    last_error = Some(err);
                    sleep(Duration::from_millis(25)).await;
                }
                Err(err) => {
                    let last_error = last_error.unwrap_or(err);
                    return Err(last_error.context("timed out waiting for validated flow asset"));
                }
            }
        }
    }

    fn try_build_asset(&self, state_event: &Value) -> anyhow::Result<ValidatedFlowGraphAsset> {
        validate_state_event(state_event, &self.state_scope, &self.state_key)?;

        let storage =
            get_span_storage().ok_or_else(|| anyhow!("span storage is not initialized"))?;
        let spans = storage.get_spans();

        let http_root = spans
            .iter()
            .find(|span| span.name == format!("{} {}", self.method, self.path))
            .ok_or_else(|| anyhow!("missing HTTP span for {} {}", self.method, self.path))?;

        let entry_call = find_child_span(
            &spans,
            http_root,
            &format!("call {}", self.entry_function_id),
            None,
        )?;
        let enqueue_call = find_child_span(
            &spans,
            entry_call,
            "call iii::durable::publish",
            Some(("messaging.destination.name", self.queue_topic.as_str())),
        )?;
        let queue_job = find_child_span(
            &spans,
            enqueue_call,
            &format!(
                "queue {}::{}",
                self.queue_topic, self.queue_consumer_function_id
            ),
            Some((
                "queue",
                &format!("{}::{}", self.queue_topic, self.queue_consumer_function_id),
            )),
        )?;
        let consumer_call = find_child_span(
            &spans,
            queue_job,
            &format!("call {}", self.queue_consumer_function_id),
            None,
        )?;
        let state_set = find_child_span(&spans, consumer_call, "call state::set", None)?;
        let state_triggers = find_child_span(&spans, state_set, "state_triggers", None)?;
        let _observer_call = find_child_span(
            &spans,
            state_triggers,
            &format!("call {}", self.state_observer_function_id),
            None,
        )?;

        let http_node_id = format!("http:{}:{}", self.method, self.path);
        let entry_node_id = self.entry_function_id.clone();
        let queue_node_id = format!("queue:{}", self.queue_topic);
        let consumer_node_id = self.queue_consumer_function_id.clone();
        let state_node_id = format!("state:{}:{}", self.state_scope, self.state_key);
        let observer_node_id = self.state_observer_function_id.clone();

        Ok(ValidatedFlowGraphAsset {
            id: self.id.clone(),
            name: self.name.clone(),
            nodes: vec![
                FlowGraphNode {
                    id: http_node_id.clone(),
                    kind: "http_trigger".to_string(),
                    label: format!("{} {}", self.method, self.path),
                },
                FlowGraphNode {
                    id: entry_node_id.clone(),
                    kind: "function".to_string(),
                    label: self.entry_function_id.clone(),
                },
                FlowGraphNode {
                    id: queue_node_id.clone(),
                    kind: "queue_topic".to_string(),
                    label: self.queue_topic.clone(),
                },
                FlowGraphNode {
                    id: consumer_node_id.clone(),
                    kind: "function".to_string(),
                    label: self.queue_consumer_function_id.clone(),
                },
                FlowGraphNode {
                    id: state_node_id.clone(),
                    kind: "state_slot".to_string(),
                    label: format!("{}/{}", self.state_scope, self.state_key),
                },
                FlowGraphNode {
                    id: observer_node_id.clone(),
                    kind: "function".to_string(),
                    label: self.state_observer_function_id.clone(),
                },
            ],
            edges: vec![
                FlowGraphEdge {
                    id: format!("{http_node_id}->{entry_node_id}"),
                    kind: "http_trigger".to_string(),
                    source: http_node_id,
                    target: entry_node_id.clone(),
                    label: format!("{} {}", self.method, self.path),
                },
                FlowGraphEdge {
                    id: format!("{entry_node_id}->{queue_node_id}"),
                    kind: "enqueue".to_string(),
                    source: entry_node_id,
                    target: queue_node_id.clone(),
                    label: self.queue_topic.clone(),
                },
                FlowGraphEdge {
                    id: format!("{queue_node_id}->{consumer_node_id}"),
                    kind: "queue_trigger".to_string(),
                    source: queue_node_id,
                    target: consumer_node_id.clone(),
                    label: self.queue_topic.clone(),
                },
                FlowGraphEdge {
                    id: format!("{consumer_node_id}->{state_node_id}"),
                    kind: "state_write".to_string(),
                    source: consumer_node_id,
                    target: state_node_id.clone(),
                    label: format!("{}/{}", self.state_scope, self.state_key),
                },
                FlowGraphEdge {
                    id: format!("{state_node_id}->{observer_node_id}"),
                    kind: "state_trigger".to_string(),
                    source: state_node_id,
                    target: observer_node_id,
                    label: self.state_scope.clone(),
                },
            ],
        })
    }
}

pub fn ensure_flow_test_tracing() {
    iii::workers::observability::metrics::ensure_default_meter();

    FLOW_TRACING_INIT.call_once(|| {
        let config = OtelConfig {
            enabled: true,
            service_name: "iii-flow-test".to_string(),
            service_version: "0.1.0".to_string(),
            service_namespace: None,
            exporter: ExporterType::Memory,
            endpoint: "http://127.0.0.1:4317".to_string(),
            sampling_ratio: 1.0,
            memory_max_spans: 2048,
        };

        tracing_subscriber::registry()
            .with(init_otel(&config))
            .try_init()
            .expect("flow test tracing subscriber should initialize");
    });

    if let Some(storage) = get_span_storage() {
        storage.clear();
    } else {
        panic!("missing span storage: tracing subscriber not installed for flow tests");
    }
}

fn validate_state_event(
    state_event: &Value,
    expected_scope: &str,
    expected_key: &str,
) -> anyhow::Result<()> {
    let actual_scope = state_event
        .get("scope")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("state event is missing scope"))?;
    let actual_key = state_event
        .get("key")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("state event is missing key"))?;

    if actual_scope != expected_scope {
        bail!(
            "state event scope mismatch: expected {}, got {}",
            expected_scope,
            actual_scope
        );
    }

    if actual_key != expected_key {
        bail!(
            "state event key mismatch: expected {}, got {}",
            expected_key,
            actual_key
        );
    }

    Ok(())
}

fn find_child_span<'a>(
    spans: &'a [StoredSpan],
    parent: &StoredSpan,
    expected_name: &str,
    attribute: Option<(&str, &str)>,
) -> anyhow::Result<&'a StoredSpan> {
    spans
        .iter()
        .find(|span| {
            span.parent_span_id.as_deref() == Some(parent.span_id.as_str())
                && span.name == expected_name
                && attribute
                    .map(|(key, value)| has_attribute(span, key, value))
                    .unwrap_or(true)
        })
        .ok_or_else(|| {
            anyhow!(
                "missing child span '{}' under parent '{}'",
                expected_name,
                parent.name
            )
        })
}

fn has_attribute(span: &StoredSpan, expected_key: &str, expected_value: &str) -> bool {
    span.attributes
        .iter()
        .any(|(key, value)| key == expected_key && value == expected_value)
}

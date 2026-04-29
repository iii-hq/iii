#![allow(dead_code)]

// Validated flow tests use this helper in three steps:
// 1. run a real engine path with in-memory tracing enabled,
// 2. describe the expected nodes, edges, and span evidence,
// 3. generate the graph asset only after the span evidence is present.

use std::collections::HashMap;
use std::sync::Once;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail};
use iii::telemetry::{ExporterType, OtelConfig, get_span_storage, init_otel};
use iii::workers::observability::otel::StoredSpan;
use serde::Serialize;
use serde_json::Value;
use tokio::sync::{Mutex, MutexGuard};
use tokio::time::sleep;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

static FLOW_TRACING_INIT: Once = Once::new();
static FLOW_TRACING_LOCK: Mutex<()> = Mutex::const_new(());

pub struct FlowTracingGuard {
    _guard: MutexGuard<'static, ()>,
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpectedSpanAttribute {
    pub key: String,
    pub value: String,
}

/// Span evidence required before a flow asset can be generated.
///
/// `parent_id` validates direct parent-child trace structure. `starts_after_id`
/// handles fire-and-forget paths, such as local pub/sub, where execution hops to
/// a spawned task without retaining direct span parenting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpectedFlowSpan {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub starts_after_id: Option<String>,
    pub attribute: Option<ExpectedSpanAttribute>,
}

#[derive(Debug, Clone)]
pub struct ValidatedFlowPathSpec {
    pub id: String,
    pub name: String,
    pub nodes: Vec<FlowGraphNode>,
    pub edges: Vec<FlowGraphEdge>,
    pub spans: Vec<ExpectedFlowSpan>,
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

#[derive(Debug, Clone)]
pub struct HttpStreamPathSpec {
    pub id: String,
    pub name: String,
    pub method: String,
    pub path: String,
    pub entry_function_id: String,
    pub stream_name: String,
    pub group_id: String,
    pub item_id: String,
    pub stream_observer_function_id: String,
}

#[derive(Debug, Clone)]
pub struct HttpPubSubPathSpec {
    pub id: String,
    pub name: String,
    pub method: String,
    pub path: String,
    pub entry_function_id: String,
    pub topic: String,
    pub expected_event: Value,
    pub subscriber_function_id: String,
}

impl ValidatedFlowPathSpec {
    pub async fn wait_for_asset(
        &self,
        timeout: Duration,
    ) -> anyhow::Result<ValidatedFlowGraphAsset> {
        let deadline = Instant::now() + timeout;
        let mut last_error: Option<anyhow::Error> = None;

        loop {
            match self.try_build_asset() {
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

    fn try_build_asset(&self) -> anyhow::Result<ValidatedFlowGraphAsset> {
        let storage =
            get_span_storage().ok_or_else(|| anyhow!("span storage is not initialized"))?;
        let spans = storage.get_spans();

        validate_expected_spans(&spans, &self.spans)?;

        Ok(ValidatedFlowGraphAsset {
            id: self.id.clone(),
            name: self.name.clone(),
            nodes: self.nodes.clone(),
            edges: self.edges.clone(),
        })
    }
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
        self.declaration().try_build_asset()
    }

    fn declaration(&self) -> ValidatedFlowPathSpec {
        let http_node_id = format!("http:{}:{}", self.method, self.path);
        let entry_node_id = self.entry_function_id.clone();
        let queue_node_id = format!("queue:{}", self.queue_topic);
        let consumer_node_id = self.queue_consumer_function_id.clone();
        let state_node_id = format!("state:{}:{}", self.state_scope, self.state_key);
        let observer_node_id = self.state_observer_function_id.clone();
        let queue_span_name = format!(
            "queue {}::{}",
            self.queue_topic, self.queue_consumer_function_id
        );
        let queue_span_attribute =
            format!("{}::{}", self.queue_topic, self.queue_consumer_function_id);

        ValidatedFlowPathSpec {
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
            spans: vec![
                ExpectedFlowSpan {
                    id: "http".to_string(),
                    name: format!("{} {}", self.method, self.path),
                    parent_id: None,
                    starts_after_id: None,
                    attribute: None,
                },
                ExpectedFlowSpan {
                    id: "entry".to_string(),
                    name: format!("call {}", self.entry_function_id),
                    parent_id: Some("http".to_string()),
                    starts_after_id: None,
                    attribute: None,
                },
                ExpectedFlowSpan {
                    id: "publish".to_string(),
                    name: "call iii::durable::publish".to_string(),
                    parent_id: Some("entry".to_string()),
                    starts_after_id: None,
                    attribute: Some(ExpectedSpanAttribute {
                        key: "messaging.destination.name".to_string(),
                        value: self.queue_topic.clone(),
                    }),
                },
                ExpectedFlowSpan {
                    id: "queue_job".to_string(),
                    name: queue_span_name,
                    parent_id: Some("publish".to_string()),
                    starts_after_id: None,
                    attribute: Some(ExpectedSpanAttribute {
                        key: "queue".to_string(),
                        value: queue_span_attribute,
                    }),
                },
                ExpectedFlowSpan {
                    id: "consumer".to_string(),
                    name: format!("call {}", self.queue_consumer_function_id),
                    parent_id: Some("queue_job".to_string()),
                    starts_after_id: None,
                    attribute: None,
                },
                ExpectedFlowSpan {
                    id: "state_set".to_string(),
                    name: "call state::set".to_string(),
                    parent_id: Some("consumer".to_string()),
                    starts_after_id: None,
                    attribute: None,
                },
                ExpectedFlowSpan {
                    id: "state_triggers".to_string(),
                    name: "state_triggers".to_string(),
                    parent_id: Some("state_set".to_string()),
                    starts_after_id: None,
                    attribute: None,
                },
                ExpectedFlowSpan {
                    id: "state_observer".to_string(),
                    name: format!("call {}", self.state_observer_function_id),
                    parent_id: Some("state_triggers".to_string()),
                    starts_after_id: None,
                    attribute: None,
                },
            ],
        }
    }
}

impl HttpStreamPathSpec {
    pub async fn wait_for_validated_asset(
        &self,
        stream_event: &Value,
        timeout: Duration,
    ) -> anyhow::Result<ValidatedFlowGraphAsset> {
        let deadline = Instant::now() + timeout;
        let mut last_error: Option<anyhow::Error> = None;

        loop {
            match self.try_build_asset(stream_event) {
                Ok(asset) => return Ok(asset),
                Err(err) if Instant::now() < deadline => {
                    last_error = Some(err);
                    sleep(Duration::from_millis(25)).await;
                }
                Err(err) => {
                    let last_error = last_error.unwrap_or(err);
                    return Err(
                        last_error.context("timed out waiting for validated stream flow asset")
                    );
                }
            }
        }
    }

    fn try_build_asset(&self, stream_event: &Value) -> anyhow::Result<ValidatedFlowGraphAsset> {
        validate_stream_event(
            stream_event,
            &self.stream_name,
            &self.group_id,
            &self.item_id,
        )?;
        self.declaration().try_build_asset()
    }

    fn declaration(&self) -> ValidatedFlowPathSpec {
        let http_node_id = format!("http:{}:{}", self.method, self.path);
        let entry_node_id = self.entry_function_id.clone();
        let stream_node_id = format!(
            "stream:{}:{}:{}",
            self.stream_name, self.group_id, self.item_id
        );
        let observer_node_id = self.stream_observer_function_id.clone();
        let stream_label = format!("{}/{}/{}", self.stream_name, self.group_id, self.item_id);

        ValidatedFlowPathSpec {
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
                    id: stream_node_id.clone(),
                    kind: "stream_item".to_string(),
                    label: stream_label.clone(),
                },
                FlowGraphNode {
                    id: observer_node_id.clone(),
                    kind: "function".to_string(),
                    label: self.stream_observer_function_id.clone(),
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
                    id: format!("{entry_node_id}->{stream_node_id}"),
                    kind: "stream_write".to_string(),
                    source: entry_node_id,
                    target: stream_node_id.clone(),
                    label: stream_label.clone(),
                },
                FlowGraphEdge {
                    id: format!("{stream_node_id}->{observer_node_id}"),
                    kind: "stream_trigger".to_string(),
                    source: stream_node_id,
                    target: observer_node_id,
                    label: self.stream_name.clone(),
                },
            ],
            spans: vec![
                ExpectedFlowSpan {
                    id: "http".to_string(),
                    name: format!("{} {}", self.method, self.path),
                    parent_id: None,
                    starts_after_id: None,
                    attribute: None,
                },
                ExpectedFlowSpan {
                    id: "entry".to_string(),
                    name: format!("call {}", self.entry_function_id),
                    parent_id: Some("http".to_string()),
                    starts_after_id: None,
                    attribute: None,
                },
                ExpectedFlowSpan {
                    id: "stream_set".to_string(),
                    name: "call stream::set".to_string(),
                    parent_id: Some("entry".to_string()),
                    starts_after_id: None,
                    attribute: None,
                },
                ExpectedFlowSpan {
                    id: "stream_triggers".to_string(),
                    name: "stream_triggers".to_string(),
                    parent_id: Some("stream_set".to_string()),
                    starts_after_id: None,
                    attribute: None,
                },
                ExpectedFlowSpan {
                    id: "stream_observer".to_string(),
                    name: format!("call {}", self.stream_observer_function_id),
                    parent_id: Some("stream_triggers".to_string()),
                    starts_after_id: None,
                    attribute: None,
                },
            ],
        }
    }
}

impl HttpPubSubPathSpec {
    pub async fn wait_for_validated_asset(
        &self,
        pubsub_event: &Value,
        timeout: Duration,
    ) -> anyhow::Result<ValidatedFlowGraphAsset> {
        let deadline = Instant::now() + timeout;
        let mut last_error: Option<anyhow::Error> = None;

        loop {
            match self.try_build_asset(pubsub_event) {
                Ok(asset) => return Ok(asset),
                Err(err) if Instant::now() < deadline => {
                    last_error = Some(err);
                    sleep(Duration::from_millis(25)).await;
                }
                Err(err) => {
                    let last_error = last_error.unwrap_or(err);
                    return Err(
                        last_error.context("timed out waiting for validated pubsub flow asset")
                    );
                }
            }
        }
    }

    fn try_build_asset(&self, pubsub_event: &Value) -> anyhow::Result<ValidatedFlowGraphAsset> {
        validate_pubsub_event(pubsub_event, &self.expected_event)?;
        self.declaration().try_build_asset()
    }

    fn declaration(&self) -> ValidatedFlowPathSpec {
        let http_node_id = format!("http:{}:{}", self.method, self.path);
        let entry_node_id = self.entry_function_id.clone();
        let topic_node_id = format!("pubsub:{}", self.topic);
        let subscriber_node_id = self.subscriber_function_id.clone();

        ValidatedFlowPathSpec {
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
                    id: topic_node_id.clone(),
                    kind: "pubsub_topic".to_string(),
                    label: self.topic.clone(),
                },
                FlowGraphNode {
                    id: subscriber_node_id.clone(),
                    kind: "function".to_string(),
                    label: self.subscriber_function_id.clone(),
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
                    id: format!("{entry_node_id}->{topic_node_id}"),
                    kind: "pubsub_publish".to_string(),
                    source: entry_node_id,
                    target: topic_node_id.clone(),
                    label: self.topic.clone(),
                },
                FlowGraphEdge {
                    id: format!("{topic_node_id}->{subscriber_node_id}"),
                    kind: "subscribe_trigger".to_string(),
                    source: topic_node_id,
                    target: subscriber_node_id,
                    label: self.topic.clone(),
                },
            ],
            spans: vec![
                ExpectedFlowSpan {
                    id: "http".to_string(),
                    name: format!("{} {}", self.method, self.path),
                    parent_id: None,
                    starts_after_id: None,
                    attribute: None,
                },
                ExpectedFlowSpan {
                    id: "entry".to_string(),
                    name: format!("call {}", self.entry_function_id),
                    parent_id: Some("http".to_string()),
                    starts_after_id: None,
                    attribute: None,
                },
                ExpectedFlowSpan {
                    id: "publish".to_string(),
                    name: "call publish".to_string(),
                    parent_id: Some("entry".to_string()),
                    starts_after_id: None,
                    attribute: None,
                },
                ExpectedFlowSpan {
                    id: "subscriber".to_string(),
                    name: format!("call {}", self.subscriber_function_id),
                    parent_id: None,
                    starts_after_id: Some("publish".to_string()),
                    attribute: None,
                },
            ],
        }
    }
}

pub async fn ensure_flow_test_tracing() -> FlowTracingGuard {
    let guard = FLOW_TRACING_LOCK.lock().await;

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

    FlowTracingGuard { _guard: guard }
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

fn validate_stream_event(
    stream_event: &Value,
    expected_stream_name: &str,
    expected_group_id: &str,
    expected_item_id: &str,
) -> anyhow::Result<()> {
    let actual_stream_name = stream_event
        .get("streamName")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("stream event is missing streamName"))?;
    let actual_group_id = stream_event
        .get("groupId")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("stream event is missing groupId"))?;
    let actual_item_id = stream_event
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("stream event is missing id"))?;

    if actual_stream_name != expected_stream_name {
        bail!(
            "stream event streamName mismatch: expected {}, got {}",
            expected_stream_name,
            actual_stream_name
        );
    }

    if actual_group_id != expected_group_id {
        bail!(
            "stream event groupId mismatch: expected {}, got {}",
            expected_group_id,
            actual_group_id
        );
    }

    if actual_item_id != expected_item_id {
        bail!(
            "stream event id mismatch: expected {}, got {}",
            expected_item_id,
            actual_item_id
        );
    }

    Ok(())
}

fn validate_pubsub_event(pubsub_event: &Value, expected_event: &Value) -> anyhow::Result<()> {
    if pubsub_event != expected_event {
        bail!(
            "pubsub event mismatch: expected {}, got {}",
            expected_event,
            pubsub_event
        );
    }

    Ok(())
}

fn validate_expected_spans<'a>(
    spans: &'a [StoredSpan],
    expected_spans: &[ExpectedFlowSpan],
) -> anyhow::Result<HashMap<String, &'a StoredSpan>> {
    let mut matched_spans: HashMap<String, &'a StoredSpan> = HashMap::new();

    for expected in expected_spans {
        let parent_span_id = match expected.parent_id.as_deref() {
            Some(parent_id) => Some(
                matched_spans
                    .get(parent_id)
                    .ok_or_else(|| {
                        anyhow!(
                            "span '{}' references unknown parent '{}'",
                            expected.id,
                            parent_id
                        )
                    })?
                    .span_id
                    .as_str(),
            ),
            None => None,
        };

        let min_start_time = match expected.starts_after_id.as_deref() {
            Some(after_id) => Some(
                matched_spans
                    .get(after_id)
                    .ok_or_else(|| {
                        anyhow!(
                            "span '{}' references unknown predecessor '{}'",
                            expected.id,
                            after_id
                        )
                    })?
                    .start_time_unix_nano,
            ),
            None => None,
        };

        let span = spans
            .iter()
            .filter(|span| {
                span.name == expected.name
                    && parent_span_id
                        .map(|span_id| span.parent_span_id.as_deref() == Some(span_id))
                        .unwrap_or(true)
                    && min_start_time
                        .map(|start_time| span.start_time_unix_nano >= start_time)
                        .unwrap_or(true)
                    && expected
                        .attribute
                        .as_ref()
                        .map(|attribute| has_attribute(span, &attribute.key, &attribute.value))
                        .unwrap_or(true)
            })
            .min_by_key(|span| span.start_time_unix_nano)
            .ok_or_else(|| missing_span_error(expected, parent_span_id, min_start_time))?;

        matched_spans.insert(expected.id.clone(), span);
    }

    Ok(matched_spans)
}

fn missing_span_error(
    expected: &ExpectedFlowSpan,
    parent_span_id: Option<&str>,
    min_start_time: Option<u64>,
) -> anyhow::Error {
    let attribute = expected
        .attribute
        .as_ref()
        .map(|attribute| format!(" with {}={}", attribute.key, attribute.value))
        .unwrap_or_default();
    let parent = parent_span_id
        .map(|span_id| format!(" under parent span {span_id}"))
        .unwrap_or_default();
    let timing = min_start_time
        .map(|start_time| format!(" after {start_time}"))
        .unwrap_or_default();

    anyhow!(
        "missing expected span '{}' named '{}'{}{}{}",
        expected.id,
        expected.name,
        attribute,
        parent,
        timing
    )
}

fn has_attribute(span: &StoredSpan, expected_key: &str, expected_value: &str) -> bool {
    span.attributes
        .iter()
        .any(|(key, value)| key == expected_key && value == expected_value)
}

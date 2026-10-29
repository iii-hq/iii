// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Engine-hosted sources: functions the engine hosts whose handler returns
//! the event they are called with, observed like any other function.
//!
//! | Function | Event (`input` and `output`) |
//! |---|---|
//! | `engine::on-functions-changed` | `{ event: "functions_changed", functions: [...] }`, one call per burst of registrations (~100ms) |
//! | `engine::on-workers-changed` | `{ event, worker_id }`, `event` one of `worker_connected`, `worker_disconnected`, `worker_metadata_updated` |
//! | `engine::on-observers-changed` | `{ observer_id, function_id, target, state, reason? }`, one call per observer state change |
//! | `configuration::on-updated` | a configuration entry registered, updated or deleted |
//! | `observability::on-log` | one ingested log record |
//! | `observability::on-span` | `{ trace_ids }`, one coalesced window of span activity |
//! | `stream::on-changed` | one stream write or custom event (`iii-stream`) |
//! | `stream::on-join`, `stream::on-leave` | a stream client joined or left a group (`iii-stream`) |
//!
//! Each is a private function: the engine calls it ([`ObserverRegistry::emit`])
//! once per occurrence, a successful call with the engine as its caller
//! (`{ worker_name: "iii-engine" }`), and evaluates its observers itself.
//! Nothing is built while nothing observes the function. Its `request_format`
//! and `response_format` describe the event (closed at the top level), so an
//! observer's `input.*` and `output.*` filter paths are checked. A call from
//! anyone else, the engine included, is refused with `FORBIDDEN`.

use schemars::JsonSchema;
use serde_json::{Value, json};

use super::{FunctionOptions, ObserverRegistry, lifecycle::Observer};
use crate::{
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    event_formats::{
        ConfigurationEvent, LogEvent, SpanEvent, StreamChangedEvent, StreamJoinLeaveEvent,
    },
    function::FunctionResult,
    protocol::{DEFAULT_NAMESPACE, ErrorBody, ObserverState},
};

pub const ON_FUNCTIONS_CHANGED: &str = "engine::on-functions-changed";
pub const ON_WORKERS_CHANGED: &str = "engine::on-workers-changed";
pub const ON_OBSERVERS_CHANGED: &str = "engine::on-observers-changed";
pub const CONFIGURATION_ON_UPDATED: &str = "configuration::on-updated";
pub const OBSERVABILITY_ON_LOG: &str = "observability::on-log";
pub const OBSERVABILITY_ON_SPAN: &str = "observability::on-span";
pub const STREAM_ON_CHANGED: &str = "stream::on-changed";
pub const STREAM_ON_JOIN: &str = "stream::on-join";
pub const STREAM_ON_LEAVE: &str = "stream::on-leave";

impl ObserverRegistry {
    /// One call of `engine::on-observers-changed`: `observer` became `state`.
    pub(super) fn emit_observer_changed(
        &self,
        observer: &Observer,
        state: &ObserverState,
        reason: Option<ErrorBody>,
    ) {
        let Some(registry) = self.this() else {
            return;
        };
        registry.emit(DEFAULT_NAMESPACE, ON_OBSERVERS_CHANGED, || {
            payload_built();
            let mut event = json!({
                "observer_id": observer.id,
                "function_id": observer.function.1,
                "target": observer.target.1,
                "state": state.as_str(),
            });
            if let Some(reason) = reason {
                event["reason"] = json!({ "code": reason.code, "message": reason.message });
            }
            Some(event)
        });
    }
}

#[cfg(test)]
thread_local! {
    static PAYLOADS_BUILT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Called where an engine-hosted source builds an event. Tests read the
/// count ([`payloads_built`]): unobserved, nothing should be built.
#[inline]
pub(crate) fn payload_built() {
    #[cfg(test)]
    PAYLOADS_BUILT.with(|built| built.set(built.get() + 1));
}

/// Event payloads built on this thread so far.
#[cfg(test)]
pub(crate) fn payloads_built() -> usize {
    PAYLOADS_BUILT.with(|built| built.get())
}

/// Registers the engine-hosted source `function_id` in `default`: a private
/// function whose handler returns its input, described by `format`.
pub fn register_event_source(engine: &Engine, function_id: &str, description: &str, format: Value) {
    engine
        .observers
        .set_options(DEFAULT_NAMESPACE, function_id, FunctionOptions::private());
    engine.register_function_handler(
        RegisterFunctionRequest {
            function_id: function_id.to_string(),
            description: Some(description.to_string()),
            request_format: Some(format.clone()),
            response_format: Some(format),
            metadata: Some(json!({ "source": "builtin", "observe": "event" })),
        },
        Handler::new(|event: Value| async move { FunctionResult::Success(Some(event)) }),
    );
}

/// `engine::on-functions-changed`, `engine::on-workers-changed` and
/// `engine::on-observers-changed`.
pub fn register_engine_event_sources(engine: &Engine) {
    register_event_source(
        engine,
        ON_FUNCTIONS_CHANGED,
        "Engine-hosted source: the set of registered functions changed (a burst is coalesced \
         over ~100ms). The event lists every function.",
        functions_changed_format(),
    );
    register_event_source(
        engine,
        ON_WORKERS_CHANGED,
        "Engine-hosted source: a worker connected (`worker_connected`), disconnected \
         (`worker_disconnected`) or announced its metadata with engine::workers::register \
         (`worker_metadata_updated`).",
        closed(json!({
            "type": "object",
            "required": ["event", "worker_id"],
            "properties": {
                "event": {
                    "type": "string",
                    "enum": ["worker_connected", "worker_disconnected", "worker_metadata_updated"]
                },
                "worker_id": { "type": "string" }
            }
        })),
    );
    register_event_source(
        engine,
        ON_OBSERVERS_CHANGED,
        "Engine-hosted source: an observer changed state. One call per state change; \
         `reason` says why, when there is one.",
        observers_changed_format(),
    );
}

/// `configuration::on-updated`.
pub fn register_configuration_event_source(engine: &Engine) {
    register_event_source(
        engine,
        CONFIGURATION_ON_UPDATED,
        "Engine-hosted source: a configuration entry was registered, updated or deleted \
         (`input.event_type`); `input.id` is the configuration id, usually the worker's name.",
        closed(schema_of::<ConfigurationEvent>()),
    );
}

/// `observability::on-log` and `observability::on-span`.
pub fn register_observability_event_sources(engine: &Engine) {
    register_event_source(
        engine,
        OBSERVABILITY_ON_LOG,
        "Engine-hosted source: one log record was ingested (filter on `input.severity_text`, \
         `input.service_name`, ...).",
        closed(schema_of::<LogEvent>()),
    );
    register_event_source(
        engine,
        OBSERVABILITY_ON_SPAN,
        "Engine-hosted source: spans were recorded; one call per ~300ms window lists the \
         distinct trace ids it touched (re-read them with engine::traces::*).",
        closed(schema_of::<SpanEvent>()),
    );
}

/// `stream::on-changed`, `stream::on-join` and `stream::on-leave`.
pub fn register_stream_event_sources(engine: &Engine) {
    register_event_source(
        engine,
        STREAM_ON_CHANGED,
        "Engine-hosted source: a stream item was created, updated or deleted, a group was \
         synced, or a custom event was sent to a group (filter on `input.stream_name`, \
         `input.group_id`).",
        closed(schema_of::<StreamChangedEvent>()),
    );
    register_event_source(
        engine,
        STREAM_ON_JOIN,
        "Engine-hosted source: a stream client subscribed to a group (filter on \
         `input.stream_name`, `input.group_id`).",
        closed(schema_of::<StreamJoinLeaveEvent>()),
    );
    register_event_source(
        engine,
        STREAM_ON_LEAVE,
        "Engine-hosted source: a stream client left a group (filter on `input.stream_name`, \
         `input.group_id`).",
        closed(schema_of::<StreamJoinLeaveEvent>()),
    );
}

fn functions_changed_format() -> Value {
    closed(json!({
        "type": "object",
        "required": ["event", "functions"],
        "properties": {
            "event": { "type": "string", "const": "functions_changed" },
            "functions": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["function_id", "namespace", "worker_name", "observer_count"],
                    "properties": {
                        "function_id": { "type": "string" },
                        "namespace": { "type": "string" },
                        "worker_name": { "type": "string" },
                        "description": { "type": "string" },
                        "metadata": {},
                        "args_format": {},
                        "private": { "type": "boolean" },
                        "observable": { "type": "boolean" },
                        "observer_count": { "type": "integer", "minimum": 0 }
                    }
                }
            }
        }
    }))
}

fn observers_changed_format() -> Value {
    closed(json!({
        "type": "object",
        "required": ["observer_id", "function_id", "target", "state"],
        "properties": {
            "observer_id": { "type": "string" },
            "function_id": { "type": "string" },
            "target": { "type": "string" },
            "state": {
                "type": "string",
                "enum": ["pending", "active", "suspended", "error", "disposed"]
            },
            "reason": {
                "type": "object",
                "required": ["code", "message"],
                "properties": {
                    "code": { "type": "string" },
                    "message": { "type": "string" }
                }
            }
        }
    }))
}

fn schema_of<T: JsonSchema>() -> Value {
    serde_json::to_value(schemars::schema_for!(T)).unwrap_or_else(|_| json!({ "type": "object" }))
}

/// Closes the top level of an event schema: a filter path naming a field the
/// event does not have is `INVALID_FILTER_PATH`, not a silent miss.
fn closed(mut schema: Value) -> Value {
    if let Value::Object(object) = &mut schema
        && object.contains_key("properties")
    {
        object.insert("additionalProperties".to_string(), Value::Bool(false));
    }
    schema
}

/// In-process observers of engine-hosted sources, for their own tests.
#[cfg(test)]
pub(crate) mod test_support {
    use std::{sync::Arc, time::Duration};

    use serde_json::Value;
    use tokio::sync::mpsc;

    use crate::{
        engine::{Engine, EngineTrait, Handler, Outbound, RegisterFunctionRequest},
        function::FunctionResult,
        observer::ObserveRequest,
        protocol::{Deliver, ObserveSource, ObserveTarget, ObserverState},
        worker_connections::WorkerConnection,
    };

    /// An in-process function that forwards every input it gets.
    pub fn capture(engine: &Engine, function_id: &str) -> mpsc::UnboundedReceiver<Value> {
        crate::workers::observability::metrics::ensure_default_meter();
        let (tx, rx) = mpsc::unbounded_channel();
        engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: function_id.to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(move |input: Value| {
                let tx = tx.clone();
                async move {
                    let _ = tx.send(input);
                    FunctionResult::Success(None)
                }
            }),
        );
        rx
    }

    /// A connected observing worker, and its socket (keep it alive).
    pub fn worker(engine: &Engine) -> (WorkerConnection, mpsc::Receiver<Outbound>) {
        // Activations record invocation metrics.
        crate::workers::observability::metrics::ensure_default_meter();
        let (tx, rx) = mpsc::channel(1024);
        let worker = WorkerConnection::new(tx);
        engine.worker_registry.register_worker(worker.clone());
        (worker, rx)
    }

    /// `source ~> target` with `deliver: input` (the call's input alone),
    /// defaults otherwise.
    pub fn request(id: &str, source: &str, target: &str, filters: Option<Value>) -> ObserveRequest {
        ObserveRequest {
            id: id.to_string(),
            source: ObserveSource {
                function_id: source.to_string(),
                namespace: None,
                args: None,
                filters,
                capture: None,
                on: None,
                deliver: Some(Deliver::Input),
            },
            target: ObserveTarget {
                function_id: target.to_string(),
                namespace: None,
            },
            metadata: None,
            max_fires: None,
            expires_in_ms: None,
            max_in_flight: None,
        }
    }

    /// Sends `request` from `worker` and waits until the observer is active.
    pub async fn observe_request(
        engine: &Arc<Engine>,
        worker: &WorkerConnection,
        request: ObserveRequest,
    ) {
        let id = request.id.clone();
        engine.observers.observe(engine, worker, request).await;
        tokio::time::timeout(Duration::from_secs(5), async {
            while engine.observers.get(&id).map(|o| o.state()) != Some(ObserverState::Active) {
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("`{id}` never became active"));
    }

    /// Observes `source` with `target` from a new observing worker and waits
    /// until it is active. Keep the returned receiver alive: it is the
    /// worker's socket.
    pub async fn observe(
        engine: &Arc<Engine>,
        id: &str,
        source: &str,
        target: &str,
        filters: Option<Value>,
    ) -> mpsc::Receiver<Outbound> {
        let (worker, rx) = worker(engine);
        observe_request(engine, &worker, request(id, source, target, filters)).await;
        rx
    }

    /// The next input `rx` gets.
    pub async fn next(rx: &mut mpsc::UnboundedReceiver<Value>) -> Value {
        tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("an activation")
            .expect("open")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Deliver;

    /// An engine-hosted source's events reach its observers (with the
    /// engine's depth), are not built while nothing observes it, and a direct
    /// call is refused and evaluated by nobody.
    #[tokio::test]
    async fn an_event_source_emits_only_what_is_observed() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        crate::workers::observability::metrics::ensure_default_meter();
        let engine = Arc::new(Engine::new());
        register_engine_event_sources(&engine);

        let built = Arc::new(AtomicUsize::new(0));
        let build = |n: usize| {
            let built = built.clone();
            move || {
                built.fetch_add(1, Ordering::SeqCst);
                Some(json!({ "event": "worker_metadata_updated", "worker_id": format!("w-{n}") }))
            }
        };
        engine
            .observers
            .emit(DEFAULT_NAMESPACE, ON_WORKERS_CHANGED, build(0));
        assert_eq!(built.load(Ordering::SeqCst), 0, "unobserved: nothing built");

        let mut sink = test_support::capture(&engine, "test::workers");
        let _socket = test_support::observe(
            &engine,
            "obs-workers",
            ON_WORKERS_CHANGED,
            "test::workers",
            Some(json!({ "input.worker_id": { "$ne": "w-skip" } })),
        )
        .await;
        let evaluated = engine.observers.evaluated();
        engine
            .observers
            .emit(DEFAULT_NAMESPACE, ON_WORKERS_CHANGED, build(1));
        assert_eq!(
            test_support::next(&mut sink).await,
            json!({ "event": "worker_metadata_updated", "worker_id": "w-1" })
        );

        let refused = engine.call(ON_WORKERS_CHANGED, json!({})).await;
        assert_eq!(refused.unwrap_err().code, "FORBIDDEN");
        assert_eq!(
            engine.observers.evaluated(),
            evaluated + 1,
            "the direct call is not one of its events"
        );
        assert!(
            engine
                .observers
                .options(DEFAULT_NAMESPACE, ON_WORKERS_CHANGED)
                .private
        );
    }

    /// A source's handler returns its event: observers see it as `input` and
    /// as `output`, with the engine as the caller.
    #[tokio::test]
    async fn an_event_is_the_input_and_the_output_and_the_engine_calls() {
        let engine = std::sync::Arc::new(Engine::new());
        register_engine_event_sources(&engine);
        let mut sink = test_support::capture(&engine, "test::workers");
        let (worker, _socket) = test_support::worker(&engine);
        let mut request = test_support::request(
            "obs-envelope",
            ON_WORKERS_CHANGED,
            "test::workers",
            Some(json!({ "output.event": "worker_connected" })),
        );
        request.source.deliver = Some(Deliver::Envelope);
        test_support::observe_request(&engine, &worker, request).await;

        let event = json!({ "event": "worker_connected", "worker_id": "w-1" });
        engine
            .observers
            .emit(DEFAULT_NAMESPACE, ON_WORKERS_CHANGED, || {
                Some(event.clone())
            });
        let envelope = test_support::next(&mut sink).await;
        assert_eq!(envelope["input"], event);
        assert_eq!(envelope["output"], event);
        assert_eq!(envelope["caller"], json!({ "worker_name": "iii-engine" }));
    }

    /// Every observer state change is one call of
    /// `engine::on-observers-changed`.
    #[tokio::test]
    async fn observer_state_changes_are_an_engine_hosted_source() {
        let engine = std::sync::Arc::new(Engine::new());
        register_engine_event_sources(&engine);
        let mut changes = test_support::capture(&engine, "test::changes");
        let _changes_socket = test_support::observe(
            &engine,
            "obs-changes",
            ON_OBSERVERS_CHANGED,
            "test::changes",
            Some(json!({ "input.observer_id": "watched" })),
        )
        .await;
        let _source = test_support::capture(&engine, "test::source");
        let (worker, _socket) = test_support::worker(&engine);
        engine
            .observers
            .observe(
                &engine,
                &worker,
                test_support::request("watched", "test::source", "test::target", None),
            )
            .await;
        // Created pending (its target is missing): a creation is no change.
        let _target = test_support::capture(&engine, "test::target");
        assert_eq!(
            test_support::next(&mut changes).await,
            json!({
                "observer_id": "watched",
                "function_id": "test::source",
                "target": "test::target",
                "state": "active"
            })
        );

        let observer = engine.observers.get("watched").unwrap();
        engine
            .observers
            .dispose(&observer, "UNOBSERVED", "disposed by the test");
        let disposed = test_support::next(&mut changes).await;
        assert_eq!(disposed["state"], "disposed");
        assert_eq!(disposed["reason"]["code"], "UNOBSERVED");
    }

    #[test]
    fn event_formats_are_closed_and_name_their_fields() {
        for (schema, field) in [
            (functions_changed_format(), "functions"),
            (observers_changed_format(), "observer_id"),
            (closed(schema_of::<ConfigurationEvent>()), "event_type"),
            (closed(schema_of::<LogEvent>()), "severity_text"),
            (closed(schema_of::<SpanEvent>()), "trace_ids"),
            (closed(schema_of::<StreamChangedEvent>()), "stream_name"),
            (closed(schema_of::<StreamJoinLeaveEvent>()), "group_id"),
        ] {
            assert_eq!(schema["additionalProperties"], false, "{schema}");
            assert!(schema["properties"].get(field).is_some(), "{schema}");
        }
    }
}

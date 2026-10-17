// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Engine-hosted event sources (README §9, protocol.md §3.2): the functions
//! that replace the engine's own trigger types and those of the engine
//! workers still in use.
//!
//! | Function | Replaces the trigger type | Event (`input`) |
//! |---|---|---|
//! | `engine::on-functions-changed` | `engine::functions-available` | `{ event: "functions_changed", functions: [...] }` |
//! | `engine::on-workers-changed` | `engine::workers-available` | `{ event, worker_id }`, `event` one of `worker_connected`, `worker_disconnected`, `worker_metadata_updated` |
//! | `configuration::on-updated` | `configuration` | the `configuration` trigger's event |
//! | `observability::on-log` | `log` | one log record, as the `log` trigger sends it |
//! | `observability::on-span` | `trace` | `{ trace_ids }`, one coalesced window, as the `trace` trigger sends it |
//!
//! Each is an auto function observed like any other. Its events are
//! evaluated by the engine itself ([`ObserverRegistry::emit`]) wherever the
//! old trigger type fires, which keeps working: one event per occurrence, a
//! successful invocation whose input is the event and whose output is empty,
//! with the engine as the caller. Nothing is built while nothing observes the
//! function. Its `request_format` describes the event (closed at the top
//! level), so an observer's `input.*` filter paths are checked. Calling one
//! directly is refused with `FORBIDDEN`: its events are the engine's to emit,
//! and such a call is not one of them.

use schemars::JsonSchema;
use serde_json::{Value, json};

use super::ObserverRegistry;
use crate::{
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    function::FunctionResult,
    protocol::{DEFAULT_NAMESPACE, ErrorBody, observe_codes},
    trigger_formats::{ConfigurationCallRequest, LogCallRequest, TraceCallRequest},
};

/// Replaces the `engine::functions-available` trigger type.
pub const ON_FUNCTIONS_CHANGED: &str = "engine::on-functions-changed";
/// Replaces the `engine::workers-available` trigger type.
pub const ON_WORKERS_CHANGED: &str = "engine::on-workers-changed";
/// Replaces the `configuration` trigger type.
pub const CONFIGURATION_ON_UPDATED: &str = "configuration::on-updated";
/// Replaces the `log` trigger type.
pub const OBSERVABILITY_ON_LOG: &str = "observability::on-log";
/// Replaces the `trace` trigger type.
pub const OBSERVABILITY_ON_SPAN: &str = "observability::on-span";

impl ObserverRegistry {
    /// Marks `(namespace, function_id)` as an event source: the routed hook
    /// ignores calls to it, and [`Self::emit`] produces its events.
    pub fn declare_event_source(&self, namespace: &str, function_id: &str) {
        let key = (namespace.to_string(), function_id.to_string());
        if self.event_sources.insert(key.clone(), ()).is_none() {
            self.refresh_active(&key);
        }
    }
}

#[cfg(test)]
thread_local! {
    static PAYLOADS_BUILT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Called where an event source builds an event's payload. Tests read the
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

/// Registers the engine-hosted event source `function_id` in `default`.
pub fn register_event_source(
    engine: &Engine,
    function_id: &str,
    description: &str,
    request_format: Value,
) {
    engine
        .observers
        .declare_event_source(DEFAULT_NAMESPACE, function_id);
    let refusal = format!(
        "`{function_id}` is an event source the engine emits: observe it instead of calling it"
    );
    engine.register_function_handler(
        RegisterFunctionRequest {
            function_id: function_id.to_string(),
            description: Some(description.to_string()),
            request_format: Some(request_format),
            response_format: None,
            metadata: Some(json!({ "source": "builtin", "observe": "event" })),
        },
        Handler::new(move |_input: Value| {
            let refusal = refusal.clone();
            async move { FunctionResult::Failure(ErrorBody::new(observe_codes::FORBIDDEN, refusal)) }
        }),
    );
}

/// `engine::on-functions-changed` and `engine::on-workers-changed`.
pub fn register_engine_event_sources(engine: &Engine) {
    register_event_source(
        engine,
        ON_FUNCTIONS_CHANGED,
        "Event source: the set of registered functions changed (a burst is coalesced over \
         ~100ms). The event lists every function. Replaces the engine::functions-available \
         trigger type.",
        functions_changed_format(),
    );
    register_event_source(
        engine,
        ON_WORKERS_CHANGED,
        "Event source: a worker connected (`worker_connected`), disconnected \
         (`worker_disconnected`) or announced its metadata with engine::workers::register \
         (`worker_metadata_updated`). Replaces the engine::workers-available trigger type.",
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
}

/// `configuration::on-updated`.
pub fn register_configuration_event_source(engine: &Engine) {
    register_event_source(
        engine,
        CONFIGURATION_ON_UPDATED,
        "Event source: a configuration entry was registered, updated or deleted \
         (`input.event_type`); `input.id` is the configuration id, usually the worker's name. \
         Replaces the configuration trigger type.",
        closed(schema_of::<ConfigurationCallRequest>()),
    );
}

/// `observability::on-log` and `observability::on-span`.
pub fn register_observability_event_sources(engine: &Engine) {
    register_event_source(
        engine,
        OBSERVABILITY_ON_LOG,
        "Event source: one log record was ingested (filter on `input.severity_text`, \
         `input.service_name`, ...). Replaces the log trigger type.",
        closed(schema_of::<LogCallRequest>()),
    );
    register_event_source(
        engine,
        OBSERVABILITY_ON_SPAN,
        "Event source: spans were recorded; one event per ~300ms window lists the distinct \
         trace ids it touched (re-read them with engine::traces::*). Replaces the trace \
         trigger type.",
        closed(schema_of::<TraceCallRequest>()),
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
                    "required": ["function_id", "namespace", "worker_name"],
                    "properties": {
                        "function_id": { "type": "string" },
                        "namespace": { "type": "string" },
                        "worker_name": { "type": "string" },
                        "description": { "type": "string" },
                        "metadata": {}
                    }
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

/// In-process observers of event sources, for the event sources' own tests.
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

    /// `source ~> target` with `deliver: input` (like a trigger), defaults
    /// otherwise.
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

    /// An event source's events reach its observers (with the engine's
    /// depth), are not built while nothing observes it, and a direct call is
    /// refused and evaluated by nobody.
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
        engine
            .observers
            .emit(DEFAULT_NAMESPACE, ON_WORKERS_CHANGED, build(1));
        assert_eq!(
            test_support::next(&mut sink).await,
            json!({ "event": "worker_metadata_updated", "worker_id": "w-1" })
        );

        let refused = engine.call(ON_WORKERS_CHANGED, json!({})).await;
        assert_eq!(refused.unwrap_err().code, "FORBIDDEN");
        let evaluated = engine.observers.evaluated();
        assert_eq!(evaluated, 1, "the direct call is not one of its events");
    }

    #[test]
    fn event_formats_are_closed_and_name_their_fields() {
        for (schema, field) in [
            (functions_changed_format(), "functions"),
            (
                closed(schema_of::<ConfigurationCallRequest>()),
                "event_type",
            ),
            (closed(schema_of::<LogCallRequest>()), "severity_text"),
            (closed(schema_of::<TraceCallRequest>()), "trace_ids"),
        ] {
            assert_eq!(schema["additionalProperties"], false, "{schema}");
            assert!(schema["properties"].get(field).is_some(), "{schema}");
        }
    }
}

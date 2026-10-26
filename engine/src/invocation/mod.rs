// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use std::sync::Arc;

use dashmap::DashMap;
use opentelemetry::KeyValue;
use serde_json::Value;
use tokio::sync::oneshot::{self, error::RecvError};
use tracing::Instrument;
use tracing_opentelemetry::OpenTelemetrySpanExt;
use uuid::Uuid;

use crate::telemetry::SpanExt;

use crate::{
    function::{Function, FunctionResult, HandlerFn},
    observer::{InvocationCtx, ObserverRegistry},
    protocol::ErrorBody,
    worker_connections::WorkerConnection,
    workers::observability::metrics::get_engine_metrics,
    workers::worker::rbac_session::Session,
};

pub mod auth;
pub mod http_function;
pub mod http_invoker;
pub mod method;
pub mod signature;
pub mod url_validator;

// Keep the handler snapshot until its callback returns, and transfer the payload
// to it even when dispatch subsequently waits for a deferred worker response.
async fn call_handler_with_metadata<H: AsRef<HandlerFn> + Send>(
    handler: H,
    invocation_id: Option<Uuid>,
    data: Value,
    session: Option<Arc<Session>>,
    metadata: Option<Value>,
) -> FunctionResult<Option<Value>, ErrorBody> {
    (handler.as_ref())(invocation_id, data, session, metadata).await
}

pub struct Invocation {
    pub id: Uuid,
    pub function_id: String,
    pub worker_id: Option<Uuid>,
    pub sender: oneshot::Sender<Result<Option<Value>, ErrorBody>>,
    /// W3C traceparent for distributed tracing context
    pub traceparent: Option<String>,
    /// W3C baggage for cross-cutting context propagation
    pub baggage: Option<String>,
}

type Invocations = Arc<DashMap<Uuid, Invocation>>;

// Remove unfinished registrations when dispatch fails or its future is dropped.
struct InvocationRegistration<'a> {
    invocations: &'a Invocations,
    id: Uuid,
}

impl Drop for InvocationRegistration<'_> {
    fn drop(&mut self) {
        self.invocations.remove(&self.id);
    }
}

#[derive(Default)]
pub struct InvocationHandler {
    invocations: Invocations,
    /// Consulted on every invocation (the routed observer hook). Set once by
    /// `Engine::new`; a handler without it observes nothing.
    observers: std::sync::OnceLock<Arc<ObserverRegistry>>,
}
impl InvocationHandler {
    pub fn new() -> Self {
        Self {
            invocations: Arc::new(DashMap::new()),
            observers: std::sync::OnceLock::new(),
        }
    }

    pub(crate) fn set_observers(&self, observers: Arc<ObserverRegistry>) {
        let _ = self.observers.set(observers);
    }

    pub fn remove(&self, invocation_id: &Uuid) -> Option<Invocation> {
        self.invocations
            .remove(invocation_id)
            .map(|(_, sender)| sender)
    }

    pub fn halt_invocation(&self, invocation_id: &Uuid) {
        let invocation = self.remove(invocation_id);

        if let Some(invocation) = invocation {
            let _ = invocation.sender.send(Err(ErrorBody {
                code: "invocation_stopped".into(),
                message: "Invocation stopped".into(),
                stacktrace: None,
            }));
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn handle_invocation(
        &self,
        invocation_id: Option<Uuid>,
        worker_id: Option<Uuid>,
        function_id: String,
        body: Value,
        function_handler: Function,
        traceparent: Option<String>,
        baggage: Option<String>,
        session: Option<Arc<Session>>,
        metadata: Option<Value>,
    ) -> Result<Result<Option<Value>, ErrorBody>, RecvError> {
        self.handle_invocation_inner(
            invocation_id,
            worker_id,
            None,
            function_id,
            crate::protocol::DEFAULT_NAMESPACE,
            body,
            function_handler,
            traceparent,
            baggage,
            session,
            metadata,
            InvocationCtx::engine_internal(),
        )
        .await
    }

    /// `namespace` is where `function_handler` was resolved; `ctx` carries
    /// the caller, observe depth and activation the call arrived with.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn handle_invocation_with_handler(
        &self,
        invocation_id: Option<Uuid>,
        caller: Option<&WorkerConnection>,
        function_id: String,
        namespace: &str,
        body: Value,
        function_handler: Arc<HandlerFn>,
        traceparent: Option<String>,
        baggage: Option<String>,
        session: Option<Arc<Session>>,
        metadata: Option<Value>,
        ctx: InvocationCtx,
    ) -> Result<Result<Option<Value>, ErrorBody>, RecvError> {
        self.handle_invocation_inner(
            invocation_id,
            caller.map(|worker| worker.id),
            caller.filter(|_| invocation_id.is_some()),
            function_id,
            namespace,
            body,
            function_handler,
            traceparent,
            baggage,
            session,
            metadata,
            ctx,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn handle_invocation_inner<H: AsRef<HandlerFn> + Send>(
        &self,
        invocation_id: Option<Uuid>,
        worker_id: Option<Uuid>,
        caller: Option<&WorkerConnection>,
        function_id: String,
        namespace: &str,
        body: Value,
        function_handler: H,
        traceparent: Option<String>,
        baggage: Option<String>,
        session: Option<Arc<Session>>,
        metadata: Option<Value>,
        ctx: InvocationCtx,
    ) -> Result<Result<Option<Value>, ErrorBody>, RecvError> {
        // The routed observer hook (protocol.md §3.2). Unobserved — the usual
        // case — it costs one atomic load and copies nothing; observed, it
        // keeps only what the active observers need.
        let observers = self.observers.get();
        // An owner-delivered function is invoked by its owner only.
        if let Some(registry) = observers
            && let Err(refused) = registry.may_invoke(namespace, &function_id, worker_id)
        {
            return Ok(Err(refused));
        }
        let observed = observers.and_then(|registry| {
            registry
                .watch_routed(namespace, &function_id)
                .map(|set| registry.begin(set, &body, &metadata, &traceparent, &baggage, &ctx))
        });
        // The context goes in scope around the handler (and is kept for the
        // hook) unless there is nothing in it: engine orchestration at depth 0,
        // with no context around it already, is the same as none.
        let ctx: Option<Arc<InvocationCtx>> =
            (observed.is_some() || !ctx.is_blank() || InvocationCtx::current().is_some())
                .then(|| Arc::new(ctx));

        // Create span with dynamic name using the function_id
        // Using OTEL semantic conventions for FaaS (Function as a Service)
        let is_builtin = crate::workers::telemetry::is_iii_builtin_function_id(&function_id);

        // Decide whether the ENGINE emits its own `call <fn>` span here
        // (policy in `telemetry::should_suppress_invocation_span`):
        //   - Observability functions (`engine::traces::*`, `engine::logs::*`,
        //     …) are NEVER traced — the pipeline must not observe itself.
        //   - Built-in functions (`state::*`, `stream::*`, `engine::*`,
        //     `configuration::*`, `iii::*`, …) execute in-process, so this
        //     span is the only possible record of the call. Emitted when the
        //     caller supplied a `traceparent` — the call then nests inside
        //     the caller's trace (and carries error status when it fails).
        //     Context-free built-in calls (console RPC polling, boot reads,
        //     engine machinery) stay suppressed so they never root new
        //     single-span traces; `III_OTEL_TRACE_BUILTINS=true` forces them.
        //   - Worker-routed (non-builtin) functions are executed by an external
        //     worker that emits its own handler span (`execute <fn>`, INTERNAL,
        //     service = that worker). An engine-side `call` span would wrap the
        //     same logical invocation as a cross-service duplicate, so suppress
        //     it and let the worker's span be the canonical one.
        let suppress_span = crate::workers::telemetry::should_suppress_invocation_span(
            &function_id,
            traceparent.is_some(),
            crate::workers::telemetry::trace_builtins_enabled(),
        );

        let span = if suppress_span {
            tracing::Span::none()
        } else {
            tracing::info_span!(
                "call",
                otel.name = %format!("call {}", function_id),
                otel.kind = "server",
                otel.status_code = tracing::field::Empty,
                // FAAS semantic conventions (https://opentelemetry.io/docs/specs/semconv/faas/)
                "faas.invoked_name" = %function_id,
                "faas.trigger" = "other",  // III Engine uses its own invocation mechanism
                // Keep function_id for backward compatibility
                function_id = %function_id,
                // Tag internal vs user functions for filtering
                "iii.function.kind" = if is_builtin { "internal" } else { "user" },
                // Set below when the call sits in a chain of observers: an
                // `observability::on-span` event takes it (loop guard).
                "iii.observe_depth" = tracing::field::Empty,
            )
            .with_parent_headers(traceparent.as_deref(), None, baggage.as_deref())
        };
        let depth = ctx.as_ref().map_or(0, |ctx| ctx.observe_depth);
        if depth > 0 {
            span.record(
                crate::workers::observability::otel::OBSERVE_DEPTH_ATTRIBUTE,
                depth,
            );
        }

        // Run the dispatch under the caller's OTel context whenever one was
        // provided, for BOTH worker-routed and built-in calls:
        //   - Worker-routed: the engine span is suppressed, so there is no
        //     tracing span left to carry the caller's context across the WS
        //     boundary. Attaching it lets the worker's own span (and its
        //     descendants) nest under the caller's trace instead of orphaning
        //     into a new one. `WorkerConnection::handle_function` falls back to
        //     this ambient context (see `worker_connections/traits.rs`).
        //   - Built-in: state/stream writes fire triggers in a spawned
        //     `*_triggers` span. Without the caller context that span (and the
        //     trigger handlers it invokes) would root a brand-new, disconnected
        //     trace; attaching it nests the fan-out under the writer (e.g.
        //     `approval::resolve` → state write → `turn::on_approval`).
        let dispatch_cx = if traceparent.is_some() || baggage.is_some() {
            let cx = crate::telemetry::extract_context(traceparent.as_deref(), baggage.as_deref());
            // The caller's baggage names the CALLER; rewrite `iii.function.id`
            // to the function being invoked so worker-side spans are attributed
            // to their owning function (mirrors the enqueue boundary in
            // `engine/mod.rs` and the `fn_queue` consumer in `queue.rs`).
            // Built-ins execute in-process under the caller's identity and are
            // left untouched.
            Some(if is_builtin {
                cx
            } else {
                crate::telemetry::with_function_id_baggage(&cx, &function_id)
            })
        } else {
            None
        };

        let invocation_fut = async {
            let invocation_id = invocation_id.unwrap_or_else(Uuid::new_v4);
            let (sender, receiver) = oneshot::channel();
            // A worker can reply or disconnect before its handler returns Deferred.
            self.invocations.insert(invocation_id, Invocation {
                id: invocation_id,
                function_id: function_id.clone(),
                worker_id,
                sender,
                traceparent,
                baggage,
            });
            let _registration = InvocationRegistration {
                invocations: &self.invocations,
                id: invocation_id,
            };

            if let Some(caller) = caller {
                let mut invocations = caller.invocations.write().await;
                // Cleanup sets this flag before taking the same lock to drain IDs.
                if caller.cleanup_claimed.load(std::sync::atomic::Ordering::SeqCst) {
                    return Ok(Err(ErrorBody {
                        code: "invocation_stopped".into(),
                        message: "Invocation stopped".into(),
                        stacktrace: None,
                    }));
                }
                invocations.insert(invocation_id);
            }

            // Start timer for invocation duration
            let start_time = std::time::Instant::now();
            let metrics = get_engine_metrics();

            // In scope while the handler runs, so a worker-hosted function's
            // `InvokeFunction` frame carries this call's caller, depth and
            // activation (`WorkerConnection::handle_function`).
            let handler = call_handler_with_metadata(
                function_handler,
                Some(invocation_id),
                body,
                session,
                metadata,
            );
            let result = match &ctx {
                Some(ctx) => InvocationCtx::scope(ctx.clone(), handler).await,
                None => handler.await,
            };

            // Calculate duration
            let duration = start_time.elapsed().as_secs_f64();

            let result = match result {
                FunctionResult::Success(result) => {
                    tracing::debug!(invocation_id = %invocation_id, function_id = %function_id, "Function completed successfully");
                    tracing::Span::current().record("otel.status_code", "OK");

                    // Record metrics
                    let attributes = [
                        KeyValue::new("function_id", function_id.clone()),
                        KeyValue::new("status", "ok"),
                    ];
                    metrics.invocations_total.add(1, &attributes);
                    metrics.invocation_duration.record(duration, &attributes);

                    // Update accumulator for readable metrics
                    let acc = crate::workers::observability::metrics::get_metrics_accumulator();
                    acc.invocations_total.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    acc.invocations_success.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    acc.increment_function(&function_id);
                    if !crate::workers::telemetry::is_iii_builtin_function_id(&function_id) {
                        let _ = acc.first_user_success_fn.set(function_id.clone());
                    }

                    Ok(Ok(result))
                }
                FunctionResult::Failure(error) => {
                    tracing::debug!(invocation_id = %invocation_id, function_id = %function_id, error_code = %error.code, "Function failed: {}", error.message);
                    let current_span = tracing::Span::current();
                    current_span.record("otel.status_code", "ERROR");
                    // Attach the error onto the span so the trace UI can render
                    // detail — a bare ERROR status carries no message. The
                    // console's error tab reads `error.message`/`error.type`
                    // attributes and an `exception` event. No-op when the engine
                    // span is suppressed for a worker-routed call (that worker's
                    // own span carries the failure instead).
                    current_span.set_attribute("error.message", error.message.clone());
                    current_span.set_attribute("error.type", error.code.clone());
                    let mut exception_attrs = vec![
                        KeyValue::new("exception.message", error.message.clone()),
                        KeyValue::new("exception.type", error.code.clone()),
                    ];
                    if let Some(stacktrace) = error.stacktrace.clone() {
                        current_span.set_attribute("error.stack", stacktrace.clone());
                        exception_attrs.push(KeyValue::new("exception.stacktrace", stacktrace));
                    }
                    current_span.add_event("exception", exception_attrs);

                    // Record metrics
                    let mut attributes = [
                        KeyValue::new("function_id", function_id.clone()),
                        KeyValue::new("status", "error"),
                    ];
                    metrics.invocations_total.add(1, &attributes);
                    metrics.invocation_duration.record(duration, &attributes);
                    attributes[1] = KeyValue::new("error_code", error.code.clone());
                    metrics.invocation_errors_total.add(1, &attributes);

                    // Update accumulator for readable metrics
                    let acc = crate::workers::observability::metrics::get_metrics_accumulator();
                    acc.invocations_total.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    acc.invocations_error.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    acc.increment_function(&function_id);
                    if !crate::workers::telemetry::is_iii_builtin_function_id(&function_id) {
                        let _ = acc.first_user_failure_fn.set(function_id.clone());
                    }

                    Ok(Err(error))
                }
                FunctionResult::NoResult => {
                    tracing::debug!(invocation_id = %invocation_id, function_id = %function_id, "Function no result");
                    tracing::Span::current().record("otel.status_code", "OK");

                    // Record metrics
                    let attributes = [
                        KeyValue::new("function_id", function_id.clone()),
                        KeyValue::new("status", "ok"),
                    ];
                    metrics.invocations_total.add(1, &attributes);
                    metrics.invocation_duration.record(duration, &attributes);

                    // Update accumulator for readable metrics
                    let acc = crate::workers::observability::metrics::get_metrics_accumulator();
                    acc.invocations_total.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    acc.invocations_success.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    acc.increment_function(&function_id);

                    Ok(Ok(None))
                }
                FunctionResult::Deferred => {
                    tracing::debug!(invocation_id = %invocation_id, function_id = %function_id, "Function deferred");

                    // Drop metric attributes before waiting for the worker response.
                    {
                        let attributes = [
                            KeyValue::new("function_id", function_id.clone()),
                            KeyValue::new("status", "deferred"),
                        ];
                        metrics.invocations_total.add(1, &attributes);
                        metrics.invocation_duration.record(duration, &attributes);
                    }

                    // Update accumulator for readable metrics
                    let acc = crate::workers::observability::metrics::get_metrics_accumulator();
                    acc.invocations_total.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    acc.invocations_deferred.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    acc.increment_function(&function_id);

                    receiver.await
                }
            };
            match &result {
                Ok(Ok(_)) => {
                    tracing::Span::current().record("otel.status_code", "OK");
                }
                Ok(Err(_)) | Err(_) => {
                    tracing::Span::current().record("otel.status_code", "ERROR");
                }
            };
            if let (Some(registry), Some(observed), Some(ctx)) = (observers, observed, &ctx) {
                registry.complete(observed, namespace, &function_id, invocation_id, &result, ctx);
            }
            result
        }
        .instrument(span);

        match dispatch_cx {
            // Engine span suppressed for a worker-routed call: run the dispatch
            // under the caller's OTel context so the worker's span nests under
            // the caller's trace instead of orphaning into a new one.
            Some(cx) => {
                use opentelemetry::trace::FutureExt as _;
                invocation_fut.with_context(cx).await
            }
            None => invocation_fut.await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The engine's span of a call made inside a chain of observers records
    /// the depth (`iii.observe_depth`), and the stored span carries it: what
    /// `observability::on-span` takes for its loop guard.
    #[tokio::test]
    async fn an_invocation_span_records_its_observe_depth() {
        use opentelemetry::trace::TracerProvider as _;
        use tracing_subscriber::prelude::*;

        crate::workers::observability::metrics::ensure_default_meter();
        let storage = Arc::new(crate::workers::observability::otel::InMemorySpanStorage::new(100));
        let provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
            .with_simple_exporter(
                crate::workers::observability::otel::InMemorySpanExporter::with_storage(
                    storage.clone(),
                    "engine".to_string(),
                ),
            )
            .build();
        let subscriber = tracing_subscriber::registry().with(
            tracing_opentelemetry::OpenTelemetryLayer::new(provider.tracer("test")),
        );
        let _default = tracing::subscriber::set_default(subscriber);

        let handler = InvocationHandler::new();
        let function: Arc<HandlerFn> =
            Arc::new(|_, data, _, _| Box::pin(async move { FunctionResult::Success(Some(data)) }));
        // A built-in call is traced when it carries the caller's context.
        let traceparent = "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01";
        for (function_id, depth) in [("iii::depth-probe", 2), ("iii::flat-probe", 0)] {
            let result = handler
                .handle_invocation_with_handler(
                    Some(Uuid::new_v4()),
                    None,
                    function_id.to_string(),
                    "default",
                    serde_json::json!({}),
                    function.clone(),
                    Some(traceparent.to_string()),
                    None,
                    None,
                    None,
                    InvocationCtx::engine_internal_at(depth),
                )
                .await;
            assert!(matches!(result, Ok(Ok(_))));
        }
        let depth_of = |name: &str| {
            storage
                .get_spans()
                .into_iter()
                .find(|span| span.name == name)
                .map(|span| span.observe_depth)
        };
        assert_eq!(depth_of("call iii::depth-probe"), Some(2));
        assert_eq!(depth_of("call iii::flat-probe"), Some(0));
    }

    #[test]
    fn test_invocation_handler_new() {
        let handler = InvocationHandler::new();
        assert!(handler.invocations.is_empty());
    }

    #[test]
    fn test_invocation_handler_default() {
        let handler = InvocationHandler::default();
        assert!(handler.invocations.is_empty());
    }

    #[test]
    fn test_invocation_handler_remove_nonexistent() {
        let handler = InvocationHandler::new();
        let id = Uuid::new_v4();
        let result = handler.remove(&id);
        assert!(result.is_none());
    }

    #[test]
    fn test_invocation_handler_remove_existing() {
        let handler = InvocationHandler::new();
        let id = Uuid::new_v4();
        let (sender, _receiver) = oneshot::channel();

        let invocation = Invocation {
            id,
            function_id: "test_fn".to_string(),
            worker_id: None,
            sender,
            traceparent: None,
            baggage: None,
        };
        handler.invocations.insert(id, invocation);

        let removed = handler.remove(&id);
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().id, id);
        assert!(handler.invocations.is_empty());
    }

    #[test]
    fn test_invocation_handler_halt_invocation_sends_error() {
        let handler = InvocationHandler::new();
        let id = Uuid::new_v4();
        let (sender, mut receiver) = oneshot::channel();

        let invocation = Invocation {
            id,
            function_id: "test_fn".to_string(),
            worker_id: None,
            sender,
            traceparent: None,
            baggage: None,
        };
        handler.invocations.insert(id, invocation);

        handler.halt_invocation(&id);

        // The invocation should have been removed.
        assert!(handler.invocations.is_empty());

        // The receiver should get an error result.
        let result = receiver.try_recv();
        assert!(result.is_ok());
        let inner = result.unwrap();
        assert!(inner.is_err());
        let error = inner.unwrap_err();
        assert_eq!(error.code, "invocation_stopped");
        assert_eq!(error.message, "Invocation stopped");
    }

    #[test]
    fn test_invocation_handler_halt_nonexistent_is_noop() {
        let handler = InvocationHandler::new();
        let id = Uuid::new_v4();
        // Should not panic.
        handler.halt_invocation(&id);
    }

    #[test]
    fn dispatch_baggage_rewrite_names_the_target_function() {
        // `handle_invocation` rewrites `iii.function.id` in the dispatch
        // context for non-builtin targets (extract → rewrite → inject is the
        // exact path the worker-bound baggage header takes), so worker-side
        // spans are attributed to the function they serve — not the caller.
        let caller = "iii.session.id=s-1,iii.function.id=parent::fn";
        let cx = crate::telemetry::extract_context(None, Some(caller));
        let rewritten = crate::telemetry::with_function_id_baggage(&cx, "child::fn");
        let header =
            crate::telemetry::inject_baggage_from_context(&rewritten).expect("baggage header");
        assert!(header.contains("iii.function.id=child::fn"));
        assert!(!header.contains("iii.function.id=parent::fn"));
        assert!(
            header.contains("iii.session.id=s-1"),
            "other entries preserved"
        );
    }

    #[test]
    fn test_invocation_handler_multiple_invocations() {
        let handler = InvocationHandler::new();
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let (sender1, _rx1) = oneshot::channel();
        let (sender2, _rx2) = oneshot::channel();

        handler.invocations.insert(
            id1,
            Invocation {
                id: id1,
                function_id: "fn1".to_string(),
                worker_id: None,
                sender: sender1,
                traceparent: None,
                baggage: None,
            },
        );
        handler.invocations.insert(
            id2,
            Invocation {
                id: id2,
                function_id: "fn2".to_string(),
                worker_id: Some(Uuid::new_v4()),
                sender: sender2,
                traceparent: Some("00-trace-id".to_string()),
                baggage: Some("key=value".to_string()),
            },
        );

        assert_eq!(handler.invocations.len(), 2);

        let removed = handler.remove(&id1);
        assert!(removed.is_some());
        assert_eq!(handler.invocations.len(), 1);

        let removed = handler.remove(&id2);
        assert!(removed.is_some());
        assert!(handler.invocations.is_empty());
    }
}

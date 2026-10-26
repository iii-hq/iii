//! Running handlers.
//!
//! Both paths go through [`execute`]: the remote one (an `invokefunction`
//! from the engine) and the local one (a [`trigger`](crate::IIIClient::trigger)
//! of a function this worker hosts, once the engine confirmed it). So both
//! create the same `execute <fn>` span, set the same current invocation and
//! produce the same error shape.
//!
//! A local call of an auto function evaluates its observers' compiled filters
//! after the handler returns and reports only the matches. With nobody
//! observing it costs one atomic load and puts nothing on the wire.

use std::{
    future::Future,
    panic::AssertUnwindSafe,
    sync::Arc,
    time::{Duration, Instant},
};

use futures_util::FutureExt;
use iii_filter::Doc;
use iii_helpers::observability::opentelemetry::Context as OtelContext;
use serde_json::{Value, json, value::to_raw_value};

use crate::{
    context::{Frame, InvocationCtx, local_depth, scope_frame, with_current},
    error::Error,
    iii::{
        DEFAULT_TIMEOUT_MS, IIIClient, PreparedEnvelope, inject_trace_headers, now_ms, parent_of,
    },
    observable::{CompiledSet, FunctionEntry, panic_message},
    protocol::{
        Caller, Delivery, DroppedReportStats, ErrorBody, InvocationStatus, LocalFunctionStats,
        Message, TriggerAction, TriggerRequest,
    },
    reporter::{CaptureMask, Gate, PendingItem, STATS_INTERVAL},
    types::RemoteFunctionHandlerWithMetadata,
};

/// What running a handler produced.
pub(crate) struct Executed {
    pub result: Result<Value, Error>,
    /// The stacktrace recorded on the span for a non-`Remote` error.
    pub stacktrace: Option<String>,
    /// The `execute <fn>` span's context, for the reply's trace headers.
    pub otel_cx: OtelContext,
    /// The invocation the handler ran as, its trace context filled in.
    pub frame: Arc<Frame>,
}

/// Local calls nested deeper than this go through the engine instead.
pub(crate) const MAX_LOCAL_DEPTH: u32 = 64;

/// Every this many nested local calls, the next one continues on a task of
/// its own: a task is polled from the scheduler, not from inside its caller's
/// poll, so a deep chain of local calls never grows one stack by more than
/// this many levels (each costs tens of KiB of stack).
const LOCAL_LEVELS_PER_TASK: u32 = 8;

/// Reports wait this long for their function's `functionregistered` on a new
/// connection before they are dropped as `INACTIVE`.
const PARK_TIMEOUT: Duration = Duration::from_secs(30);

fn trace_payloads_enabled() -> bool {
    !std::env::var("III_DISABLE_TRACE_PAYLOADS")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// Runs `handler` inside an `execute <fn>` span (a child of `parent_cx`) with
/// `ctx` as the current invocation, `local_depth` local calls deep. A panic
/// in the handler becomes an error.
pub(crate) async fn execute(
    parent_cx: OtelContext,
    data: Value,
    metadata: Option<Value>,
    handler: RemoteFunctionHandlerWithMetadata,
    mut ctx: InvocationCtx,
    local_depth: u32,
) -> Executed {
    use iii_helpers::observability::opentelemetry::KeyValue;
    use iii_helpers::observability::opentelemetry::trace::{
        FutureExt as OtelFutureExt, SpanKind, Status, TraceContextExt, Tracer,
    };
    use iii_helpers::observability::redact_and_truncate;

    // INTERNAL and named `execute` (not `call`/`trigger`): the engine already
    // emits the SERVER `call <fn>` span for a routed hop AND a `trigger <fn>`
    // span from fire_triggers. Reusing either name would duplicate an engine
    // span under the worker's service. `execute` is unique, so the worker
    // handler span reads as a clean internal child of its caller.
    let otel_cx = {
        let tracer = iii_helpers::observability::opentelemetry::global::tracer("iii-rust-sdk");
        let span = tracer
            .span_builder(format!("execute {}", ctx.function_id))
            .with_kind(SpanKind::Internal)
            .start_with_context(&tracer, &parent_cx);
        parent_cx.with_span(span)
    };
    // What events emitted from this invocation carry, so their activations
    // join its trace (also from tasks the handler spawns with `scope`).
    (ctx.traceparent, ctx.baggage) = {
        let _guard = otel_cx.clone().attach();
        inject_trace_headers()
    };
    let frame = Arc::new(Frame {
        ctx: Some(ctx),
        local_depth,
    });

    let trace_payloads = otel_cx.span().is_recording() && trace_payloads_enabled();
    let payload_max_bytes = if trace_payloads {
        iii_helpers::observability::resolve_max_bytes_from_env()
    } else {
        None
    };

    if trace_payloads {
        let (input_json, truncated) = redact_and_truncate(&data, payload_max_bytes);
        otel_cx.span().add_event(
            "iii.invocation.input",
            vec![
                KeyValue::new("iii.payload.json", input_json),
                KeyValue::new("iii.payload.truncated", truncated),
            ],
        );
    }

    // The handler is called inside the scope and the span: a sync handler's
    // body runs at that call.
    let run = scope_frame(frame.clone(), async move { handler(data, metadata).await });
    let result = AssertUnwindSafe(run.with_context(otel_cx.clone()))
        .catch_unwind()
        .await
        .unwrap_or_else(|panic| {
            Err(Error::Handler(format!(
                "handler panicked: {}",
                panic_message(&panic)
            )))
        });

    if trace_payloads {
        let (output_json, truncated, ok) = match &result {
            Ok(value) => {
                let (j, t) = redact_and_truncate(value, payload_max_bytes);
                (j, t, true)
            }
            Err(err) => {
                let payload = json!({ "error": err.to_string() });
                let (j, t) = redact_and_truncate(&payload, payload_max_bytes);
                (j, t, false)
            }
        };
        otel_cx.span().add_event(
            "iii.invocation.output",
            vec![
                KeyValue::new("iii.payload.json", output_json),
                KeyValue::new("iii.payload.truncated", truncated),
                KeyValue::new("iii.payload.ok", ok),
            ],
        );
    }

    // Record span status based on result
    let mut stacktrace = None;
    {
        let span = otel_cx.span();
        match &result {
            Ok(_) => span.set_status(Status::Ok),
            Err(err) => {
                let (exc_type, exc_message, trace) = match err {
                    Error::Remote {
                        code,
                        message,
                        stacktrace,
                    } => (
                        code.clone(),
                        message.clone(),
                        stacktrace.clone().unwrap_or_else(|| {
                            std::backtrace::Backtrace::force_capture().to_string()
                        }),
                    ),
                    other => (
                        "InvocationError".to_string(),
                        other.to_string(),
                        std::backtrace::Backtrace::force_capture().to_string(),
                    ),
                };
                span.set_status(Status::error(exc_message.clone()));
                span.add_event(
                    "exception",
                    vec![
                        KeyValue::new("exception.type", exc_type),
                        KeyValue::new("exception.message", exc_message),
                        KeyValue::new("exception.stacktrace", trace.clone()),
                    ],
                );
                // Consumed only by the non-`Remote` fallback arm of
                // `error_body`; `Remote` passes its own (possibly absent)
                // stacktrace through.
                stacktrace = Some(trace);
            }
        }
    }

    Executed {
        result,
        stacktrace,
        otel_cx,
        frame,
    }
}

/// The wire error of a failed handler: a `Remote` error keeps its code;
/// anything else is `invocation_failed`.
pub(crate) fn error_body(error: Error, stacktrace: Option<String>) -> ErrorBody {
    match error {
        Error::Remote {
            code,
            message,
            stacktrace,
        } => ErrorBody {
            code,
            message,
            // `Remote` is a structured, expected error owned by the handler:
            // respect `stacktrace: None` instead of backfilling the dispatch
            // loop's backtrace, which points at the SDK, not the error site.
            stacktrace,
        },
        other => ErrorBody {
            code: "invocation_failed".to_string(),
            message: other.to_string(),
            stacktrace: stacktrace
                .or_else(|| Some(std::backtrace::Backtrace::force_capture().to_string())),
        },
    }
}

impl IIIClient {
    /// The local handler for a call, when the five local dispatch conditions
    /// (see [`Dispatch`](crate::protocol::Dispatch)) that depend on the
    /// function hold.
    pub(crate) fn local_target(
        &self,
        function_id: &str,
        namespace: Option<&str>,
    ) -> Option<(RemoteFunctionHandlerWithMetadata, Arc<FunctionEntry>)> {
        let functions = self.functions();
        let data = functions.get(function_id)?;
        if !data.observe.local_dispatch() {
            return None;
        }
        let handler = data.handler.as_ref()?;
        let registered_in = data.observe.namespace()?;
        if namespace.unwrap_or(crate::protocol::DEFAULT_NAMESPACE) != registered_in.as_str() {
            return None;
        }
        Some((handler.clone(), data.observe.clone()))
    }

    /// The invocation context of a local call made from the current task.
    fn local_ctx(&self, function_id: &str, metadata: Option<Value>) -> InvocationCtx {
        let me = self.self_caller();
        with_current(|parent| InvocationCtx {
            function_id: function_id.to_string(),
            invocation_id: uuid::Uuid::new_v4().to_string(),
            caller: Some(Caller {
                function_id: parent.map(|p| p.function_id.clone()),
                invocation_id: parent.map(|p| p.invocation_id.clone()),
                parent: parent.and_then(|p| p.caller.as_ref()).and_then(parent_of),
                ..me
            }),
            activation: None,
            observe_depth: parent.map_or(0, |p| p.observe_depth),
            metadata,
            traceparent: None,
            baggage: None,
        })
    }

    /// Runs a function this worker hosts in-process. The handler gets the
    /// request's own payload (moved, so the caller's value is untouched by
    /// construction), the call returns the same result, error shape and
    /// timeout as a remote one, and a `void` call is spawned with its context.
    pub(crate) async fn invoke_local(
        &self,
        handler: RemoteFunctionHandlerWithMetadata,
        entry: Arc<FunctionEntry>,
        request: TriggerRequest,
        metadata: Option<Value>,
    ) -> Result<Value, Error> {
        let ctx = self.local_ctx(&request.function_id, metadata);
        let is_void = matches!(request.action, Some(TriggerAction::Void));
        let depth = local_depth() + 1;
        let run = self.clone().run_local(
            handler,
            entry,
            ctx,
            depth,
            OtelContext::current(),
            request.payload,
        );
        if is_void {
            tokio::spawn(async move {
                if let Err(err) = run.await {
                    tracing::warn!(error = %err, "error handling async invocation");
                }
            });
            return Ok(Value::Null);
        }
        let timeout = Duration::from_millis(request.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS));
        let mut run: std::pin::Pin<Box<dyn Future<Output = Result<Value, Error>> + Send>> =
            if depth % LOCAL_LEVELS_PER_TASK == 0 {
                let task = tokio::spawn(run);
                Box::pin(async move {
                    task.await.unwrap_or_else(|error| {
                        Err(Error::Handler(format!("local invocation stopped: {error}")))
                    })
                })
            } else {
                Box::pin(run)
            };
        tokio::select! {
            biased;
            result = &mut run => result,
            _ = tokio::time::sleep(timeout) => {
                // Like a remote handler, the local one keeps running past the
                // caller's timeout; its outcome is still counted and reported.
                tokio::spawn(run);
                Err(Error::Timeout)
            }
        }
    }

    async fn run_local(
        self,
        handler: RemoteFunctionHandlerWithMetadata,
        entry: Arc<FunctionEntry>,
        ctx: InvocationCtx,
        local_depth: u32,
        parent_cx: OtelContext,
        payload: Value,
    ) -> Result<Value, Error> {
        let observed = (entry.delivery == Delivery::Auto && entry.is_observed())
            .then(|| entry.compiled())
            .filter(|set| !set.is_empty());
        // Only what a filter or a capture reads is kept: the handler owns the
        // payload.
        let input = match &observed {
            Some(set) if set.needs.input => Some(payload.clone()),
            _ => None,
        };
        let started_at_ms = if observed.is_some() { now_ms() } else { 0 };
        let started = Instant::now();
        let metadata = ctx.metadata.clone();
        let Executed {
            result,
            stacktrace,
            frame,
            ..
        } = execute(parent_cx, payload, metadata, handler, ctx, local_depth).await;
        let elapsed = started.elapsed();
        entry.stats.record(result.is_ok(), elapsed);
        let result = result.map_err(|error| {
            let body = error_body(error, stacktrace);
            Error::Remote {
                code: body.code,
                message: body.message,
                stacktrace: body.stacktrace,
            }
        });
        if let Some(set) = observed {
            self.report_auto(
                &entry,
                &set,
                frame.invocation(),
                input,
                &result,
                started_at_ms,
                elapsed,
            );
        }
        result
    }

    /// Evaluates the observers of an auto function against one finished
    /// local call and buffers a report when any matches.
    #[allow(clippy::too_many_arguments)]
    fn report_auto(
        &self,
        entry: &FunctionEntry,
        set: &CompiledSet,
        ctx: &InvocationCtx,
        input: Option<Value>,
        result: &Result<Value, Error>,
        started_at_ms: u64,
        elapsed: Duration,
    ) {
        let success = result.is_ok();
        // The error as the engine reports a routed one: the invocation's
        // error body (`run_local` already shaped every error as `Remote`).
        let error = match result {
            Ok(_) => None,
            Err(Error::Remote {
                code,
                message,
                stacktrace,
            }) => serde_json::to_value(ErrorBody {
                code: code.clone(),
                message: message.clone(),
                stacktrace: stacktrace.clone(),
            })
            .ok(),
            Err(other) => {
                Some(json!({ "code": "invocation_failed", "message": other.to_string() }))
            }
        };
        let caller = if set.needs.caller {
            ctx.caller
                .as_ref()
                .and_then(|caller| serde_json::to_value(caller).ok())
        } else {
            None
        };
        let doc = Doc {
            input: input.as_ref(),
            output: result.as_ref().ok(),
            error: error.as_ref(),
            caller: caller.as_ref(),
            metadata: ctx.metadata.as_ref(),
        };
        let mut matched = Vec::new();
        let mut mask = CaptureMask::default();
        for observer in &set.observers {
            if observer.is_active() && observer.accepts(success) && observer.filter().matches(&doc)
            {
                matched.push(observer.id().to_string());
                mask = mask.union(observer.mask());
            }
        }
        if matched.is_empty() {
            return;
        }

        let raw = |value: &Value| to_raw_value(value).ok();
        self.reporter().push(PendingItem {
            function_id: entry.id.clone(),
            namespace: entry.namespace().map(|ns| ns.to_string()),
            invocation_id: ctx.invocation_id.clone(),
            matched,
            status: if success {
                InvocationStatus::Success
            } else {
                InvocationStatus::Error
            },
            input: input.as_ref().filter(|_| mask.input).and_then(raw),
            output: result.as_ref().ok().filter(|_| mask.output).and_then(raw),
            error: error.as_ref().filter(|_| mask.error).and_then(raw),
            caller: ctx.caller.clone().filter(|_| mask.caller),
            metadata: ctx
                .metadata
                .as_ref()
                .filter(|_| mask.metadata)
                .and_then(raw),
            started_at_ms,
            duration_ms: elapsed.as_secs_f64() * 1000.0,
            observe_depth: ctx.observe_depth,
            traceparent: ctx.traceparent.clone(),
            baggage: ctx.baggage.clone(),
            mask,
            emitted: false,
            seq: 0,
        });
    }

    /// Sends what is due from the report buffer: items of functions the
    /// engine confirmed on this connection. The others wait (parked) for
    /// their confirmation, or are dropped when the function is gone.
    pub(crate) fn flush_reports(&self) {
        let epoch = self.connection_epoch();
        let frames = self.reporter().take_report_frames(|function_id| {
            match self.function_entry(function_id) {
                None => Gate::Drop,
                Some(entry) if entry.confirmed_on(epoch) => Gate::Send,
                // Another worker owns it now: nothing of it will be accepted.
                Some(entry) if entry.is_revoked() => Gate::Drop,
                Some(_) => Gate::Hold,
            }
        });
        for frame in frames {
            if !self.is_running() {
                // the send path is closed: the frame would vanish
                self.reporter()
                    .count_items(&frame.functions, crate::protocol::observe_codes::INACTIVE);
                continue;
            }
            let envelope = PreparedEnvelope::from_json(frame.json, None, None)
                .observe_family()
                .with_report_lease(frame.lease);
            if let Err(error) = self.send_prepared(envelope) {
                tracing::debug!(error = %error, "report frame not sent");
                self.reporter()
                    .count_items(&frame.functions, crate::protocol::observe_codes::INACTIVE);
            }
        }
    }

    /// `localinvocationstats` for the window since `since`, when non-zero.
    pub(crate) fn send_stats(&self, since: Instant) {
        let functions: Vec<LocalFunctionStats> = self
            .functions()
            .values()
            .filter_map(|data| {
                let (count, errors, duration_ms_sum) = data.observe.stats.take();
                (count > 0).then(|| LocalFunctionStats {
                    function_id: data.message.id.clone(),
                    count,
                    errors,
                    duration_ms_sum,
                })
            })
            .collect();
        let dropped_reports: Vec<DroppedReportStats> = self
            .reporter()
            .take_drops()
            .into_iter()
            .map(|(function_id, reason, count)| DroppedReportStats {
                function_id,
                reason: reason.to_string(),
                count,
            })
            .collect();
        if functions.is_empty() && dropped_reports.is_empty() {
            return;
        }
        let _ = self.send_message(Message::LocalInvocationStats {
            window_ms: since.elapsed().as_millis() as u64,
            functions,
            dropped_reports,
        });
    }
}

/// Flushes reports (256 items or 5 ms) and sends stats (every 10 s) while the
/// worker runs. Runs on the connection's runtime.
pub(crate) async fn run_reporter(iii: IIIClient) {
    let reporter = iii.reporter();
    let mut stats = tokio::time::interval(STATS_INTERVAL);
    stats.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    stats.tick().await;
    let mut window = Instant::now();
    while iii.is_running() {
        let deadline = if iii.is_connected() {
            reporter.deadline()
        } else {
            None
        };
        tokio::select! {
            _ = reporter.wake.notified() => {}
            _ = tokio::time::sleep_until(tokio::time::Instant::from_std(
                deadline.unwrap_or_else(Instant::now),
            )), if deadline.is_some() => {}
            _ = stats.tick() => {
                // Reports of a function the engine never confirmed again.
                let epoch = iii.connection_epoch();
                reporter.expire_parked(PARK_TIMEOUT, |function_id| {
                    iii.function_entry(function_id)
                        .is_some_and(|entry| entry.confirmed_on(epoch))
                });
                // Counted while disconnected or against an engine that does not
                // take them; sent from the next window an observe engine sees.
                if iii.is_connected() && iii.engine_observes() {
                    iii.send_stats(window);
                    window = Instant::now();
                }
                continue;
            }
        }
        if !iii.is_connected() {
            continue;
        }
        if reporter.deadline().is_some_and(|due| due <= Instant::now()) {
            iii.flush_reports();
        }
    }
}

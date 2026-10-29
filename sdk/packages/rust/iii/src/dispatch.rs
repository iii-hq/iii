//! Running handlers and delivering their calls.
//!
//! Both paths go through [`execute`]: the remote one (an `invokefunction`
//! from the engine) and the local one (a [`call`](crate::IIIClient::call) of
//! a function this worker hosts, once the engine confirmed it). So both
//! create the same `execute <fn>` span, set the same current invocation and
//! produce the same error shape.
//!
//! Then the host delivers the call: iii's delivery evaluates the observers'
//! `on` and compiled filters and reports only the matches, or the function's
//! deliver hook decides, through [`Next`]. A local call's matches join the
//! batched `invocationsobserved` reports; a routed call's ride its
//! `invocationresult` (`observed`), or go out as report items just ahead of
//! it. A call with no observer and no deliver hook costs one atomic load and
//! puts nothing on the wire.
use std::{
    future::Future,
    panic::AssertUnwindSafe,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use futures_util::FutureExt;
use iii_filter::Roots;
use iii_helpers::observability::opentelemetry::Context as OtelContext;
use serde_json::{Value, json, value::to_raw_value};

use crate::{
    context::{Frame, InvocationCtx, local_depth, scope_frame, with_current},
    error::Error,
    hooks::{Call, DeliverHook, Next, same_json},
    iii::{
        DEFAULT_TIMEOUT_MS, IIIClient, MutexExt, PreparedEnvelope, inject_trace_headers, now_ms,
        parent_of,
    },
    observable::{DocParts, FunctionEntry, Observer, panic_message},
    observe::ObservedError,
    protocol::{
        CallAction, CallRequest, Caller, DroppedReportStats, ErrorBody, InvocationStatus,
        LocalFunctionStats, Message, observe_codes,
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

    // INTERNAL and named `execute` (not `call`): the engine already emits the
    // SERVER `call <fn>` span for a routed hop. Reusing that name would
    // duplicate an engine span under the worker's service. `execute` is
    // unique, so the worker handler span reads as a clean internal child of
    // its caller.
    let otel_cx = {
        let tracer = iii_helpers::observability::opentelemetry::global::tracer("iii-rust-sdk");
        let span = tracer
            .span_builder(format!("execute {}", ctx.function_id))
            .with_kind(SpanKind::Internal)
            .start_with_context(&tracer, &parent_cx);
        parent_cx.with_span(span)
    };
    // What the reports of this invocation carry, so their activations join
    // its trace (also from tasks the handler spawns with `scope`).
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

/// Who delivers a call of a function this worker hosts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Route {
    /// A local call: its matches are reported.
    Local,
    /// A call routed by an `observe.v2` engine: its matches ride the answer
    /// or go out just ahead of it.
    Routed,
    /// A call routed by an engine that evaluates its observers itself.
    Engine,
}

/// A call of a function this worker hosts, to run and deliver.
pub(crate) struct Hosted {
    pub handler: RemoteFunctionHandlerWithMetadata,
    pub entry: Arc<FunctionEntry>,
    pub ctx: InvocationCtx,
    pub local_depth: u32,
    pub parent_cx: OtelContext,
    pub payload: Value,
    pub route: Route,
}

/// What a call of a function this worker hosts produced, once delivered.
pub(crate) struct Answer {
    /// What the caller gets: the handler's outcome, or its deliver hook's.
    pub result: Result<Value, ErrorBody>,
    /// A routed call's matches that ride its `invocationresult`.
    pub observed: Vec<String>,
    /// A routed call's report items, sent just ahead of its answer.
    pub items: Vec<PendingItem>,
    /// The `execute <fn>` span's context, for the answer's trace headers.
    pub otel_cx: OtelContext,
}

/// Runs a deliver hook inside the call's invocation context and span; a
/// panic is an error.
async fn run_deliver_hook(
    hook: Arc<dyn DeliverHook>,
    call: Call,
    observers: Vec<Observer>,
    next: Next,
    frame: Arc<Frame>,
    otel_cx: &OtelContext,
) -> Result<Value, Error> {
    use iii_helpers::observability::opentelemetry::trace::FutureExt as OtelFutureExt;
    let run = scope_frame(frame, async move {
        hook.on_deliver(call, observers, next).await
    });
    AssertUnwindSafe(run.with_context(otel_cx.clone()))
        .catch_unwind()
        .await
        .unwrap_or_else(|panic| {
            Err(Error::Handler(format!(
                "on_observer_deliver panicked: {}",
                panic_message(&panic)
            )))
        })
}

fn observed_error(body: &ErrorBody) -> ObservedError {
    ObservedError {
        code: body.code.clone(),
        message: body.message.clone(),
    }
}

/// A call as a filter and a report see it, borrowed.
pub(crate) struct CallView<'a> {
    input: Option<&'a Value>,
    output: Option<&'a Value>,
    error: Option<&'a ObservedError>,
    caller: Option<&'a Caller>,
    metadata: Option<&'a Value>,
    success: bool,
}

impl<'a> CallView<'a> {
    fn of(call: &'a Call) -> Self {
        let success = call.status == InvocationStatus::Success;
        Self {
            input: Some(&call.input),
            output: call.output.as_ref().filter(|_| success),
            error: call.error.as_ref(),
            caller: call.caller.as_ref(),
            metadata: call.metadata.as_ref(),
            success,
        }
    }
}

/// What every report item of one call shares.
struct ReportCtx {
    invocation_id: String,
    started_at_ms: u64,
    duration_ms: f64,
    observe_depth: u32,
    traceparent: Option<String>,
    baggage: Option<String>,
}

/// The delivery of one call: what its `next` calls delivered, until its
/// deliver hook settles.
pub(crate) struct Delivery {
    iii: IIIClient,
    entry: Arc<FunctionEntry>,
    report: ReportCtx,
    /// A routed call: the matches of the handler's call ride the answer.
    routed: bool,
    /// The handler's call, to tell a `next` call given it unchanged.
    original: Option<Call>,
    state: Mutex<DeliveryState>,
}

#[derive(Default)]
struct DeliveryState {
    /// Observers delivered so far: each at most once per call.
    delivered: Vec<String>,
    /// Matches of `next` calls given the handler's call unchanged.
    pooled: Vec<String>,
    pooled_mask: CaptureMask,
    /// One item per `next` call given a changed call.
    items: Vec<PendingItem>,
    settled: bool,
}

impl Delivery {
    /// The observers of this function among `observers` that are active,
    /// accept the call's status and whose filters match it, less those
    /// already delivered, which it records.
    fn select(
        &self,
        view: &CallView<'_>,
        observers: &[Observer],
        delivered: &mut Vec<String>,
    ) -> (Vec<String>, CaptureMask) {
        let candidates = || {
            observers.iter().filter(|obs| {
                obs.function_id() == self.entry.id && obs.is_active() && obs.accepts(view.success)
            })
        };
        let mut roots = Roots::default();
        for obs in candidates() {
            let read = obs.filter().roots();
            roots.error |= read.error;
            roots.caller |= read.caller;
        }
        let parts = DocParts::of(roots, view.error, view.caller);
        let doc = parts.doc(view.input, view.output, view.metadata);
        let mut matched = Vec::new();
        let mut mask = CaptureMask::default();
        for obs in candidates() {
            if delivered.iter().any(|id| id == obs.id()) || !obs.filter().matches(&doc) {
                continue;
            }
            delivered.push(obs.id().to_string());
            matched.push(obs.id().to_string());
            mask = mask.union(obs.mask());
        }
        (matched, mask)
    }

    /// iii's delivery of the handler's call to `observers`: a routed call's
    /// matches ride its answer, a local call's are reported.
    fn deliver_unchanged(
        &self,
        view: &CallView<'_>,
        observers: &[Observer],
    ) -> (Vec<String>, Vec<PendingItem>) {
        let (matched, mask) = self.select(view, observers, &mut Vec::new());
        if matched.is_empty() || self.routed {
            return (matched, Vec::new());
        }
        self.iii.reporter().push(self.item(view, matched, mask));
        (Vec::new(), Vec::new())
    }

    /// One `next` call: see [`Next::run`].
    pub(crate) fn run(&self, call: &Call, observers: &[Observer]) -> Result<Value, Error> {
        let unchanged = self
            .original
            .as_ref()
            .is_some_and(|original| original.same_as(call));
        let view = CallView::of(call);
        let mut state = self.state.lock_or_recover();
        if state.settled && self.routed {
            drop(state);
            // Its answer went out: nothing more is delivered for it.
            self.iii
                .reporter()
                .count_drop(&self.entry.id, observe_codes::LATE_DELIVERY);
            return call.result();
        }
        let (matched, mask) = self.select(&view, observers, &mut state.delivered);
        if !matched.is_empty() {
            if state.settled {
                let item = self.item(&view, matched, mask);
                drop(state);
                self.iii.reporter().push(item);
            } else if unchanged {
                state.pooled.extend(matched);
                state.pooled_mask = state.pooled_mask.union(mask);
            } else {
                let item = self.item(&view, matched, mask);
                state.items.push(item);
            }
        }
        call.result()
    }

    /// The deliver hook settled with `answer`. A routed call's pooled
    /// matches ride the answer when it is the handler's own outcome, and go
    /// in an item carrying the handler's call otherwise. A local call's
    /// items are reported; a routed call's are returned, to go out ahead of
    /// its answer.
    fn settle(&self, answer: &Result<Value, ErrorBody>) -> (Vec<String>, Vec<PendingItem>) {
        let mut state = self.state.lock_or_recover();
        state.settled = true;
        let pooled = std::mem::take(&mut state.pooled);
        let pooled_mask = state.pooled_mask;
        let mut items = std::mem::take(&mut state.items);
        drop(state);
        let mut observed = Vec::new();
        if let Some(original) = self.original.as_ref().filter(|_| !pooled.is_empty()) {
            if self.routed && Self::is_own_outcome(original, answer) {
                observed = pooled;
            } else {
                items.push(self.item(&CallView::of(original), pooled, pooled_mask));
            }
        }
        if self.routed {
            return (observed, items);
        }
        for item in items {
            self.iii.reporter().push(item);
        }
        (Vec::new(), Vec::new())
    }

    /// Whether `answer` is the handler's own outcome: its output, or the
    /// error it threw.
    fn is_own_outcome(original: &Call, answer: &Result<Value, ErrorBody>) -> bool {
        match (answer, &original.error) {
            (Ok(value), None) => original
                .output
                .as_ref()
                .is_some_and(|output| same_json(output, value)),
            (Err(body), Some(error)) => body.code == error.code && body.message == error.message,
            _ => false,
        }
    }

    /// A report item: the parts of the call `mask` captures. An item that
    /// carries both the input and the output of a handler that returned its
    /// input carries the value once (`output_is_input`).
    fn item(&self, view: &CallView<'_>, matched: Vec<String>, mask: CaptureMask) -> PendingItem {
        let raw = |value: &Value| to_raw_value(value).ok();
        let input = view.input.filter(|_| mask.input);
        let output = view.output.filter(|_| mask.output);
        let output_is_input = matches!((input, output), (Some(i), Some(o)) if same_json(i, o));
        PendingItem {
            function_id: self.entry.id.clone(),
            namespace: self.entry.namespace().map(|ns| ns.to_string()),
            invocation_id: self.report.invocation_id.clone(),
            matched,
            status: if view.success {
                InvocationStatus::Success
            } else {
                InvocationStatus::Error
            },
            input: input.and_then(raw),
            output: output.filter(|_| !output_is_input).and_then(raw),
            error: view
                .error
                .filter(|_| mask.error)
                .and_then(|error| to_raw_value(error).ok()),
            caller: view.caller.filter(|_| mask.caller).and_then(report_caller),
            metadata: view.metadata.filter(|_| mask.metadata).and_then(raw),
            started_at_ms: self.report.started_at_ms,
            duration_ms: self.report.duration_ms,
            observe_depth: self.report.observe_depth,
            traceparent: self.report.traceparent.clone(),
            baggage: self.report.baggage.clone(),
            mask,
            output_is_input,
            seq: 0,
        }
    }
}

/// The `caller` a report item carries: the call's context, without the
/// `worker_id` and `worker_name` the engine stamps itself; `None` for a call
/// made outside any handler.
fn report_caller(caller: &Caller) -> Option<Caller> {
    let caller = Caller {
        worker_id: None,
        worker_name: None,
        ..caller.clone()
    };
    (caller != Caller::default()).then_some(caller)
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
        request: CallRequest,
        metadata: Option<Value>,
    ) -> Result<Value, Error> {
        let ctx = self.local_ctx(&request.function_id, metadata);
        let is_void = matches!(request.action, Some(CallAction::Void));
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
        let hosted = self
            .run_hosted(Hosted {
                handler,
                entry,
                ctx,
                local_depth,
                parent_cx,
                payload,
                route: Route::Local,
            })
            .await;
        hosted.result.map_err(|body| Error::Remote {
            code: body.code,
            message: body.message,
            stacktrace: body.stacktrace,
        })
    }

    /// Runs the handler of a function this worker hosts, then delivers the
    /// call: iii's delivery, or the function's deliver hook. With no observer
    /// and no deliver hook, it only runs the handler.
    pub(crate) async fn run_hosted(&self, run: Hosted) -> Answer {
        let Hosted {
            handler,
            entry,
            ctx,
            local_depth,
            parent_cx,
            payload,
            route,
        } = run;
        let local = route == Route::Local;
        if route == Route::Engine || !entry.delivers() {
            let started = Instant::now();
            let metadata = ctx.metadata.clone();
            let executed = execute(parent_cx, payload, metadata, handler, ctx, local_depth).await;
            if local {
                entry
                    .stats
                    .record(executed.result.is_ok(), started.elapsed());
            }
            return Answer {
                result: executed
                    .result
                    .map_err(|error| error_body(error, executed.stacktrace)),
                observed: Vec::new(),
                items: Vec::new(),
                otel_cx: executed.otel_cx,
            };
        }

        let hook = entry.deliver_hook();
        let snapshot = entry.compiled();
        // What a filter reads, what a report carries and what a deliver hook
        // gets: only that is kept of the input, which the handler owns.
        let keep_input =
            hook.is_some() || snapshot.roots.input || (local && snapshot.capture.input);
        let input = keep_input.then(|| payload.clone());
        let started_at_ms = now_ms();
        let started = Instant::now();
        let metadata = ctx.metadata.clone();
        let Executed {
            result,
            stacktrace,
            otel_cx,
            frame,
        } = execute(parent_cx, payload, metadata, handler, ctx, local_depth).await;
        let elapsed = started.elapsed();
        if local {
            entry.stats.record(result.is_ok(), elapsed);
        }
        let result = result.map_err(|error| error_body(error, stacktrace));
        let invocation = frame.invocation();
        let delivery = Delivery {
            iii: self.clone(),
            entry: entry.clone(),
            report: ReportCtx {
                invocation_id: invocation.invocation_id.clone(),
                started_at_ms,
                duration_ms: elapsed.as_secs_f64() * 1000.0,
                observe_depth: invocation.observe_depth,
                traceparent: invocation.traceparent.clone(),
                baggage: invocation.baggage.clone(),
            },
            routed: !local,
            original: None,
            state: Mutex::new(DeliveryState::default()),
        };

        let Some(hook) = hook else {
            // iii's delivery: every active observer, the handler's call.
            let error = result.as_ref().err().map(observed_error);
            let view = CallView {
                input: input.as_ref(),
                output: result.as_ref().ok(),
                error: error.as_ref(),
                caller: invocation.caller.as_ref(),
                metadata: invocation.metadata.as_ref(),
                success: result.is_ok(),
            };
            let (observed, items) = delivery.deliver_unchanged(&view, &snapshot.observers);
            return Answer {
                result,
                observed,
                items,
                otel_cx,
            };
        };

        let (output, handler_error) = match result {
            Ok(output) => (Some(output), None),
            Err(body) => (None, Some(body)),
        };
        let mut call = Call {
            function_id: entry.id.clone(),
            invocation_id: invocation.invocation_id.clone(),
            status: if handler_error.is_some() {
                InvocationStatus::Error
            } else {
                InvocationStatus::Success
            },
            input: input.unwrap_or(Value::Null),
            output,
            error: handler_error.as_ref().map(observed_error),
            caller: invocation.caller.clone(),
            metadata: invocation.metadata.clone(),
        };

        if let Some(keyed) = hook.as_keyed() {
            // A keyed index: the observers filed under the call's keys get
            // the handler's call, as from iii's delivery.
            let chosen = keyed.observers_for(&call);
            let (observed, items) = delivery.deliver_unchanged(&CallView::of(&call), &chosen);
            let result = match handler_error {
                Some(body) => Err(body),
                None => Ok(call.output.take().unwrap_or(Value::Null)),
            };
            return Answer {
                result,
                observed,
                items,
                otel_cx,
            };
        }

        // The hook's own copy, when what `next` delivers is compared with
        // the handler's call.
        let observers: Vec<Observer> = snapshot
            .observers
            .iter()
            .filter(|obs| obs.is_active())
            .cloned()
            .collect();
        let delivery = Arc::new(Delivery {
            original: (!observers.is_empty()).then(|| call.clone()),
            ..delivery
        });
        let next = Next {
            delivery: delivery.clone(),
        };
        let answer = run_deliver_hook(hook, call, observers, next, frame.clone(), &otel_cx).await;
        let answer = answer.map_err(|error| match (&handler_error, &error) {
            // the handler's own error, rethrown: its body, stacktrace included
            (Some(body), Error::Remote { code, message, .. })
                if body.code == *code && body.message == *message =>
            {
                body.clone()
            }
            _ => error_body(error, None),
        });
        let (observed, items) = delivery.settle(&answer);
        Answer {
            result: answer,
            observed,
            items,
            otel_cx,
        }
    }

    /// Sends what is due from the report buffer: items of functions the
    /// engine confirmed on this connection. The others wait (parked) for
    /// their confirmation, or are dropped when the function is gone.
    pub(crate) fn flush_reports(&self) {
        let epoch = self.connection_epoch();
        let v2 = self.engine_observes_v2();
        let frames = self.reporter().take_report_frames(
            |function_id| match self.function_entry(function_id) {
                None => Gate::Drop,
                Some(entry) if entry.confirmed_on(epoch) => Gate::Send,
                // Another worker hosts it now: nothing of it will be accepted.
                Some(entry) if entry.is_revoked() => Gate::Drop,
                Some(_) => Gate::Hold,
            },
            v2,
        );
        for frame in frames {
            if !self.is_running() {
                // the send path is closed: the frame would vanish
                self.reporter()
                    .count_items(&frame.functions, observe_codes::INACTIVE);
                continue;
            }
            let envelope = PreparedEnvelope::from_json(frame.json, None, None)
                .observe_family()
                .with_report_lease(frame.lease);
            if let Err(error) = self.send_prepared(envelope) {
                tracing::debug!(error = %error, "report frame not sent");
                self.reporter()
                    .count_items(&frame.functions, observe_codes::INACTIVE);
            }
        }
    }

    /// Sends the report items of a routed call that arrived on connection
    /// `epoch`, ahead of its answer and without waiting for a batch. Items
    /// whose connection closed meanwhile count as `DISCONNECTED`.
    pub(crate) fn send_routed_items(&self, items: Vec<PendingItem>, epoch: u64) {
        if items.is_empty() {
            return;
        }
        if epoch != self.connection_epoch() || !self.is_connected() {
            let functions: Vec<String> = items.into_iter().map(|item| item.function_id).collect();
            self.reporter()
                .count_items(&functions, observe_codes::DISCONNECTED);
            return;
        }
        for (json, functions) in self.reporter().frames_of(items, self.engine_observes_v2()) {
            let envelope = PreparedEnvelope::from_json(json, None, None)
                .observe_family()
                .routed_reports(functions.clone());
            if self.send_prepared(envelope).is_err() {
                self.reporter()
                    .count_items(&functions, observe_codes::DISCONNECTED);
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

//! Running handlers and delivering their calls.
//!
//! Both paths go through [`execute`]: the remote one (an `invokefunction`
//! from the engine) and the local one (a [`call`](crate::IIIClient::call) of
//! a function this worker hosts, once the engine confirmed it). So both
//! create the same `execute <fn>` span, set the same current invocation and
//! produce the same error shape.
//!
//! Then the call is delivered. Filters are iii's, and run where the call's
//! parts are: the engine evaluates the `on` and filters of every call it
//! routes (it holds the input, the answer, the caller and the metadata), and
//! this host those of the calls it makes to its own functions, which the
//! engine never sees. The host runs what is worker code for both: the
//! function's match hook, which reads the observers' args, and its deliver
//! hook.
//!
//! - A local call: iii's delivery evaluates the `on` and compiled filters of
//!   the function's observers and reports the matches in the batched
//!   `invocationsobserved` frames. Without a match hook, the compiled set's
//!   [`Plan`] for the call's outcome matches them: through an index of their
//!   equality conditions ([`FilterIndex`](iii_filter::FilterIndex)), or by a
//!   scan of every filter for a set of at most
//!   [`SCAN_MAX`](crate::observable::SCAN_MAX) observers, or one most of
//!   whose observers the index would evaluate anyway. The observers a match
//!   hook matched are each evaluated. Or the deliver hook delivers the call
//!   itself.
//! - A routed call: the answer says how it is delivered. Without hooks it
//!   carries nothing more, and the engine evaluates every active observer;
//!   with a match hook it lists the observers the hook matched
//!   (`invocationresult.matched`, before `on` and filters; `[]` also when
//!   the host holds no observer for it to match); with a deliver hook it is
//!   the hook's answer, with `delivered: true`.
//!
//! A call with no observer and no deliver hook costs one atomic load and puts
//! nothing on the wire.
use std::{
    future::Future,
    panic::AssertUnwindSafe,
    sync::Arc,
    time::{Duration, Instant},
};

use futures_util::FutureExt;
use iii_filter::{Doc, Roots};
use iii_helpers::observability::opentelemetry::Context as OtelContext;
use serde_json::{Value, json, value::to_raw_value};

use crate::{
    context::{Frame, InvocationCtx, local_depth, scope_frame, with_current},
    error::Error,
    hooks::{Call, DeliverHook, same_json},
    iii::{
        DEFAULT_TIMEOUT_MS, IIIClient, PreparedEnvelope, inject_trace_headers, now_ms, parent_of,
    },
    observable::{
        ById, CompiledSet, DocParts, FunctionEntry, Observer, OutcomeIndex, Plan, panic_message,
    },
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

/// The wire error of a failed deliver hook, as Node and Python answer it: a
/// call the hook made that failed, rethrown, keeps its code (`Remote`, and
/// [`Error::Timeout`] as `TIMEOUT`); anything else is `invocation_failed`.
pub(crate) fn deliver_error_body(error: Error) -> ErrorBody {
    match error {
        Error::Timeout => ErrorBody {
            code: TIMEOUT.to_string(),
            message: Error::Timeout.to_string(),
            stacktrace: None,
        },
        other => error_body(other, None),
    }
}

/// The code of a call that got no answer in time.
const TIMEOUT: &str = "TIMEOUT";

/// Who delivers a call of a function this worker hosts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Route {
    /// A local call: the host evaluates its observers and reports the
    /// matches.
    Local,
    /// A call routed by an `observe.v2` engine, which evaluates its
    /// observers' `on` and filters: the host runs the function's match and
    /// deliver hooks, and its answer says how the call is delivered.
    Routed,
    /// A call routed by an engine without `observe.v2`, which delivers it.
    Engine,
}

/// How a routed call is delivered, as its `invocationresult` says
/// (decision 55).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) enum Delivery {
    /// To every active observer whose `on` and filters match: neither
    /// `matched` nor `delivered`.
    #[default]
    Every,
    /// To those of these observers, the ones the match hook matched, whose
    /// `on` and filters match: `matched`, `[]` included.
    Matched(Vec<String>),
    /// By the function's deliver hook: `delivered: true`.
    Delivered,
}

impl Delivery {
    /// The answer's `matched` and `delivered`.
    pub(crate) fn into_wire(self) -> (Option<Vec<String>>, bool) {
        match self {
            Delivery::Every => (None, false),
            Delivery::Matched(ids) => (Some(ids), false),
            Delivery::Delivered => (None, true),
        }
    }
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
    /// How a routed call is delivered, which its `invocationresult` says.
    pub delivery: Delivery,
    /// The `execute <fn>` span's context, for the answer's trace headers.
    pub otel_cx: OtelContext,
}

/// Runs a deliver hook inside the call's invocation context and span; a
/// panic is an error.
async fn run_deliver_hook(
    hook: Arc<dyn DeliverHook>,
    call: Call,
    observers: Vec<Observer>,
    frame: Arc<Frame>,
    otel_cx: &OtelContext,
) -> Result<Value, Error> {
    use iii_helpers::observability::opentelemetry::trace::FutureExt as OtelFutureExt;
    let run = scope_frame(frame, async move { hook.on_deliver(call, observers).await });
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
struct CallView<'a> {
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
struct ReportCtx<'a> {
    entry: &'a FunctionEntry,
    invocation: &'a InvocationCtx,
    started_at_ms: u64,
    duration_ms: f64,
}

/// The observers of the function in `snapshot` that are active, accept the
/// call's status and whose filters match it, in id order, as the set's plan
/// for the call's status matches them: a scan of every observer, or the
/// observers the index of the status does not rule out, each filter
/// evaluated in full. The same observers as [`select`] over every observer
/// of the set. The call's `error` and `caller` are built only when a filter
/// of an observer of its status reads them.
fn select_compiled(
    function_id: &str,
    view: &CallView<'_>,
    snapshot: &CompiledSet,
) -> (Vec<String>, CaptureMask) {
    let plan = snapshot.plan(view.success);
    if plan == Plan::Nothing {
        return (Vec::new(), CaptureMask::default());
    }
    let parts = DocParts::of(snapshot.reads(view.success), view.error, view.caller);
    let doc = parts.doc(view.input, view.output, view.metadata);
    match (plan, snapshot.observers.as_slice()) {
        // A set that is one leaf is read as one slice.
        (Plan::Scan, Some(observers)) => scan(function_id, view.success, &doc, [observers]),
        (Plan::Scan, None) => scan(function_id, view.success, &doc, snapshot.observers.leaves()),
        _ => lookup(function_id, &doc, snapshot.accepting(view.success)),
    }
}

/// The observers of the function in `index` that are active and whose
/// filters match `doc`, in id order.
fn lookup(function_id: &str, doc: &Doc<'_>, index: &OutcomeIndex) -> (Vec<String>, CaptureMask) {
    let mut matched = Vec::new();
    let mut mask = CaptureMask::default();
    for (ById(obs), ()) in index.matches(doc) {
        if obs.function_id() == function_id && obs.is_active() {
            matched.push(obs.id().to_string());
            mask = mask.union(obs.mask());
        }
    }
    (matched, mask)
}

/// The observers of the function among `leaves` (a list, or the leaves of a
/// compiled set, each a slice) that are active, accept the call's status and
/// whose filters match it: each one evaluated.
fn select(
    function_id: &str,
    view: &CallView<'_>,
    leaves: &[&[Observer]],
) -> (Vec<String>, CaptureMask) {
    let candidate = |obs: &&Observer| candidate(obs, function_id, view.success);
    let mut roots = Roots::default();
    for leaf in leaves {
        for obs in leaf.iter().filter(candidate) {
            let read = obs.filter().roots();
            roots.error |= read.error;
            roots.caller |= read.caller;
        }
    }
    let parts = DocParts::of(roots, view.error, view.caller);
    scan(
        function_id,
        view.success,
        &parts.doc(view.input, view.output, view.metadata),
        leaves.iter().copied(),
    )
}

/// An observer of the function, active, whose `on` accepts the call.
fn candidate(obs: &Observer, function_id: &str, success: bool) -> bool {
    obs.function_id() == function_id && obs.is_active() && obs.accepts(success)
}

/// The observers of the function among `leaves` that are active, accept a
/// call that ended with `success` (or not) and whose filters match `doc`:
/// each one evaluated.
fn scan<'a>(
    function_id: &str,
    success: bool,
    doc: &Doc<'_>,
    leaves: impl IntoIterator<Item = &'a [Observer]>,
) -> (Vec<String>, CaptureMask) {
    let mut matched = Vec::new();
    let mut mask = CaptureMask::default();
    for leaf in leaves {
        for obs in leaf.iter() {
            if candidate(obs, function_id, success) && obs.filter().matches(doc) {
                matched.push(obs.id().to_string());
                mask = mask.union(obs.mask());
            }
        }
    }
    (matched, mask)
}

/// A report item: the parts of the call `mask` captures. An item that
/// carries both the input and the output of a handler that returned its
/// input carries the value once (`output_is_input`).
fn item(
    report: &ReportCtx<'_>,
    view: &CallView<'_>,
    matched: Vec<String>,
    mask: CaptureMask,
) -> PendingItem {
    let raw = |value: &Value| to_raw_value(value).ok();
    let input = view.input.filter(|_| mask.input);
    let output = view.output.filter(|_| mask.output);
    let output_is_input = matches!((input, output), (Some(i), Some(o)) if same_json(i, o));
    PendingItem {
        function_id: report.entry.id.clone(),
        namespace: report.entry.namespace().map(|ns| ns.to_string()),
        invocation_id: report.invocation.invocation_id.clone(),
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
        started_at_ms: report.started_at_ms,
        duration_ms: report.duration_ms,
        observe_depth: report.invocation.observe_depth,
        traceparent: report.invocation.traceparent.clone(),
        baggage: report.invocation.baggage.clone(),
        mask,
        output_is_input,
        seq: 0,
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
    /// and no deliver hook, it only runs the handler; so does a routed call
    /// of a function without hooks, whose observers the engine evaluates.
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
        let delivers = route != Route::Engine && entry.delivers();
        let hook = delivers.then(|| entry.deliver_hook()).flatten();
        let matcher = delivers.then(|| entry.matcher()).flatten();
        if !delivers || (route == Route::Routed && hook.is_none() && matcher.is_none()) {
            // The handler alone. A routed call's answer says how it is
            // delivered: to every active observer, or, by a function whose
            // match hook has no observer here to match, to none.
            let delivery = if route == Route::Routed && !delivers && entry.has_match_hook() {
                Delivery::Matched(Vec::new())
            } else {
                Delivery::Every
            };
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
                delivery,
                otel_cx: executed.otel_cx,
            };
        }

        let snapshot = entry.compiled();
        // What a filter reads, what a report carries and what a match or
        // deliver hook gets: only that is kept of the input, which the handler
        // owns. The filters of a routed call are the engine's.
        let keep_input = hook.is_some()
            || matcher.is_some()
            || (local && (snapshot.roots.input || snapshot.capture.input));
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
        let report = ReportCtx {
            entry: &entry,
            invocation,
            started_at_ms,
            duration_ms: elapsed.as_secs_f64() * 1000.0,
        };

        if hook.is_none() && matcher.is_none() {
            // A local call, delivered by iii to every active observer whose
            // `on` and filters match: the call's parts are read in place, and
            // the set's plan for the call's outcome scans its observers or
            // looks the call up in the outcome's index.
            let error = result.as_ref().err().map(observed_error);
            let view = CallView {
                input: input.as_ref(),
                output: result.as_ref().ok(),
                error: error.as_ref(),
                caller: invocation.caller.as_ref(),
                metadata: invocation.metadata.as_ref(),
                success: result.is_ok(),
            };
            self.report(&report, &view, select_compiled(&entry.id, &view, &snapshot));
            return Answer {
                result,
                delivery: Delivery::Every,
                otel_cx,
            };
        }

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
        let matched: Vec<Observer> = match &matcher {
            Some(matcher) => matcher.matching(&entry.id, &snapshot, &call),
            None => snapshot
                .observers
                .iter()
                .filter(|obs| obs.is_active())
                .cloned()
                .collect(),
        };

        let Some(hook) = hook else {
            // iii delivers the call to the observers the match hook matched
            // whose `on` and filters match it: evaluated here for a local
            // call, by the engine for a routed one.
            let delivery = if local {
                let view = CallView::of(&call);
                self.report(&report, &view, select(&entry.id, &view, &[&matched]));
                Delivery::Every
            } else {
                Delivery::Matched(matched.iter().map(|obs| obs.id().to_string()).collect())
            };
            let result = match handler_error {
                Some(body) => Err(body),
                None => Ok(call.output.take().unwrap_or(Value::Null)),
            };
            return Answer {
                result,
                delivery,
                otel_cx,
            };
        };

        // The hook delivers the call itself: iii delivers nothing for it.
        let answer = run_deliver_hook(hook, call, matched, frame.clone(), &otel_cx).await;
        let result = answer.map_err(|error| match (&handler_error, &error) {
            // the handler's own error, rethrown: its body, stacktrace included
            (Some(body), Error::Remote { code, message, .. })
                if body.code == *code && body.message == *message =>
            {
                body.clone()
            }
            _ => deliver_error_body(error),
        });
        Answer {
            result,
            delivery: Delivery::Delivered,
            otel_cx,
        }
    }

    /// Reports a local call's matches, `(ids, what they capture)`, when there
    /// are any.
    fn report(
        &self,
        report: &ReportCtx<'_>,
        view: &CallView<'_>,
        (matched, mask): (Vec<String>, CaptureMask),
    ) {
        if !matched.is_empty() {
            self.reporter().push(item(report, view, matched, mask));
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
                // Released: the engine accepts nothing of it.
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

#[cfg(test)]
mod tests {
    //! Matching of local calls against the evaluator: for seeded random
    //! observers (filters, `on`, capture, active or not) and calls, each
    //! outcome's index ([`lookup`]) and the set's plan ([`select_compiled`])
    //! select exactly what [`select`] over every observer of the set selects,
    //! in the same order and with the same capture, through adds,
    //! replacements and removals, and every version a call may hold keeps
    //! selecting what it held.
    //!
    //! Two corpora: the shared generator of `iii-filter`
    //! (`crates/iii-filter/tests/support/gen.rs`: every operator, nesting,
    //! `$any`, escaped keys, index segments, numbers near the integer / float
    //! boundaries, documents shaped by each filter), compared document by
    //! document; and an equality-heavy one written here, where many filters
    //! share paths and values of every root and calls respell them (`1` /
    //! `1.0` / `-0.0`, `"1"`, `true`, `null`, arrays and objects, absent
    //! paths), compared call by call as a local call is matched.

    use serde_json::{Map, Value, json};

    use super::*;
    use crate::filter_gen::{self as generator, Rng};
    use crate::observable::SCAN_MAX;
    use crate::observe::ObservedError;

    const FUNCTION: &str = "test::f";

    /// `on` values, `success` the most common; `start` is one this SDK does
    /// not know, which accepts no call.
    const ON: [&str; 6] = [
        "success", "success", "success", "error", "complete", "start",
    ];
    const PARTS: [&str; 5] = ["input", "output", "error", "caller", "metadata"];

    /// An observer of [`FUNCTION`] with `filters`, a random `on` and
    /// capture, active 9 times in 10.
    fn observer(rng: &mut Rng, id: &str, filters: &Value, seq: u64) -> Observer {
        let capture: Vec<&str> = PARTS.into_iter().filter(|_| rng.chance(40)).collect();
        Observer::test_view(
            json!({ "id": id, "target": "t", "filters": filters, "on": *rng.pick(&ON),
                    "capture": capture }),
            seq,
            !rng.chance(10),
        )
    }

    /// Ids in an order unrelated to the order observers are added in.
    fn id(n: usize) -> String {
        format!("o-{:04}", (n * 7_919) % 10_000)
    }

    /// What the scan of every observer and the index select agree on `doc`
    /// for a call that ended with `success`; returns how many observers the
    /// index evaluated.
    fn agree_on_doc(set: &CompiledSet, doc_value: &Value, success: bool, context: &str) -> usize {
        let mut doc = Doc::from_value(doc_value);
        if !success {
            // A failed call has no output.
            doc.output = None;
        }
        let leaves: Vec<&[Observer]> = set.observers.leaves().collect();
        let expected = scan(FUNCTION, success, &doc, leaves.iter().copied());
        let got = lookup(FUNCTION, &doc, set.accepting(success));
        assert_eq!(
            got, expected,
            "{context}: success {success}, doc {doc_value}\n  index {:?}\n  scan  {:?}",
            got.0, expected.0
        );
        set.accepting(success).candidates(&doc)
    }

    fn valid(rng: &mut Rng) -> Value {
        loop {
            let src = generator::filter(rng);
            if iii_filter::Filter::parse(&src).is_ok() {
                return src;
            }
        }
    }

    #[test]
    fn the_index_selects_what_the_evaluator_selects_on_the_shared_generator() {
        let mut docs = 0usize;
        for seed in 0..150u64 {
            let mut rng = Rng::new(40_000 + seed);
            let count = 8 + rng.below(56);
            let sources: Vec<Value> = (0..count).map(|_| valid(&mut rng)).collect();
            let mut set = CompiledSet::default();
            for (n, src) in sources.iter().enumerate() {
                set = set.with(&observer(&mut rng, &id(n), src, n as u64));
            }
            assert_eq!(set.observers.len(), count);
            for src in &sources {
                for _ in 0..6 {
                    let doc = generator::doc(&mut rng, src);
                    let success = rng.chance(70);
                    agree_on_doc(&set, &doc, success, &format!("seed {seed}"));
                    docs += 1;
                }
            }
        }
        assert!(docs > 25_000, "only {docs} documents");
    }

    /// Values equality filters and calls share: numbers in several
    /// spellings, strings that look like them, booleans, `null`.
    fn scalar(rng: &mut Rng) -> Value {
        let text = *rng.pick(&[
            "0",
            "-0",
            "0.0",
            "-0.0",
            "1",
            "1.0",
            "1e0",
            "-1",
            "2.5",
            "9007199254740992",
            "\"1\"",
            "\"\"",
            "\"a\"",
            "\"A\"",
            "\"é\"",
            "true",
            "false",
            "null",
        ]);
        serde_json::from_str(text).unwrap()
    }

    /// A string a typed part of a call holds (`error.code`, `caller.*`).
    fn word(rng: &mut Rng) -> String {
        (*rng.pick(&["a", "A", "1", "", "TIMEOUT", "checkout", "é"])).to_string()
    }

    /// A literal a filter holds: mostly a scalar, sometimes a container.
    fn literal(rng: &mut Rng, path: &str) -> Value {
        if rng.chance(6) {
            return json!([scalar(rng)]);
        }
        if rng.chance(4) {
            return json!({ "k": scalar(rng) });
        }
        if TYPED.contains(&path) && rng.chance(70) {
            return json!(word(rng));
        }
        scalar(rng)
    }

    /// The paths the corpus shares, of every root.
    const PATHS: &[&str] = &[
        "input.k",
        "input.k",
        "input.op",
        "input.row.status",
        "input.items.0",
        "output.k",
        "error.code",
        "caller.worker_name",
        "caller.function_id",
        "metadata.tenant",
    ];

    /// The paths of [`PATHS`] in a typed part of a call: strings only.
    const TYPED: &[&str] = &["error.code", "caller.worker_name", "caller.function_id"];

    /// A filter of one to three top-level clauses on shared paths: mostly
    /// `$eq` / `$in`, with other operators on the same or other paths, and
    /// now and then a combinator.
    fn equality_filter(rng: &mut Rng) -> Value {
        let mut clauses = Vec::new();
        for _ in 0..1 + rng.below(3) {
            let path = *rng.pick(PATHS);
            let operand = match rng.below(100) {
                0..45 => json!({ "$eq": literal(rng, path) }),
                45..63 => {
                    let items: Vec<Value> =
                        (0..1 + rng.below(4)).map(|_| literal(rng, path)).collect();
                    json!({ "$in": items })
                }
                63..70 => json!({ "$eq": literal(rng, path), "$ne": literal(rng, path) }),
                70..75 => json!({ "$in": [], "$exists": true }),
                75..80 => json!({ "$gte": json!(word(rng)) }),
                80..84 => json!({ "$nin": [literal(rng, path)] }),
                84..88 => json!({ "$exists": rng.chance(50) }),
                88..92 => json!({ "$prefix": word(rng) }),
                92..96 => json!({ "$any": { "$eq": literal(rng, path) } }),
                _ => {
                    let other = *rng.pick(PATHS);
                    clauses.push(json!({ "$or": [
                        { path: { "$eq": literal(rng, path) } },
                        { other: { "$eq": literal(rng, other) } },
                    ] }));
                    continue;
                }
            };
            let mut clause = Map::new();
            clause.insert(path.to_owned(), operand);
            clauses.push(Value::Object(clause));
        }
        if rng.chance(5) {
            let inner = clauses.pop().unwrap_or_else(|| json!({}));
            clauses.push(json!({ "$not": inner }));
        }
        json!({ "$and": clauses })
    }

    /// A value at a shared path of a call: mostly a scalar, sometimes a
    /// container.
    fn call_value(rng: &mut Rng) -> Value {
        match rng.below(100) {
            0..80 => scalar(rng),
            80..88 => json!([scalar(rng), scalar(rng)]),
            88..94 => json!({ "k": scalar(rng) }),
            _ => json!([]),
        }
    }

    /// One local call's parts: `input`, `output`, `error`, `caller` and
    /// `metadata`, each path present 7 times in 10.
    struct Parts {
        input: Value,
        output: Value,
        error: ObservedError,
        caller: Caller,
        metadata: Option<Value>,
        success: bool,
    }

    fn call(rng: &mut Rng) -> Parts {
        let mut input = json!({});
        for key in ["k", "op"] {
            if rng.chance(70) {
                input[key] = call_value(rng);
            }
        }
        if rng.chance(70) {
            input["row"] = json!({ "status": call_value(rng) });
        }
        if rng.chance(70) {
            input["items"] = if rng.chance(50) {
                json!([call_value(rng)])
            } else {
                json!({ "0": call_value(rng) })
            };
        }
        let mut output = json!({});
        if rng.chance(70) {
            output["k"] = call_value(rng);
        }
        let mut words = || rng.chance(70).then(|| word(rng));
        let caller = Caller {
            worker_name: words(),
            function_id: words(),
            ..Caller::default()
        };
        Parts {
            input,
            output: if rng.chance(10) { json!(null) } else { output },
            error: ObservedError {
                code: word(rng),
                message: word(rng),
            },
            caller,
            metadata: rng.chance(70).then(|| json!({ "tenant": call_value(rng) })),
            success: rng.chance(60),
        }
    }

    /// What [`select`] over every observer, [`select_compiled`] and the
    /// index of the call's outcome select agree on `parts`, as a local call
    /// is matched (its `error` and `caller` serialized only when an observer
    /// of its outcome reads them); returns how many observers the index
    /// evaluated.
    fn agree_on_call(set: &CompiledSet, parts: &Parts, context: &str) -> usize {
        let view = CallView {
            input: Some(&parts.input),
            output: parts.success.then_some(&parts.output),
            error: (!parts.success).then_some(&parts.error),
            caller: Some(&parts.caller).filter(|caller| **caller != Caller::default()),
            metadata: parts.metadata.as_ref(),
            success: parts.success,
        };
        let leaves: Vec<&[Observer]> = set.observers.leaves().collect();
        let expected = select(FUNCTION, &view, &leaves);
        let context = format!(
            "{context}: success {}, input {}, output {}, error {:?}, caller {:?}, metadata {:?}",
            parts.success, parts.input, parts.output, parts.error, parts.caller, parts.metadata
        );
        assert_eq!(
            select_compiled(FUNCTION, &view, set),
            expected,
            "{context}, plan {:?}",
            set.plan(parts.success)
        );
        let doc_parts = DocParts::of(set.reads(parts.success), view.error, view.caller);
        let doc = doc_parts.doc(view.input, view.output, view.metadata);
        let index = set.accepting(parts.success);
        assert_eq!(lookup(FUNCTION, &doc, index), expected, "{context}, index");
        index.candidates(&doc)
    }

    #[test]
    fn the_index_selects_what_the_evaluator_selects_on_shared_paths_and_values() {
        let (mut evaluated, mut held) = (0usize, 0usize);
        for seed in 0..40u64 {
            let mut rng = Rng::new(50_000 + seed);
            let count = 50 + rng.below(250);
            let mut set = CompiledSet::default();
            for n in 0..count {
                let filters = equality_filter(&mut rng);
                set = set.with(&observer(&mut rng, &id(n), &filters, n as u64));
            }
            for _ in 0..200 {
                let parts = call(&mut rng);
                evaluated += agree_on_call(&set, &parts, &format!("seed {seed}"));
                held += count;
            }
        }
        // Most observers are indexed, and a lookup rules most of them out.
        assert!(
            evaluated * 3 < held,
            "the index evaluated {evaluated} filters where a scan evaluates {held}"
        );
    }

    /// The roots the filters of `held`'s observers of an outcome read, and
    /// the plan a set of `held` has for it, counted from scratch.
    fn expected_reads_and_plan(held: &[Observer], success: bool) -> (Roots, Plan) {
        let mut reads = Roots::default();
        let (mut of_outcome, mut scanned) = (0, 0);
        for obs in held.iter().filter(|obs| obs.accepts(success)) {
            let read = obs.filter().roots();
            reads.input |= read.input;
            reads.output |= read.output;
            reads.error |= read.error;
            reads.caller |= read.caller;
            reads.metadata |= read.metadata;
            of_outcome += 1;
            scanned += usize::from(!obs.indexed());
        }
        let plan = if of_outcome == 0 {
            Plan::Nothing
        } else if held.len() <= SCAN_MAX || scanned * 2 > held.len() {
            Plan::Scan
        } else {
            Plan::Index
        };
        (reads, plan)
    }

    /// Adds, replacements (another filter, another `on`, another capture)
    /// and removals keep the index exact, and the roots and plan of each
    /// outcome, and every version a call may have loaded keeps selecting
    /// what it held.
    #[test]
    fn changes_keep_the_index_exact_and_every_version_intact() {
        for seed in 0..30u64 {
            let mut rng = Rng::new(60_000 + seed);
            let mut set = CompiledSet::default();
            let mut held: Vec<Observer> = Vec::new();
            let mut versions: Vec<CompiledSet> = Vec::new();
            for step in 0..500u64 {
                let n = rng.below(100);
                let at = held.iter().position(|obs| obs.id() == id(n));
                if rng.chance(35) {
                    if let Some(at) = at {
                        let gone = held.remove(at);
                        set = set.without(&gone).expect("held");
                    } else {
                        let absent = observer(&mut rng, &id(n), &json!({}), step);
                        assert!(set.without(&absent).is_none());
                    }
                } else {
                    let filters = if rng.chance(50) {
                        equality_filter(&mut rng)
                    } else {
                        valid(&mut rng)
                    };
                    let obs = observer(&mut rng, &id(n), &filters, step);
                    set = set.with(&obs);
                    match at {
                        Some(at) => held[at] = obs,
                        None => held.push(obs),
                    }
                }
                assert_eq!(set.observers.len(), held.len());
                let filed = |success: bool| held.iter().filter(|obs| obs.accepts(success)).count();
                assert_eq!(set.on_success.len(), filed(true), "seed {seed} step {step}");
                assert_eq!(set.on_error.len(), filed(false), "seed {seed} step {step}");
                for success in [true, false] {
                    assert_eq!(
                        (set.reads(success), set.plan(success)),
                        expected_reads_and_plan(&held, success),
                        "seed {seed} step {step} success {success}"
                    );
                }
                for _ in 0..3 {
                    let parts = call(&mut rng);
                    agree_on_call(&set, &parts, &format!("seed {seed} step {step}"));
                }
                if step % 50 == 0 {
                    versions.push(set.clone());
                }
            }
            for (n, version) in versions.iter().enumerate() {
                for _ in 0..20 {
                    let parts = call(&mut rng);
                    agree_on_call(version, &parts, &format!("seed {seed} version {n}"));
                }
            }
        }
    }

    /// A local call of a function whose 1,000 observers each filter on their
    /// own `input.tenant` evaluates one filter.
    #[test]
    fn one_of_a_thousand_equality_observers_is_one_evaluation() {
        let mut set = CompiledSet::default();
        for n in 0..1_000u64 {
            set = set.with(&Observer::test_view(
                json!({ "id": format!("tenant-{n:04}"), "target": "t", "on": "success",
                        "filters": { "$and": [{ "input.tenant": { "$eq": format!("t{n}") } }] } }),
                n,
                true,
            ));
        }
        let input = json!({ "tenant": "t417" });
        let view = CallView {
            input: Some(&input),
            output: None,
            error: None,
            caller: None,
            metadata: None,
            success: true,
        };
        assert_eq!(set.plan(true), Plan::Index);
        assert_eq!(select_compiled(FUNCTION, &view, &set).0, ["tenant-0417"]);
        let doc = Doc {
            input: Some(&input),
            ..Doc::default()
        };
        assert_eq!(set.on_success.candidates(&doc), 1);
        assert!(set.on_error.is_empty(), "no observer of failed calls");
    }

    /// An active observer of [`FUNCTION`] with `filters` and `on`.
    fn with_filters(id: &str, on: &str, filters: Value) -> Observer {
        Observer::test_view(
            json!({ "id": id, "target": "t", "on": on, "filters": filters }),
            0,
            true,
        )
    }

    fn set_of(observers: impl IntoIterator<Item = Observer>) -> CompiledSet {
        observers
            .into_iter()
            .fold(CompiledSet::default(), |set, obs| set.with(&obs))
    }

    /// An observer is indexed when its filter's top-level `$and` holds an
    /// `$eq` of a scalar, or an `$in` of scalars, on a path of the call.
    #[test]
    fn an_observer_is_indexed_when_the_index_files_its_filter() {
        for (filters, indexed) in [
            (json!({ "input.k": { "$eq": 1 } }), true),
            (json!({ "caller.worker_name": { "$eq": "w" } }), true),
            (
                json!({ "input.k": { "$in": ["a", null] }, "input.n": { "$gt": 1 } }),
                true,
            ),
            (json!({ "input.k": { "$eq": [1] } }), false),
            (json!({ "input.k": { "$in": [] } }), false),
            (json!({ "input.k": { "$in": [1, { "a": 1 }] } }), false),
            (json!({ "$or": [{ "input.k": { "$eq": 1 } }] }), false),
            (json!({ "input.k": { "$any": { "$eq": 1 } } }), false),
            (json!({ "input.k": { "$gte": 1 } }), false),
            (json!({}), false),
        ] {
            let obs = with_filters("o", "success", filters.clone());
            assert_eq!(obs.indexed(), indexed, "{filters}");
            let set = set_of([obs]);
            assert_eq!(
                set.on_success.candidates(&Doc::default()),
                usize::from(!indexed),
                "{filters}: the index scans it on every call"
            );
        }
    }

    /// A set of at most [`SCAN_MAX`] observers is scanned, and so is one in
    /// which more than half are observers of the call's outcome that the
    /// index files under no condition; any other set is matched through the
    /// index of the call's outcome, and an outcome no observer accepts has
    /// nothing to match. Each plan selects what a scan of every observer
    /// selects.
    #[test]
    fn a_set_is_scanned_when_small_or_mostly_unindexed_and_indexed_otherwise() {
        let keyed = |n: usize| {
            with_filters(
                &format!("k-{n:04}"),
                "success",
                json!({ "input.k": { "$eq": n } }),
            )
        };
        let unfiltered = |n: usize| with_filters(&format!("u-{n:04}"), "success", json!({}));
        let ranged = |n: usize| {
            with_filters(
                &format!("r-{n:04}"),
                "error",
                json!({ "input.k": { "$gte": n } }),
            )
        };

        let small = set_of((0..SCAN_MAX).map(keyed));
        assert_eq!(small.plan(true), Plan::Scan);
        assert_eq!(
            small.plan(false),
            Plan::Nothing,
            "no observer of failed calls"
        );
        let large = small.with(&keyed(SCAN_MAX));
        assert_eq!(large.plan(true), Plan::Index);
        let first = large.observers.get("k-0000").expect("held").clone();
        assert_eq!(large.without(&first).expect("held").plan(true), Plan::Scan);

        // Half of the set unindexed: the index still rules out the rest.
        let half = (0..=SCAN_MAX).fold(large.clone(), |set, n| set.with(&unfiltered(n)));
        assert_eq!(half.observers.len(), 2 * (SCAN_MAX + 1));
        assert_eq!(half.plan(true), Plan::Index);
        let most = half.with(&unfiltered(SCAN_MAX + 1));
        assert_eq!(
            most.plan(true),
            Plan::Scan,
            "the index would scan most of it"
        );

        // Deeper than one leaf, unfiltered.
        let deep = set_of((0..300).map(unfiltered));
        assert!(deep.observers.as_slice().is_none(), "several leaves");
        assert_eq!(deep.plan(true), Plan::Scan);

        // Unindexed observers of the other outcome do not count.
        let mixed = set_of((0..100).map(ranged).chain((0..20).map(keyed)));
        assert_eq!(
            mixed.plan(true),
            Plan::Index,
            "every observer of success indexed"
        );
        assert_eq!(
            mixed.plan(false),
            Plan::Scan,
            "every observer of error scanned"
        );

        let error = ObservedError {
            code: "E".into(),
            message: "m".into(),
        };
        for set in [&small, &large, &half, &most, &deep, &mixed] {
            let leaves: Vec<&[Observer]> = set.observers.leaves().collect();
            for (input, success) in [
                (json!({ "k": 3 }), true),
                (json!({ "k": SCAN_MAX }), true),
                (json!({ "k": 50 }), false),
                (json!({}), true),
                (json!({}), false),
            ] {
                let view = CallView {
                    input: Some(&input),
                    output: success.then_some(&input),
                    error: (!success).then_some(&error),
                    caller: None,
                    metadata: None,
                    success,
                };
                assert_eq!(
                    select_compiled(FUNCTION, &view, set),
                    select(FUNCTION, &view, &leaves),
                    "{input} success {success}, plan {:?}",
                    set.plan(success)
                );
            }
        }
    }

    /// A local call builds its `error` and `caller` only when a filter of an
    /// observer of its outcome reads them: a call that succeeded does not
    /// serialize its caller for an observer of failed calls, as observers
    /// come, change their `on` and go.
    #[test]
    fn a_call_builds_what_the_observers_of_its_outcome_read() {
        let by_caller = |on: &str| {
            with_filters(
                "by-caller",
                on,
                json!({ "caller.worker_name": { "$eq": "w" } }),
            )
        };
        let set = set_of([
            by_caller("error"),
            with_filters("a", "success", json!({ "input.n": { "$eq": 1 } })),
            with_filters("b", "success", json!({ "input.n": { "$eq": 2 } })),
        ]);
        assert!(set.roots.caller, "a filter of the set reads it");
        assert!(set.reads(true).input && !set.reads(true).caller);
        assert!(set.reads(false).caller && !set.reads(false).input);

        let caller = Caller {
            worker_name: Some("w".into()),
            ..Caller::default()
        };
        let error = ObservedError {
            code: "E".into(),
            message: "m".into(),
        };
        let built = |set: &CompiledSet, success: bool| {
            let error = (!success).then_some(&error);
            DocParts::of(set.reads(success), error, Some(&caller))
                .doc(None, None, None)
                .caller
                .is_some()
        };
        assert!(!built(&set, true), "a call that succeeded");
        assert!(built(&set, false), "a call that failed");
        let input = json!({ "n": 1 });
        let view = CallView {
            input: Some(&input),
            output: Some(&input),
            error: None,
            caller: Some(&caller),
            metadata: None,
            success: true,
        };
        assert_eq!(select_compiled(FUNCTION, &view, &set).0, ["a"]);

        // The same observer, of every call, then gone.
        let complete = by_caller("complete");
        let set = set.with(&complete);
        assert!(built(&set, true) && built(&set, false));
        let set = set.without(&complete).expect("held");
        assert!(!built(&set, true) && !built(&set, false));
        assert!(!set.roots.caller && set.reads(true).input);
    }
}

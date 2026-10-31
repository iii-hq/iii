// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! The routed hook, reports, calls through an observer and activations
//! (protocol.md §3.2, §3.4; v2 §4.6).
//!
//! `InvocationHandler::handle_invocation_inner` asks [`ObserverRegistry::route`]
//! before running a handler: one atomic load and, only when some function is
//! observed or hosted privately by a connection, one hash lookup. Unobserved,
//! nothing is copied. Observed, the hook keeps the parts of the invocation the
//! active observers need, and once the result is known
//! [`ObserverRegistry::complete`] hands it to an evaluator task, so the
//! caller's reply never waits for filters or activations. There is one
//! evaluator per runtime worker thread, and a function's completions always go
//! to the same one, which handles them in the order they completed: an
//! observer watches one function, so its activations stay in engine arrival
//! order while different functions are evaluated in parallel.
//!
//! Who matches a call's observers (v2 §8.2, decision 2): a host that declared
//! `observe.v2` delivers its own calls, listing the observers its delivery
//! matched on each routed answer (`invocationresult.observed`) and reporting
//! its local calls (`invocationsobserved`); the engine activates those. A
//! routed call of such a host that ends without its answer (its caller left,
//! the engine could not send it, the host went away) activates nothing: each
//! of its observers counts a `CANCELLED` drop. The engine evaluates the
//! observers' `on` and filters itself for every other call: to a host without
//! `observe.v2`, and to an engine-hosted or HTTP-invoked function.
//!
//! A matched event is shared, not copied, by the observers it activates: a
//! queued activation holds a pointer to it, and the target's argument is built
//! when the activation starts.

use std::{
    collections::HashSet,
    sync::{
        Arc,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use iii_filter::Doc;
use opentelemetry::KeyValue;
use serde_json::{Map, Value};
use tokio::sync::{mpsc, oneshot::error::RecvError};
use uuid::Uuid;

use super::{
    FnKey, ObserverRegistry,
    context::{ENGINE_CALLER_NAME, InvocationCtx},
    lifecycle::{
        CANCELLED, INVALID_REPORT, Observer, PendingActivation, failure_reasons, lock, now_ms,
    },
    owner::{ActiveSet, Needs},
};
use crate::{
    protocol::{
        Activation, Caller, Deliver, DroppedReportStats, ErrorBody, InvocationStatus,
        LocalFunctionStats, ObservedItem, ObservedItems, On, observe_codes,
    },
    worker_connections::WorkerConnection,
};

/// An invocation of an observed function, from before its handler runs until
/// its result is known. Dropped without a result (the invocation was
/// cancelled: its caller gave up, e.g. an activation timeout), it counts a
/// `CANCELLED` drop for each observer that was waiting for it, since nobody
/// can know which of them would have matched.
pub struct ObservedCall {
    inner: Option<Tracked>,
}

struct Tracked {
    set: Arc<ActiveSet>,
    /// The function's host delivers its calls itself: only its answer
    /// activates the call's observers.
    host_delivers: bool,
    /// What the call keeps (see [`ActiveSet::needs_for`]).
    needs: Needs,
    /// The evaluator had room when the call began: what it needs was kept.
    admitted: bool,
    started: Instant,
    started_at_ms: u64,
    input: Option<Arc<Value>>,
    metadata: Option<Arc<Value>>,
    traceparent: Option<String>,
    baggage: Option<String>,
}

impl Drop for ObservedCall {
    fn drop(&mut self) {
        if let Some(cancelled) = self.inner.take() {
            for observer in &cancelled.set.observers {
                drop_event(observer, CANCELLED);
            }
        }
    }
}

/// Completions waiting for one evaluator. Past this, or past
/// [`EVALUATOR_BYTES`] of payloads waiting for all of them, a completion is
/// not evaluated (nor its payload copied): each active observer of the
/// function counts an `OVERFLOW` drop.
pub(crate) const EVALUATOR_QUEUE: usize = 65_536;

/// Approximate bytes of payloads waiting for the evaluators. A completion
/// whose payloads alone are larger never fits, however idle the evaluators:
/// a `TOO_LARGE` drop, not an `OVERFLOW`.
pub(crate) const EVALUATOR_BYTES: usize = 64 * 1024 * 1024;

/// Evaluators at most, whatever the number of runtime worker threads.
const MAX_EVALUATORS: usize = 16;

/// How often drop counts reach `iii.observer.dropped.total` (see
/// [`ObserverRegistry::export_drops`]).
pub(crate) const DROP_EXPORT_INTERVAL: Duration = Duration::from_secs(1);

/// How long a reporting connection waits for room in the queues its report
/// overflowed before the engine reads from it again (see
/// [`ObserverRegistry::wait_for_room`]).
pub(crate) const REPORT_BACKPRESSURE_WAIT: Duration = Duration::from_millis(250);

/// A hold of a reporting connection is followed by at least
/// `1 / REPORT_HOLD_PER_READ` of its length spent reading before the next one
/// (see [`ReportHold`]).
pub(crate) const REPORT_HOLD_PER_READ: u32 = 4;

/// When a reporting connection may be held again, kept by its reader (see
/// [`ObserverRegistry::wait_for_room`]). After a hold the engine reads the
/// connection for at least a quarter as long ([`REPORT_HOLD_PER_READ`]), and
/// a report that overflows meanwhile is dropped (`OVERFLOW`) without holding
/// it. So the connection is held at most four fifths of the time, and what it
/// sends besides reports (its calls, its answers to calls) waits at most
/// [`REPORT_BACKPRESSURE_WAIT`] plus five times the time the engine takes to
/// read the reports ahead of it, however long they keep overflowing. Reading
/// meanwhile means decoding reports only to drop them: reading half the time
/// cost a one-vCPU engine about a third of its activations in a report flood.
#[derive(Debug, Default)]
pub struct ReportHold {
    not_before: Option<tokio::time::Instant>,
}

/// How many of an `observe.v2` host's last answered routed calls
/// [`RoutedAnswers`] keeps: several report lingers' worth of answers (an SDK
/// flushes its report at least every 5 ms) at any rate one connection
/// answers.
pub(crate) const ROUTED_ANSWERS_KEPT: usize = 4096;

/// The routed calls an `observe.v2` host answered last, by invocation id,
/// [`ROUTED_ANSWERS_KEPT`] at most, kept by its connection: a report item
/// naming one is dropped as `INVALID_REPORT` after the answer as well as
/// before it (decision 38).
#[derive(Debug, Default)]
pub struct RoutedAnswers {
    kept: std::sync::Mutex<KeptIds>,
}

#[derive(Debug, Default)]
struct KeptIds {
    ids: HashSet<Uuid>,
    /// `ids` in the order they were kept, oldest first.
    order: std::collections::VecDeque<Uuid>,
}

impl RoutedAnswers {
    /// Keeps `invocation_id`, forgetting the oldest past
    /// [`ROUTED_ANSWERS_KEPT`].
    pub fn record(&self, invocation_id: Uuid) {
        let mut kept = lock(&self.kept);
        if !kept.ids.insert(invocation_id) {
            return;
        }
        kept.order.push_back(invocation_id);
        if kept.order.len() > ROUTED_ANSWERS_KEPT
            && let Some(oldest) = kept.order.pop_front()
        {
            kept.ids.remove(&oldest);
        }
    }

    fn contains(&self, invocation_id: &Uuid) -> bool {
        lock(&self.kept).ids.contains(invocation_id)
    }
}

/// Queue slots an observer keeps once its queue drains; a burst that grew it
/// past this gives the rest back.
const QUEUE_KEPT: usize = 64;

/// Approximate bytes of the events waiting in observers' queues, all queues
/// together, each event counted once however many queues hold it. Past it, an
/// activation that would queue is dropped (`OVERFLOW`) as if its own queue
/// were full: `max_queue_per_observer` bounds one queue, this bounds them all.
/// An event larger than all of it never queues (`TOO_LARGE`).
pub(crate) const QUEUED_BYTES: usize = 64 * 1024 * 1024;

/// How many refused sends [`ObserverRegistry::refused_send`] remembers:
/// enough for each refused activation to find its own, however many other
/// calls were refused meanwhile.
const REFUSED_SENDS_KEPT: usize = 256;

/// What a queued event costs besides its payloads: ids, names, the structures
/// that hold them.
const EVENT_OVERHEAD: usize = 256;

/// The observers' queues together, against [`QUEUED_BYTES`].
pub(crate) struct QueueBudget {
    /// Approximate bytes of the events queued now.
    bytes: AtomicUsize,
    limit: AtomicUsize,
    /// Wakes readers waiting for room ([`ObserverRegistry::wait_for_room`]):
    /// a queue or the budget is down to half, or an observer left `active`.
    pub(crate) room: tokio::sync::Notify,
}

impl Default for QueueBudget {
    fn default() -> Self {
        Self {
            bytes: AtomicUsize::new(0),
            limit: AtomicUsize::new(QUEUED_BYTES),
            room: tokio::sync::Notify::new(),
        }
    }
}

impl QueueBudget {
    /// Charges `event` for one more queue holding it: its cost the first
    /// time, nothing after. `false` when the first charge does not fit.
    fn charge(&self, event: &Completed) -> bool {
        if event.queues.fetch_add(1, Ordering::AcqRel) > 0 {
            return true;
        }
        let cost = event.cost();
        let limit = self.limit.load(Ordering::Relaxed);
        let fits = self
            .bytes
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |bytes| {
                bytes.checked_add(cost).filter(|total| *total <= limit)
            })
            .is_ok();
        // Kept either way: what `too_large` reads.
        event.charged.store(cost, Ordering::Release);
        if !fits {
            event.queues.fetch_sub(1, Ordering::AcqRel);
        }
        fits
    }

    /// `(cost, limit)` when `event` alone is larger than the whole budget:
    /// it never queues, however empty the queues. Known only once a charge
    /// computed its cost.
    fn too_large(&self, event: &Completed) -> Option<(usize, usize)> {
        let cost = event.charged.load(Ordering::Acquire);
        let limit = self.limit.load(Ordering::Relaxed);
        (cost > limit).then_some((cost, limit))
    }

    /// One queue let go of `event`; the last one gives its cost back.
    pub(crate) fn release(&self, event: &Completed) {
        if event.queues.fetch_sub(1, Ordering::AcqRel) != 1 {
            return;
        }
        let cost = event.charged.load(Ordering::Acquire);
        let half = self.limit.load(Ordering::Relaxed) / 2;
        let before = self.bytes.fetch_sub(cost, Ordering::AcqRel);
        if before > half && before - cost <= half {
            self.room.notify_waiters();
        }
    }

    /// At most half full: a waiting reader may read on.
    fn has_room(&self) -> bool {
        self.bytes.load(Ordering::Acquire) <= self.limit.load(Ordering::Relaxed) / 2
    }
}

/// How an engine-hosted source's event is evaluated.
#[derive(Debug, Default, Clone)]
pub struct Emission {
    /// The depth of what caused the event; `None`: the running invocation's.
    pub depth: Option<u32>,
    /// The trace its activations run in; `None`: a new one each.
    pub traceparent: Option<String>,
}

/// A JSON value's size, roughly: what holding it costs, not its encoding.
fn approx_size(value: &Value) -> usize {
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => 16,
        Value::String(s) => 24 + s.len(),
        Value::Array(items) => 24 + items.iter().map(approx_size).sum::<usize>(),
        Value::Object(map) => {
            24 + map
                .iter()
                .map(|(key, value)| 24 + key.len() + approx_size(value))
                .sum::<usize>()
        }
    }
}

/// `delivery` label: the engine evaluated the call.
const ENGINE_DELIVERY: &str = "engine";
/// `delivery` label: the call's host reported the match.
const HOST_DELIVERY: &str = "host";
/// `delivery` label: a call through an observer.
const OBSERVER_CALL_DELIVERY: &str = "observer_call";

/// The `function_id` label of a drop that names no observer the engine
/// knows, so no observed function.
const UNKNOWN_FUNCTION: &str = "<unknown>";

/// The stamp the engine adds to the payload of every call a worker makes:
/// the calling connection's id.
const CALLER_STAMP: &str = "_caller_worker_id";

/// `value` as observers receive it: without the engine's top-level
/// `_caller_worker_id` stamp (decision 40). The caller is in `caller`.
fn unstamped(mut value: Value) -> Value {
    if let Value::Object(map) = &mut value {
        map.remove(CALLER_STAMP);
    }
    value
}

/// What a host that declared `observe.v2` answered a routed call with besides
/// its result (`invocationresult.observed`, v2 §8.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostDelivery {
    /// The connection that answered.
    pub host: Uuid,
    /// The observers its delivery matched, as listed: empty when it matched
    /// none, or when the function delivers its calls itself.
    pub observed: Vec<String>,
}

/// Which of a completion's active observers it activates.
enum Matching {
    /// Those whose `on` and filters match it: the engine evaluates the call.
    Filters,
    /// Those the call's host listed (sorted, each once) whose `on` takes its
    /// status: the host evaluated the call.
    Listed(Vec<String>),
}

impl Matching {
    /// The `delivery` label of the activations it starts.
    fn delivery(&self) -> &'static str {
        match self {
            Self::Filters => ENGINE_DELIVERY,
            Self::Listed(_) => HOST_DELIVERY,
        }
    }

    /// Whether the completion may activate `observer`: what a drop of the
    /// whole completion is charged to.
    fn concerns(&self, observer: &Observer) -> bool {
        match self {
            Self::Filters => true,
            Self::Listed(ids) => is_listed(ids, &observer.id),
        }
    }
}

/// Whether the sorted `ids` hold `id`.
fn is_listed(ids: &[String], id: &str) -> bool {
    ids.binary_search_by(|listed| listed.as_str().cmp(id))
        .is_ok()
}

/// A completed observed invocation (or a reported one), as its activations
/// see it. Payloads are shared: each activation clones what it ships.
pub(crate) struct Completed {
    /// Who matched it, as the `delivery` label of
    /// `iii.observer.activations.total`: [`ENGINE_DELIVERY`] or
    /// [`HOST_DELIVERY`].
    delivery: &'static str,
    namespace: String,
    function_id: String,
    invocation_id: String,
    success: bool,
    input: Option<Arc<Value>>,
    output: Option<Arc<Value>>,
    error: Option<Arc<Value>>,
    caller: Option<Arc<Value>>,
    metadata: Option<Arc<Value>>,
    started_at_ms: u64,
    duration_ms: f64,
    depth: u32,
    traceparent: Option<String>,
    baggage: Option<String>,
    /// Queues holding it now (see [`QueueBudget`]).
    queues: AtomicUsize,
    /// What it was charged when the first of them took it (or would have
    /// been, when that did not fit).
    charged: AtomicUsize,
}

impl Completed {
    /// Roughly what holding the event costs (see [`QUEUED_BYTES`]).
    fn cost(&self) -> usize {
        let payloads: usize = [
            &self.input,
            &self.output,
            &self.error,
            &self.caller,
            &self.metadata,
        ]
        .into_iter()
        .filter_map(|value| value.as_deref())
        .map(approx_size)
        .sum();
        let text = [&self.traceparent, &self.baggage]
            .into_iter()
            .filter_map(|value| value.as_deref())
            .map(str::len)
            .sum::<usize>();
        EVENT_OVERHEAD
            + self.namespace.len()
            + self.function_id.len()
            + self.invocation_id.len()
            + text
            + payloads
    }
}

/// One routed completion waiting for the evaluator.
pub(crate) struct Evaluation {
    /// Approximate size of the payloads it holds, against the evaluator's
    /// byte budget.
    bytes: usize,
    set: Arc<ActiveSet>,
    matching: Matching,
    completed: Completed,
    /// Turned into the document's `error` only when an observer needs it.
    error: Option<ErrorBody>,
    /// Resolved into the document's `caller` only when an observer needs it.
    ctx: Arc<InvocationCtx>,
}

impl Evaluation {
    /// The active observers it may activate.
    fn concerned(&self) -> impl Iterator<Item = &Arc<Observer>> {
        self.set
            .observers
            .iter()
            .filter(|observer| self.matching.concerns(observer))
    }
}

/// An observer's in-flight slot. Released on drop unless handed on, so a
/// panic while delivering cannot leak the slot.
struct Slot {
    registry: Arc<ObserverRegistry>,
    observer: Arc<Observer>,
    held: bool,
}

impl Drop for Slot {
    fn drop(&mut self) {
        if self.held {
            self.registry.release_slot(&self.observer);
        }
    }
}

impl ObserverRegistry {
    /// The active observers of `(namespace, function_id)`, when it has any:
    /// what [`Self::route`] answers, without the caller check. One atomic
    /// load while nothing is observed or hosted privately by a connection,
    /// and otherwise one more hash lookup.
    #[inline]
    pub fn watch(&self, namespace: &str, function_id: &str) -> Option<Arc<ActiveSet>> {
        let view = self.routed.load();
        if view.is_empty() {
            return None;
        }
        view.get(&(namespace, function_id) as &dyn crate::function::FunctionKey)
            .and_then(|routed| routed.active.load_full())
    }

    /// Whether `(namespace, function_id)` has an active observer: what an
    /// event source checks before building an event.
    #[inline]
    pub fn is_observed(&self, namespace: &str, function_id: &str) -> bool {
        self.watch(namespace, function_id).is_some()
    }

    /// One call of an engine-hosted source (see [`super::events`]): evaluated
    /// like a successful invocation of `(namespace, function_id)` whose input
    /// and output are both the event (the source's handler returns it), with
    /// `{ worker_name: "iii-engine" }` as the caller and the depth of the
    /// invocation that caused it. `build` runs only when an active observer
    /// needs the event and the evaluator has room; unobserved, this costs what
    /// [`Self::watch`] costs.
    pub fn emit(
        self: &Arc<Self>,
        namespace: &str,
        function_id: &str,
        build: impl FnOnce() -> Option<Value>,
    ) {
        self.emit_with(namespace, function_id, Emission::default(), build);
    }

    /// [`Self::emit`] with the event's depth and trace given (an event
    /// produced away from the invocation that caused it: a log record
    /// ingested later, a coalesced burst).
    pub fn emit_with(
        self: &Arc<Self>,
        namespace: &str,
        function_id: &str,
        emission: Emission,
        build: impl FnOnce() -> Option<Value>,
    ) {
        if let Some(evaluation) = self.emission(namespace, function_id, emission, build) {
            self.enqueue(evaluation);
        }
    }

    /// The evaluation of one call of the engine-hosted source `(namespace,
    /// function_id)` (see [`Self::emit_with`]), when it has active observers
    /// and the evaluator has room.
    pub(super) fn emission(
        &self,
        namespace: &str,
        function_id: &str,
        emission: Emission,
        build: impl FnOnce() -> Option<Value>,
    ) -> Option<Evaluation> {
        let set = self.watch(namespace, function_id)?;
        if !self.evaluator_has_room(&set) {
            for observer in &set.observers {
                drop_event(observer, observe_codes::OVERFLOW);
            }
            return None;
        }
        let event = if set.needs.input || set.needs.output {
            self.events_built.fetch_add(1, Ordering::Relaxed);
            Some(Arc::new(build()?))
        } else {
            None
        };
        let depth = emission.depth.unwrap_or_else(InvocationCtx::current_depth);
        Some(Evaluation {
            bytes: event.as_deref().map_or(0, approx_size),
            completed: Completed {
                delivery: ENGINE_DELIVERY,
                namespace: namespace.to_string(),
                function_id: function_id.to_string(),
                invocation_id: super::new_uuid().to_string(),
                success: true,
                input: event.clone().filter(|_| set.needs.input),
                output: event.filter(|_| set.needs.output),
                error: None,
                caller: None,
                metadata: None,
                started_at_ms: now_ms(),
                duration_ms: 0.0,
                depth,
                traceparent: emission.traceparent,
                baggage: None,
                queues: AtomicUsize::new(0),
                charged: AtomicUsize::new(0),
            },
            set,
            matching: Matching::Filters,
            error: None,
            ctx: Arc::new(InvocationCtx::engine_source(depth)),
        })
    }

    /// Starts tracking an observed invocation: copies the input and metadata
    /// only when an active observer needs them, and only when the evaluator
    /// has room (otherwise the call counts an `OVERFLOW` drop when it
    /// completes, without having copied anything). The input is kept whoever
    /// will match the call's observers: the envelope of an observer its host
    /// lists is built from it too. Whether its host or the engine matches
    /// them is decided when the call begins, from the host it goes to.
    ///
    /// Observers see the payload as the caller sent it: a call a worker makes
    /// carries the engine's `_caller_worker_id` stamp, which the copy leaves
    /// out (the caller is `caller.worker_id`).
    pub fn begin(
        &self,
        set: Arc<ActiveSet>,
        body: &Value,
        metadata: &Option<Value>,
        traceparent: &Option<String>,
        baggage: &Option<String>,
    ) -> ObservedCall {
        let admitted = self.evaluator_has_room(&set);
        let host_delivers = self.host_delivers(&set.function);
        let needs = set.needs_for(host_delivers);
        let input = (admitted && needs.input).then(|| {
            self.payload_copies.fetch_add(1, Ordering::Relaxed);
            Arc::new(unstamped(body.clone()))
        });
        let metadata = if admitted && needs.metadata {
            metadata.clone().map(Arc::new)
        } else {
            None
        };
        ObservedCall {
            inner: Some(Tracked {
                set,
                host_delivers,
                needs,
                admitted,
                started: Instant::now(),
                started_at_ms: now_ms(),
                input,
                metadata,
                traceparent: traceparent.clone(),
                baggage: baggage.clone(),
            }),
        }
    }

    /// `(running, queued)` activations of observer `id`, for introspection.
    pub fn in_flight(&self, id: &str) -> Option<(u32, usize)> {
        self.get(id).map(|observer| {
            let flow = lock(&observer.flow);
            (flow.running, flow.queue.len())
        })
    }

    /// How many times the routed hook copied an invocation's input. Zero as
    /// long as nothing that needs the input is observed.
    pub fn payload_copies(&self) -> u64 {
        self.payload_copies.load(Ordering::Relaxed)
    }

    /// Events an event source built because an observer needed them. Zero
    /// as long as nothing observes the event sources.
    pub fn events_built(&self) -> u64 {
        self.events_built.load(Ordering::Relaxed)
    }

    /// Observed routed invocations the evaluators handled so far: evaluated,
    /// or activated from the observers their host listed.
    pub fn evaluated(&self) -> u64 {
        self.evaluated.load(Ordering::Acquire)
    }

    /// Times a reporting connection's reader waited for room in a queue its
    /// report overflowed (see [`Self::wait_for_room`]).
    pub fn report_waits(&self) -> u64 {
        self.report_waits.load(Ordering::Relaxed)
    }

    /// Hands the completed invocation to the evaluator: the caller's reply
    /// goes out without waiting for filters or activations. Keeps the output
    /// only when an active observer needs it.
    ///
    /// `delivered` is what an `observe.v2` host answered besides the result:
    /// the call's observers are then the ones it listed, each activated once
    /// it is checked to be an active observer of the function (decisions 3,
    /// 36). Its result is their `output`, and its `error` their `error`. An
    /// answer from a connection that does not host the function activates
    /// nothing it lists (`REPORTER_NOT_OWNER`). Without `delivered`, a call
    /// whose host delivers its calls ended without its answer: each active
    /// observer counts a `CANCELLED` drop, since only the host knows which of
    /// them would have matched. The engine evaluates the observers of every
    /// other call itself.
    #[allow(clippy::too_many_arguments)]
    pub fn complete(
        self: &Arc<Self>,
        mut call: ObservedCall,
        namespace: &str,
        function_id: &str,
        invocation_id: Uuid,
        result: &Result<Result<Option<Value>, ErrorBody>, RecvError>,
        delivered: Option<HostDelivery>,
        ctx: &Arc<InvocationCtx>,
    ) {
        let Some(call) = call.inner.take() else {
            return;
        };
        let matching = match delivered {
            None if call.host_delivers => {
                for observer in &call.set.observers {
                    drop_event(observer, CANCELLED);
                }
                return;
            }
            None => Matching::Filters,
            Some(HostDelivery { host, observed }) => {
                if observed.is_empty() {
                    return;
                }
                if !self.hosts(host, namespace, function_id) {
                    let key = (namespace.to_string(), function_id.to_string());
                    self.drop_matched(&key, &observed, observe_codes::REPORTER_NOT_OWNER);
                    return;
                }
                let mut ids = observed;
                ids.sort_unstable();
                ids.dedup();
                Matching::Listed(ids)
            }
        };
        if !call.admitted || !self.evaluator_has_room(&call.set) {
            for observer in call.set.observers.iter().filter(|o| matching.concerns(o)) {
                drop_event(observer, observe_codes::OVERFLOW);
            }
            return;
        }
        let needs = call.needs;
        let (success, output, error) = match result {
            Ok(Ok(output)) => (
                true,
                if needs.output {
                    output.clone().map(|output| Arc::new(unstamped(output)))
                } else {
                    None
                },
                None,
            ),
            Ok(Err(error)) => (false, None, needs.error.then(|| error.clone())),
            Err(err) => (
                false,
                None,
                needs
                    .error
                    .then(|| ErrorBody::new("invocation_error", err.to_string())),
            ),
        };
        let bytes =
            call.input.as_deref().map_or(0, approx_size) + output.as_deref().map_or(0, approx_size);
        let evaluation = Evaluation {
            bytes,
            completed: Completed {
                delivery: matching.delivery(),
                namespace: namespace.to_string(),
                function_id: function_id.to_string(),
                invocation_id: invocation_id.to_string(),
                success,
                input: call.input,
                output,
                error: None,
                caller: None,
                metadata: call.metadata,
                started_at_ms: call.started_at_ms,
                duration_ms: call.started.elapsed().as_secs_f64() * 1000.0,
                depth: ctx.observe_depth,
                traceparent: call.traceparent,
                baggage: call.baggage,
                queues: AtomicUsize::new(0),
                charged: AtomicUsize::new(0),
            },
            set: call.set,
            matching,
            error,
            ctx: ctx.clone(),
        };
        self.enqueue(evaluation);
    }

    /// Whether `connection` hosts `(namespace, function_id)` now.
    fn hosts(&self, connection: Uuid, namespace: &str, function_id: &str) -> bool {
        self.deps.get().is_some_and(|deps| {
            deps.function_owners
                .get(&(namespace, function_id) as &dyn crate::function::FunctionKey)
                .is_some_and(|owner| owner.0 == connection)
        })
    }

    /// The observers a host listed for a call of `(namespace, function_id)`
    /// that had no active observer when it began: the engine kept nothing to
    /// build their events from, so each is counted as `INACTIVE`.
    pub(crate) fn not_held(&self, namespace: &str, function_id: &str, observed: &[String]) {
        for id in observed {
            self.drop_not_held(namespace, function_id, id);
        }
    }

    /// One id a host listed that the call cannot activate: an observer of the
    /// function that was not active when the call began, or no observer of
    /// it (gone since, or another function's). `INACTIVE`, as for a report
    /// item naming one.
    fn drop_not_held(&self, namespace: &str, function_id: &str, id: &str) {
        match self
            .get(id)
            .filter(|o| o.function.0 == namespace && o.function.1 == function_id)
        {
            Some(observer) => drop_event(&observer, observe_codes::INACTIVE),
            None => count_function_drop(function_id, observe_codes::INACTIVE, 1),
        }
    }

    /// Whether `set`'s evaluator can take one more completion: under both
    /// its count ([`EVALUATOR_QUEUE`]) and the evaluators' byte budget
    /// ([`EVALUATOR_BYTES`], approximate payload sizes).
    fn evaluator_has_room(&self, set: &ActiveSet) -> bool {
        self.evaluators
            .get()
            .is_none_or(|evaluators| evaluator_of(evaluators, set).capacity() > 0)
            && self.queued_bytes.load(Ordering::Acquire)
                < self.evaluator_bytes.load(Ordering::Relaxed)
    }

    /// Hands `evaluation` to its function's evaluator task; inline when there
    /// is none. Past the queue's bounds the completion is dropped as
    /// `OVERFLOW` for each active observer of the function; larger than the
    /// whole byte budget, as `TOO_LARGE`.
    fn enqueue(self: &Arc<Self>, evaluation: Evaluation) {
        if let Some(evaluation) = self.try_enqueue(evaluation) {
            self.evaluate(evaluation);
        }
    }

    /// [`Self::enqueue`], except that an evaluation no evaluator task can
    /// take is returned, for the caller to evaluate (see
    /// [`ObserverRegistry::evaluate`]) once it holds no lock an activation
    /// may take. A function's evaluations reach its evaluator in the order
    /// they are handed over.
    pub(super) fn try_enqueue(self: &Arc<Self>, evaluation: Evaluation) -> Option<Evaluation> {
        let Some(evaluators) = self.evaluators() else {
            return Some(evaluation);
        };
        let evaluator = evaluator_of(evaluators, &evaluation.set);
        let bytes = evaluation.bytes;
        let budget = self.evaluator_bytes.load(Ordering::Relaxed);
        if bytes > budget {
            for observer in evaluation.concerned() {
                drop_too_large(observer, bytes, budget, TooLarge::Held);
            }
            return None;
        }
        // Reserved, not checked then added: concurrent completions cannot
        // overshoot the budget together.
        let reserved = self
            .queued_bytes
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |queued| {
                queued.checked_add(bytes).filter(|total| *total <= budget)
            })
            .is_ok();
        if !reserved {
            for observer in evaluation.concerned() {
                drop_event(observer, observe_codes::OVERFLOW);
            }
            return None;
        }
        match evaluator.try_send(evaluation) {
            Ok(()) => None,
            Err(mpsc::error::TrySendError::Full(evaluation)) => {
                self.queued_bytes.fetch_sub(bytes, Ordering::AcqRel);
                for observer in evaluation.concerned() {
                    drop_event(observer, observe_codes::OVERFLOW);
                }
                None
            }
            Err(mpsc::error::TrySendError::Closed(evaluation)) => {
                self.queued_bytes.fetch_sub(bytes, Ordering::AcqRel);
                Some(evaluation)
            }
        }
    }

    /// The evaluator tasks' queues, started on first use: one per runtime
    /// worker thread, up to [`MAX_EVALUATORS`].
    fn evaluators(self: &Arc<Self>) -> Option<&[mpsc::Sender<Evaluation>]> {
        if let Some(evaluators) = self.evaluators.get() {
            return Some(evaluators);
        }
        let runtime = self.runtime()?;
        let count = runtime.metrics().num_workers().clamp(1, MAX_EVALUATORS);
        let capacity = self.evaluator_capacity.load(Ordering::Relaxed).max(1);
        let (senders, receivers): (Vec<_>, Vec<_>) = (0..count)
            .map(|_| mpsc::channel::<Evaluation>(capacity))
            .unzip();
        if self.evaluators.set(senders).is_ok() {
            for mut rx in receivers {
                let registry = Arc::downgrade(self);
                runtime.spawn(async move {
                    while let Some(evaluation) = rx.recv().await {
                        let Some(registry) = registry.upgrade() else {
                            return;
                        };
                        registry
                            .queued_bytes
                            .fetch_sub(evaluation.bytes, Ordering::AcqRel);
                        registry.evaluate(evaluation);
                    }
                });
            }
        }
        self.evaluators.get().map(Vec::as_slice)
    }

    /// Activates the observers one completed invocation matched: evaluating
    /// the active observers' `on` and filters, or taking the ones its host
    /// listed.
    pub(super) fn evaluate(self: &Arc<Self>, evaluation: Evaluation) {
        let Evaluation {
            set,
            matching,
            mut completed,
            error,
            ctx,
            ..
        } = evaluation;
        let needs = set.needs_for(matches!(matching, Matching::Listed(_)));
        completed.error = error
            .and_then(|e| serde_json::to_value(e).ok())
            .map(Arc::new);
        if needs.caller {
            completed.caller = ctx
                .caller()
                .and_then(|c| serde_json::to_value(c).ok())
                .map(Arc::new);
        }
        let completed = Arc::new(completed);
        match matching {
            Matching::Filters => {
                let doc = Doc {
                    input: completed.input.as_deref(),
                    output: completed.output.as_deref(),
                    error: completed.error.as_deref(),
                    caller: completed.caller.as_deref(),
                    metadata: completed.metadata.as_deref(),
                };
                for leaf in set.observers.leaves() {
                    for observer in leaf {
                        if accepts(&observer.on, completed.success) && observer.filter.matches(&doc)
                        {
                            self.activate(observer, &completed);
                        }
                    }
                }
            }
            Matching::Listed(ids) => {
                let mut held = vec![false; ids.len()];
                for leaf in set.observers.leaves() {
                    for observer in leaf {
                        let Ok(at) = ids.binary_search_by(|id| id.as_str().cmp(&observer.id))
                        else {
                            continue;
                        };
                        held[at] = true;
                        if accepts(&observer.on, completed.success) {
                            self.activate(observer, &completed);
                        }
                    }
                }
                for (id, held) in ids.iter().zip(held) {
                    if !held {
                        self.drop_not_held(&completed.namespace, &completed.function_id, id);
                    }
                }
            }
        }
        self.evaluated.fetch_add(1, Ordering::AcqRel);
    }

    /// Policy for one matched observer (protocol.md §3.4): state, loop guard,
    /// expiry, `max_fires`, then an in-flight slot or the bounded queue.
    /// Returns whether the event was dropped because the queue was full.
    fn activate(self: &Arc<Self>, observer: &Arc<Observer>, completed: &Arc<Completed>) -> bool {
        if !observer.is_active() {
            drop_event(observer, observe_codes::INACTIVE);
            return false;
        }
        let max_depth = self.config.load().max_observe_depth;
        if completed.depth.saturating_add(1) > max_depth {
            drop_event(observer, observe_codes::LOOP_GUARD);
            warn_loop_guard(observer, completed.depth, max_depth);
            return false;
        }
        if observer.is_expired() {
            self.dispose(
                observer,
                observe_codes::EXPIRED,
                "the observer's expires_in_ms elapsed",
            );
            drop_event(observer, observe_codes::INACTIVE);
            return false;
        }
        let Some(fired) = take_fire(observer) else {
            return false;
        };
        let run_now = {
            let mut flow = lock(&observer.flow);
            if flow.running < observer.policy.max_in_flight && flow.queue.is_empty() {
                flow.running += 1;
                Some(PendingActivation::running(completed.clone()))
            } else if flow.queue.len() < observer.policy.max_queue
                && self.queue_budget.charge(completed)
            {
                flow.queue.push_back(PendingActivation::queued(
                    completed.clone(),
                    self.queue_budget.clone(),
                ));
                None
            } else {
                drop(flow);
                observer.fires.fetch_sub(1, Ordering::AcqRel);
                if let Some((cost, limit)) = self.queue_budget.too_large(completed) {
                    drop_too_large(observer, cost, limit, TooLarge::Queued);
                    return false;
                }
                drop_event(observer, observe_codes::OVERFLOW);
                return true;
            }
        };
        if let Some(pending) = run_now {
            match self.runtime() {
                Some(runtime) => {
                    runtime.spawn(run_activations(self.clone(), observer.clone(), pending));
                }
                None => self.release_slot(observer),
            }
        }
        if observer.policy.max_fires == Some(fired) {
            self.dispose(
                observer,
                observe_codes::MAX_FIRES,
                format!("the observer fired {fired} times, its max_fires"),
            );
        }
        false
    }

    /// Invokes `observer`'s target with one activation, fire-and-forget for
    /// the observed side: the result is discarded. Bounded by
    /// `observers.activation_timeout_ms`, so a target that never answers does
    /// not hold the observer's in-flight slot forever; a target that
    /// disconnects mid-activation ends it at once (its invocations are
    /// halted). Either way the failure is counted.
    async fn invoke_activation(&self, observer: &Arc<Observer>, pending: PendingActivation) {
        let Some(deps) = self.deps.get() else {
            return;
        };
        let Some(invocations) = deps.invocations.upgrade() else {
            return;
        };
        let (target_namespace, target_function) = &observer.target;
        let Some(handler) = deps
            .functions
            .get_handler(target_namespace, target_function)
        else {
            count_failure(observer, failure_reasons::TARGET_UNAVAILABLE);
            return;
        };
        if let Some(metrics) = super::metrics() {
            metrics.observer_activations_total.add(
                1,
                &[
                    KeyValue::new("function_id", observer.function_label()),
                    KeyValue::new("delivery", pending.event.delivery),
                ],
            );
        }
        // Built now, from the event the observers share; the event is let go
        // (and its share of the queue budget given back) before the target
        // answers.
        let event = pending.event.clone();
        drop(pending);
        let activation_id = super::new_uuid().to_string();
        let envelope = project(observer, &event, &activation_id);
        let ctx = InvocationCtx::activation(
            Caller {
                worker_name: Some(ENGINE_CALLER_NAME.to_string()),
                function_id: Some(event.function_id.clone()),
                invocation_id: Some(event.invocation_id.clone()),
                ..Caller::default()
            },
            event.depth.saturating_add(1),
            Activation {
                observer_id: observer.id.clone(),
                activation_id: Some(activation_id),
            },
        );
        let (traceparent, baggage) = (event.traceparent.clone(), event.baggage.clone());
        drop(event);
        let invocation_id = super::new_uuid();
        let call = invocations.handle_invocation_with_handler(
            Some(invocation_id),
            None,
            target_function.clone(),
            target_namespace,
            envelope,
            handler,
            traceparent,
            baggage,
            None,
            observer.metadata.clone(),
            ctx,
        );
        let timeout = Duration::from_millis(self.config.load().activation_timeout_ms);
        let outcome = tokio::time::timeout(timeout, call).await;
        if outcome.is_err() {
            self.abandon(&observer.target, invocation_id).await;
        }
        if self.not_sent_too_large(observer, invocation_id, &outcome) {
            return;
        }
        if let Some(reason) = failure_of(&outcome) {
            tracing::debug!(
                observer_id = %observer.id,
                target = %target_function,
                reason,
                "activation failed"
            );
            count_failure(observer, reason);
        }
    }

    /// Notes that the engine did not send invocation `invocation_id` to its
    /// target worker: the serialized envelope (`size` bytes) is larger than
    /// the worker accepts (`limit`). The caller gets `payload_too_large`; when
    /// that caller is an activation, the event never reached the target and
    /// counts as a `TOO_LARGE` drop rather than the target's error (a target
    /// answering `payload_too_large` itself, because its result was too large,
    /// is still `TARGET_ERROR`). Only the last [`REFUSED_SENDS_KEPT`] are
    /// kept: noting one costs nothing on the path of calls that fit.
    pub(crate) fn refused_send(&self, invocation_id: Uuid, size: usize, limit: usize) {
        let mut refused = lock(&self.refused_sends);
        if refused.len() == REFUSED_SENDS_KEPT {
            refused.pop_front();
        }
        refused.push_back(RefusedSend {
            invocation_id,
            size,
            limit,
        });
    }

    /// Counts a `TOO_LARGE` drop when `outcome` is the engine refusing to send
    /// the activation `invocation_id` (see [`Self::refused_send`]).
    fn not_sent_too_large(
        &self,
        observer: &Observer,
        invocation_id: Uuid,
        outcome: &ActivationOutcome,
    ) -> bool {
        let Ok(Ok(Err(error))) = outcome else {
            return false;
        };
        if error.code != PAYLOAD_TOO_LARGE {
            return false;
        }
        let refused = {
            let mut refused = lock(&self.refused_sends);
            refused
                .iter()
                .rposition(|r| r.invocation_id == invocation_id)
                .and_then(|at| refused.remove(at))
        };
        let Some(refused) = refused else {
            return false;
        };
        drop_too_large(observer, refused.size, refused.limit, TooLarge::Sent);
        true
    }

    /// After a timeout: the target's connection stops tracking the
    /// invocation. A late answer finds nobody waiting and is discarded, as
    /// for any caller that gave up.
    async fn abandon(&self, target: &FnKey, invocation_id: Uuid) {
        let Some(deps) = self.deps.get() else {
            return;
        };
        let owner = deps.function_owners.get(target).map(|owner| owner.0);
        if let Some(worker) = owner.and_then(|id| deps.workers.get_worker(&id)) {
            worker.remove_invocation(&invocation_id).await;
        }
    }

    /// The next queued activation, keeping the slot; or `None`, freeing it.
    fn next_queued(&self, observer: &Observer) -> Option<PendingActivation> {
        let mut flow = lock(&observer.flow);
        match flow.queue.pop_front() {
            Some(pending) => {
                if flow.queue.len() == observer.policy.max_queue / 2 {
                    self.queue_budget.room.notify_waiters();
                }
                Some(pending)
            }
            None => {
                flow.running = flow.running.saturating_sub(1);
                if flow.queue.capacity() > QUEUE_KEPT {
                    flow.queue.shrink_to(QUEUE_KEPT);
                }
                None
            }
        }
    }

    /// Frees one of `observer`'s in-flight slots, or hands it to the next
    /// queued activation.
    fn release_slot(self: &Arc<Self>, observer: &Arc<Observer>) {
        if let Some(pending) = self.next_queued(observer) {
            match self.runtime() {
                Some(runtime) => {
                    runtime.spawn(run_activations(self.clone(), observer.clone(), pending));
                }
                None => {
                    let mut flow = lock(&observer.flow);
                    flow.running = flow.running.saturating_sub(1);
                }
            }
        }
    }

    /// `invocationsobserved` from `connection` (protocol.md §3.4): local
    /// calls of the functions it hosts, with the observers its delivery
    /// matched. The reporter must host each item's function; each `matched`
    /// id must be an active observer of it whose `on` takes the call's
    /// status. The engine does not re-evaluate filters: the host is
    /// authoritative for its own function's calls. An item with
    /// `output_is_input` carries its value once, as `input`, which is also
    /// its `output`. Observers receive `input` and `output` without the
    /// engine's `_caller_worker_id` stamp.
    ///
    /// A routed call's host reports its matches on the call's answer
    /// (`invocationresult.observed`) only: an item for a call the engine
    /// routed to the reporting connection is dropped as `INVALID_REPORT`
    /// (decision 38), while the engine waits on it and, once it is
    /// answered, while `answered` (the connection's) keeps it.
    ///
    /// Returns the observers whose queue was full for some item, for the
    /// connection's reader to wait on ([`Self::wait_for_room`]); not those
    /// whose target the reporting connection hosts, since its answers come
    /// in on the connection that would wait.
    #[must_use = "the reader waits for room in the queues a report overflowed"]
    pub fn report(
        self: &Arc<Self>,
        connection: Uuid,
        connection_namespace: &str,
        answered: &RoutedAnswers,
        items: ObservedItems,
    ) -> Vec<Arc<Observer>> {
        let mut full: Vec<Arc<Observer>> = Vec::new();
        if !items.rejected.is_empty() {
            tracing::warn!(
                connection = %connection,
                rejected = items.rejected.len(),
                first_reason = %items.rejected[0].reason,
                "invocationsobserved: items that do not decode were dropped"
            );
            for rejected in &items.rejected {
                count_function_drop(
                    rejected.function_id.as_deref().unwrap_or("<undecodable>"),
                    INVALID_REPORT,
                    1,
                );
            }
        }
        let worker_name = self
            .deps
            .get()
            .and_then(|deps| deps.workers.get_worker_name(&connection));
        let invocations = self.deps.get().and_then(|deps| deps.invocations.upgrade());

        for item in items.items {
            let ObservedItem {
                function_id,
                namespace,
                invocation_id,
                matched,
                status,
                input,
                output,
                error,
                caller,
                metadata,
                started_at_ms,
                duration_ms,
                observe_depth,
                traceparent,
                baggage,
                output_is_input,
            } = item;
            let key: FnKey = (
                namespace.unwrap_or_else(|| connection_namespace.to_string()),
                function_id,
            );
            let mut seen = HashSet::new();
            let matched: Vec<String> = matched
                .into_iter()
                .filter(|id| seen.insert(id.clone()))
                .collect();
            if self.owner_of(&key).map(|owner| owner.connection) != Some(connection) {
                self.drop_matched(&key, &matched, observe_codes::REPORTER_NOT_OWNER);
                continue;
            }
            let routed = Uuid::parse_str(&invocation_id).is_ok_and(|id| {
                answered.contains(&id)
                    || invocations
                        .as_ref()
                        .is_some_and(|invocations| invocations.is_pending(&id, &key.1))
            });
            if routed {
                self.drop_matched(&key, &matched, INVALID_REPORT);
                continue;
            }
            let success = match status {
                InvocationStatus::Success => true,
                InvocationStatus::Error => false,
                InvocationStatus::Unknown(_) => {
                    self.drop_matched(&key, &matched, INVALID_REPORT);
                    continue;
                }
            };
            if let Some(metrics) = super::metrics() {
                metrics
                    .observer_reports_total
                    .add(1, &[KeyValue::new("function_id", key.1.clone())]);
            }
            // The reporting connection is where the invocation ran: its
            // identity is stamped, whatever the item says.
            let mut caller = caller.unwrap_or_default();
            caller.worker_id = Some(connection.to_string());
            caller.worker_name = worker_name.clone();
            let input = input.map(|input| Arc::new(unstamped(input)));
            let output = if output_is_input {
                input.clone()
            } else {
                output.map(|output| Arc::new(unstamped(output)))
            };
            let completed = Arc::new(Completed {
                delivery: HOST_DELIVERY,
                namespace: key.0.clone(),
                function_id: key.1.clone(),
                invocation_id,
                success,
                input,
                output,
                error: error.map(Arc::new),
                caller: serde_json::to_value(&caller).ok().map(Arc::new),
                metadata: metadata.map(Arc::new),
                started_at_ms,
                duration_ms,
                depth: observe_depth,
                traceparent,
                baggage,
                queues: AtomicUsize::new(0),
                charged: AtomicUsize::new(0),
            });
            for id in &matched {
                match self.get(id).filter(|o| o.function == key) {
                    Some(observer) if !accepts(&observer.on, success) => {}
                    Some(observer) => {
                        if self.activate(&observer, &completed)
                            && !full.iter().any(|o| Arc::ptr_eq(o, &observer))
                            && !self.hosts_target(connection, &observer)
                        {
                            full.push(observer);
                        }
                    }
                    None => count_function_drop(&key.1, observe_codes::INACTIVE, 1),
                }
            }
        }
        full
    }

    /// Whether `connection` hosts `observer`'s target.
    fn hosts_target(&self, connection: Uuid, observer: &Observer) -> bool {
        self.deps
            .get()
            .and_then(|deps| deps.function_owners.get(&observer.target).map(|o| o.0))
            == Some(connection)
    }

    /// Backpressure on a reporting connection (protocol.md §3.4). After a
    /// report overflowed `full`, the connection's reader waits here until
    /// each of those queues, and the queues together ([`QUEUED_BYTES`]), are
    /// down to half their limit (or the observer is no longer active), for
    /// [`REPORT_BACKPRESSURE_WAIT`] at most. Meanwhile
    /// the engine reads nothing more from that connection: a worker that
    /// reports faster than the targets take is slowed by its own socket, and
    /// its SDK drops the excess from its bounded report buffer
    /// (`BUFFER_FULL`), where it costs the engine nothing, instead of the
    /// engine decoding every report to drop it as `OVERFLOW`.
    ///
    /// Everything else the worker sends waits behind its reports, so `hold`
    /// bounds how much of the time it is held: not again until the reader
    /// has read for a quarter as long as it waited (see [`ReportHold`]).
    pub async fn wait_for_room(&self, hold: &mut ReportHold, full: &[Arc<Observer>]) {
        let started = tokio::time::Instant::now();
        if hold.not_before.is_some_and(|at| started < at) {
            return;
        }
        let deadline = started + REPORT_BACKPRESSURE_WAIT;
        let mut waited = false;
        'full: for observer in full {
            loop {
                let room = self.queue_budget.room.notified();
                tokio::pin!(room);
                room.as_mut().enable();
                let queued = lock(&observer.flow).queue.len();
                if !observer.is_active()
                    || (queued <= observer.policy.max_queue / 2 && self.queue_budget.has_room())
                {
                    break;
                }
                if !waited {
                    waited = true;
                    self.report_waits.fetch_add(1, Ordering::Relaxed);
                }
                if tokio::time::timeout_at(deadline, room).await.is_err() {
                    break 'full;
                }
            }
        }
        if waited {
            let ended = tokio::time::Instant::now();
            hold.not_before = Some(ended + (ended - started) / REPORT_HOLD_PER_READ);
        }
    }

    /// Charges a dropped report item to the observers it named (or to the
    /// function, for ids that are not its observers).
    fn drop_matched(&self, key: &FnKey, matched: &[String], reason: &str) {
        if matched.is_empty() {
            count_function_drop(&key.1, reason, 1);
        }
        for id in matched {
            match self.get(id).filter(|o| o.function == *key) {
                Some(observer) => drop_event(&observer, reason),
                None => count_function_drop(&key.1, reason, 1),
            }
        }
    }

    /// A call through an observer (v2 §4.6): an `InvokeFunction` from
    /// `worker` carrying `activation`. The observer must exist, `worker` must
    /// host its function and it must be active; then its policy applies and
    /// the call is routed to the observer's stored target (the request's
    /// `function_id` and `namespace` are not read) with the observer's
    /// metadata, a fresh `activation_id` and depth + 1. The payload goes as
    /// given. The result, or the refusal code, goes back to the host.
    ///
    /// A refusal by policy of a call the host waits on counts a drop as it
    /// does for an activation (`LOOP_GUARD`, `MAX_FIRES`, `OVERFLOW`,
    /// `INACTIVE` once expired). A fire-and-forget call nobody waits on
    /// counts every refusal under the refusal's code, and each is logged at
    /// `debug` (decision 13).
    pub async fn observer_call(
        self: &Arc<Self>,
        worker: &WorkerConnection,
        call: ObserverCall,
    ) -> Result<Option<Value>, ErrorBody> {
        let admitted = self
            .check_observer_call(worker.id, &call)
            .and_then(|observer| {
                let depth = call.observe_depth.unwrap_or(0);
                self.admit_observer_call(&observer, depth, call.fire_and_forget)
                    .map(|slot| (observer, slot, depth))
            });
        let (observer, _slot, depth) = match admitted {
            Ok(admitted) => admitted,
            Err(refusal) => {
                if call.fire_and_forget {
                    tracing::debug!(
                        worker_id = %worker.id,
                        observer_id = %call.activation.observer_id,
                        code = %refusal.code,
                        reason = %refusal.message,
                        "fire-and-forget call through an observer refused; dropped"
                    );
                }
                return Err(refusal);
            }
        };
        self.route_observer_call(worker, &observer, call, depth)
            .await
    }

    /// The checks before policy: the observer exists, `worker` hosts its
    /// function and it is active. A refused fire-and-forget call counts a
    /// drop under the refusal's code: in the observer's counters, or, for
    /// an observer the engine does not know, in the metric alone.
    fn check_observer_call(
        &self,
        worker: Uuid,
        call: &ObserverCall,
    ) -> Result<Arc<Observer>, ErrorBody> {
        let observer_id = &call.activation.observer_id;
        let refuse = |observer: Option<&Observer>, code: &str, message: String| {
            if call.fire_and_forget {
                match observer {
                    Some(observer) => drop_event(observer, code),
                    None => count_function_drop(UNKNOWN_FUNCTION, code, 1),
                }
            }
            Err(ErrorBody::new(code, message))
        };
        let Some(observer) = self.get(observer_id) else {
            return refuse(
                None,
                observe_codes::OBSERVER_NOT_FOUND,
                format!("no observer `{observer_id}`"),
            );
        };
        if self
            .owner_of(&observer.function)
            .map(|owner| owner.connection)
            != Some(worker)
        {
            return refuse(
                Some(&observer),
                observe_codes::NOT_FUNCTION_OWNER,
                format!(
                    "only the worker hosting `{}` calls through its observers",
                    observer.function.1
                ),
            );
        }
        if !observer.is_active() {
            return refuse(
                Some(&observer),
                observe_codes::OBSERVER_NOT_ACTIVE,
                format!("observer `{}` is not active", observer.id),
            );
        }
        Ok(observer)
    }

    /// Policy for a call through `observer`, in order: depth, expiry,
    /// `max_fires`, `max_in_flight` (no queue: the host is waiting for the
    /// result). Returns the in-flight slot the call holds. A refusal counts
    /// a drop under the policy's reason, or under the refusal's code for a
    /// `fire_and_forget` call.
    fn admit_observer_call(
        self: &Arc<Self>,
        observer: &Arc<Observer>,
        depth: u32,
        fire_and_forget: bool,
    ) -> Result<Slot, ErrorBody> {
        let refuse = |policy: &str, code: &str, message: String| {
            drop_event(observer, if fire_and_forget { code } else { policy });
            Err(ErrorBody::new(code, message))
        };
        let max_depth = self.config().max_observe_depth;
        if depth.saturating_add(1) > max_depth {
            warn_loop_guard(observer, depth, max_depth);
            return refuse(
                observe_codes::LOOP_GUARD,
                observe_codes::LOOP_GUARD,
                format!("the chain of observers is deeper than {max_depth}"),
            );
        }
        if observer.is_expired() {
            self.dispose(
                observer,
                observe_codes::EXPIRED,
                "the observer's expires_in_ms elapsed",
            );
            return refuse(
                observe_codes::INACTIVE,
                observe_codes::OBSERVER_NOT_ACTIVE,
                format!("observer `{}` expired", observer.id),
            );
        }
        let Some(fired) = try_fire(observer) else {
            return refuse(
                observe_codes::MAX_FIRES,
                observe_codes::OBSERVER_NOT_ACTIVE,
                format!("observer `{}` reached its max_fires", observer.id),
            );
        };
        {
            let mut flow = lock(&observer.flow);
            if flow.running >= observer.policy.max_in_flight {
                drop(flow);
                observer.fires.fetch_sub(1, Ordering::AcqRel);
                return refuse(
                    observe_codes::OVERFLOW,
                    observe_codes::OVERFLOW,
                    format!(
                        "observer `{}` already has {} calls in flight",
                        observer.id, observer.policy.max_in_flight
                    ),
                );
            }
            flow.running += 1;
        }
        let slot = Slot {
            registry: self.clone(),
            observer: observer.clone(),
            held: true,
        };
        if observer.policy.max_fires == Some(fired) {
            self.dispose(
                observer,
                observe_codes::MAX_FIRES,
                format!("the observer fired {fired} times, its max_fires"),
            );
        }
        Ok(slot)
    }

    async fn route_observer_call(
        &self,
        worker: &WorkerConnection,
        observer: &Arc<Observer>,
        call: ObserverCall,
        depth: u32,
    ) -> Result<Option<Value>, ErrorBody> {
        let Some(deps) = self.deps.get() else {
            return Err(ErrorBody::new(
                "invocation_error",
                "the engine is shutting down",
            ));
        };
        let Some(invocations) = deps.invocations.upgrade() else {
            return Err(ErrorBody::new(
                "invocation_error",
                "the engine is shutting down",
            ));
        };
        let (target_namespace, target_function) = &observer.target;
        let Some(handler) = deps
            .functions
            .get_handler(target_namespace, target_function)
        else {
            count_failure(observer, failure_reasons::TARGET_UNAVAILABLE);
            return Err(ErrorBody::new(
                "function_not_found",
                format!("Function {target_function} not found in namespace {target_namespace}."),
            ));
        };
        if let Some(metrics) = super::metrics() {
            metrics.observer_activations_total.add(
                1,
                &[
                    KeyValue::new("function_id", observer.function.1.clone()),
                    KeyValue::new("delivery", OBSERVER_CALL_DELIVERY),
                ],
            );
        }
        let ctx = InvocationCtx::observer_call(
            worker.id,
            deps.workers.clone(),
            call.caller,
            depth.saturating_add(1),
            Activation {
                observer_id: observer.id.clone(),
                activation_id: Some(Uuid::new_v4().to_string()),
            },
        );
        let invocation_id = call.invocation_id.unwrap_or_else(Uuid::new_v4);
        let invocation = invocations.handle_invocation_with_handler(
            Some(invocation_id),
            Some(worker),
            target_function.clone(),
            target_namespace,
            call.data,
            handler,
            call.traceparent,
            call.baggage,
            None,
            observer.metadata.clone(),
            ctx,
        );
        let timeout = Duration::from_millis(self.config().activation_timeout_ms);
        let outcome = tokio::time::timeout(timeout, invocation).await;
        // The host going away halts its calls, this one included: that is
        // not the target's failure.
        let host_gone = worker
            .cleanup_claimed
            .load(std::sync::atomic::Ordering::SeqCst);
        if outcome.is_err() || host_gone {
            self.abandon(&observer.target, invocation_id).await;
        }
        // Tracked on the host's connection whether or not it gets an answer
        // (an `action: void` call does not).
        worker.remove_invocation(&invocation_id).await;
        match failure_of(&outcome) {
            Some(failure_reasons::TARGET_UNAVAILABLE) if host_gone => {}
            // Not sent: the observer counts a drop, and the host still gets
            // the `payload_too_large` error below.
            Some(_) if self.not_sent_too_large(observer, invocation_id, &outcome) => {}
            Some(reason) => count_failure(observer, reason),
            None => {}
        }
        match outcome {
            Ok(Ok(result)) => result,
            Ok(Err(err)) => Err(ErrorBody::new("invocation_error", err.to_string())),
            Err(_) => Err(ErrorBody::new(
                "TIMEOUT",
                format!(
                    "the target `{target_function}` did not answer within \
                     observers.activation_timeout_ms"
                ),
            )),
        }
    }

    /// `localinvocationstats` from `connection`: local invocation counts
    /// (otherwise invisible to the engine's invocation metrics) and the SDK's
    /// own report drops. Only for functions the connection owns.
    pub fn local_stats(
        &self,
        connection: Uuid,
        connection_namespace: &str,
        functions: &[LocalFunctionStats],
        dropped_reports: &[DroppedReportStats],
    ) {
        let owns = |function_id: &str| {
            let key = (connection_namespace.to_string(), function_id.to_string());
            self.owner_of(&key).map(|owner| owner.connection) == Some(connection)
        };
        let acc = crate::workers::observability::metrics::get_metrics_accumulator();
        for stats in functions {
            if !owns(&stats.function_id) {
                tracing::debug!(
                    connection = %connection,
                    function_id = %stats.function_id,
                    "localinvocationstats for a function the connection does not own; ignoring"
                );
                continue;
            }
            let errors = stats.errors.min(stats.count);
            let ok = stats.count - errors;
            if let Some(metrics) = super::metrics() {
                for (status, count) in [("ok", ok), ("error", errors)] {
                    if count > 0 {
                        metrics.invocations_total.add(
                            count,
                            &[
                                KeyValue::new("function_id", stats.function_id.clone()),
                                KeyValue::new("status", status),
                                KeyValue::new("dispatch", "local"),
                            ],
                        );
                    }
                }
            }
            for (counter, count) in [
                (&acc.invocations_total, stats.count),
                (&acc.invocations_success, ok),
                (&acc.invocations_error, errors),
            ] {
                saturating_add(counter, count);
            }
            acc.add_function_invocations(&stats.function_id, stats.count);
        }
        for dropped in dropped_reports {
            if owns(&dropped.function_id) {
                count_function_drop(
                    &dropped.function_id,
                    sdk_drop_reason(&dropped.reason),
                    dropped.count,
                );
            }
        }
    }
}

/// A host's call through an observer, as it arrived.
pub struct ObserverCall {
    pub invocation_id: Option<Uuid>,
    pub data: Value,
    pub caller: Option<Box<Caller>>,
    pub observe_depth: Option<u32>,
    pub activation: Activation,
    pub traceparent: Option<String>,
    pub baggage: Option<String>,
    /// Nobody waits for the answer (`action: void`, or no `invocation_id`).
    pub fire_and_forget: bool,
}

/// Whether `on` takes an invocation that ended in success (or not).
fn accepts(on: &On, success: bool) -> bool {
    match on {
        On::Success => success,
        On::Error => !success,
        On::Complete => true,
        On::Unknown(_) => false,
    }
}

/// An SDK-side drop reason as a metric label: the reasons SDKs report
/// (protocol.md §4), anything else as `OTHER`, so a peer cannot mint labels.
fn sdk_drop_reason(reason: &str) -> &'static str {
    [
        observe_codes::BUFFER_FULL,
        observe_codes::TOO_LARGE,
        observe_codes::DISCONNECTED,
    ]
    .into_iter()
    .find(|known| reason.eq_ignore_ascii_case(known))
    .unwrap_or("OTHER")
}

fn saturating_add(counter: &std::sync::atomic::AtomicU64, count: u64) {
    let _ = counter.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |total| {
        Some(total.saturating_add(count))
    });
}

/// Counts one fire against `max_fires`; `None` (and a `MAX_FIRES` drop) when
/// the observer has none left. Returns the fire's number.
fn take_fire(observer: &Observer) -> Option<u64> {
    let fired = try_fire(observer);
    if fired.is_none() {
        drop_event(observer, observe_codes::MAX_FIRES);
    }
    fired
}

/// Counts one fire against `max_fires`; `None` when the observer has none
/// left. Returns the fire's number.
fn try_fire(observer: &Observer) -> Option<u64> {
    match observer.policy.max_fires {
        Some(max) => observer
            .fires
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |fires| {
                (fires < max).then_some(fires + 1)
            })
            .ok()
            .map(|previous| previous + 1),
        None => Some(observer.fires.fetch_add(1, Ordering::AcqRel) + 1),
    }
}

/// An activation's result, as its invoker sees it.
type ActivationOutcome =
    Result<Result<Result<Option<Value>, ErrorBody>, RecvError>, tokio::time::error::Elapsed>;

/// The code the engine answers a call with when it does not send the call
/// because the target worker cannot receive it (`Engine::guard_outbound`).
const PAYLOAD_TOO_LARGE: &str = "payload_too_large";

/// An invocation the engine did not send (see
/// [`ObserverRegistry::refused_send`]).
pub(crate) struct RefusedSend {
    invocation_id: Uuid,
    size: usize,
    limit: usize,
}

/// Why an activation failed, if it did.
fn failure_of(outcome: &ActivationOutcome) -> Option<&'static str> {
    match outcome {
        Err(_) => Some(failure_reasons::TIMEOUT),
        Ok(Ok(Ok(_))) => None,
        Ok(Err(_)) => Some(failure_reasons::TARGET_UNAVAILABLE),
        Ok(Ok(Err(error)))
            if error.code == "invocation_stopped" || error.code == "function_not_found" =>
        {
            Some(failure_reasons::TARGET_UNAVAILABLE)
        }
        Ok(Ok(Err(_))) => Some(failure_reasons::TARGET_ERROR),
    }
}

fn count_failure(observer: &Observer, reason: &str) {
    observer.failures.count(reason);
    if let Some(metrics) = super::metrics() {
        metrics.observer_activation_failures_total.add(
            1,
            &[
                KeyValue::new("function_id", observer.function_label()),
                KeyValue::new("reason", reason.to_string()),
            ],
        );
    }
}

/// A dropped event that no observer can be charged with (the matched id is
/// not one of the function's observers, the item did not decode).
fn count_function_drop(function_id: &str, reason: &str, count: u64) {
    if let Some(metrics) = super::metrics() {
        metrics.observer_dropped_total.add(
            count,
            &[
                KeyValue::new("function_id", function_id.to_string()),
                KeyValue::new("reason", reason.to_string()),
            ],
        );
    }
}

/// Runs `first`, then whatever queued behind it while the slot was held, in
/// arrival order; releases the slot when the queue is empty.
async fn run_activations(
    registry: Arc<ObserverRegistry>,
    observer: Arc<Observer>,
    first: PendingActivation,
) {
    let mut slot = Slot {
        registry: registry.clone(),
        observer: observer.clone(),
        held: true,
    };
    let mut next = first;
    loop {
        registry.invoke_activation(&observer, next).await;
        match registry.next_queued(&observer) {
            Some(pending) => next = pending,
            None => {
                slot.held = false;
                return;
            }
        }
    }
}

/// The target's first argument (README §4.3): the `Observed` envelope, or the
/// observed input / output alone with `deliver`.
fn project(observer: &Observer, completed: &Completed, activation_id: &str) -> Value {
    let shipped = |value: &Option<Arc<Value>>| value.as_deref().cloned().unwrap_or(Value::Null);
    match &observer.deliver {
        Deliver::Input => return shipped(&completed.input),
        Deliver::Output => return shipped(&completed.output),
        Deliver::Envelope | Deliver::Unknown(_) => {}
    }
    let capture = observer.capture;
    let mut envelope = Map::new();
    envelope.insert("observer_id".into(), observer.id.clone().into());
    envelope.insert("activation_id".into(), activation_id.into());
    envelope.insert("function_id".into(), completed.function_id.clone().into());
    envelope.insert("namespace".into(), completed.namespace.clone().into());
    envelope.insert(
        "invocation_id".into(),
        completed.invocation_id.clone().into(),
    );
    envelope.insert(
        "status".into(),
        if completed.success {
            "success"
        } else {
            "error"
        }
        .into(),
    );
    for (wanted, key, value) in [
        (capture.input, "input", &completed.input),
        (capture.error, "error", &completed.error),
        (capture.caller, "caller", &completed.caller),
        (capture.metadata, "metadata", &completed.metadata),
    ] {
        if wanted && let Some(value) = value {
            envelope.insert(key.into(), Value::clone(value));
        }
    }
    if capture.output && completed.success {
        envelope.insert("output".into(), shipped(&completed.output));
    }
    envelope.insert("started_at_ms".into(), completed.started_at_ms.into());
    envelope.insert(
        "duration_ms".into(),
        serde_json::Number::from_f64(completed.duration_ms)
            .map(Value::Number)
            .unwrap_or(Value::Null),
    );
    Value::Object(envelope)
}

/// Counts one dropped event for `observer` under `reason`: one atomic add.
/// `iii.observer.dropped.total` follows from the counters (see
/// [`ObserverRegistry::export_drops`]).
fn drop_event(observer: &Observer, reason: &str) {
    let counter = match reason {
        observe_codes::LOOP_GUARD => &observer.drops.loop_guard,
        observe_codes::MAX_FIRES => &observer.drops.max_fires,
        observe_codes::OVERFLOW => &observer.drops.overflow,
        observe_codes::REPORTER_NOT_OWNER => &observer.drops.reporter_not_owner,
        CANCELLED => &observer.drops.cancelled,
        INVALID_REPORT => &observer.drops.invalid_report,
        observe_codes::TOO_LARGE => &observer.drops.too_large,
        observe_codes::NOT_FUNCTION_OWNER => &observer.drops.not_function_owner,
        observe_codes::OBSERVER_NOT_ACTIVE => &observer.drops.observer_not_active,
        _ => &observer.drops.inactive,
    };
    counter.fetch_add(1, Ordering::Relaxed);
}

/// The limit a `TOO_LARGE` event did not fit in.
#[derive(Debug, Clone, Copy)]
enum TooLarge {
    /// The bytes the engine holds for observers' evaluation
    /// ([`EVALUATOR_BYTES`]).
    Held,
    /// The observers' queues together ([`QUEUED_BYTES`]).
    Queued,
    /// What the target worker receives in one message.
    Sent,
}

impl TooLarge {
    /// What did not fit, and what to change.
    fn explain(self) -> &'static str {
        match self {
            Self::Held => {
                "what the function's observers capture or filter on is larger than the engine \
                 keeps for observers; capture fewer fields (input and output both count)"
            }
            Self::Queued => {
                "the event is larger than the observers' queues hold together; capture fewer \
                 fields (input and output both count)"
            }
            Self::Sent => {
                "the activation is larger than the target worker accepts; send the target less \
                 (capture fewer fields, or deliver the input or output alone) or pass large \
                 payloads through a channel"
            }
        }
    }
}

/// Counts a `TOO_LARGE` drop for `observer`: one event of about `size`
/// bytes that cannot fit `limit` even with nothing else waiting, so unlike
/// `OVERFLOW` it says nothing about load. Warns at most once a minute per
/// observer.
fn drop_too_large(observer: &Observer, size: usize, limit: usize, too_large: TooLarge) {
    drop_event(observer, observe_codes::TOO_LARGE);
    if !once_a_minute(&observer.last_too_large_warn_ms) {
        return;
    }
    tracing::warn!(
        observer_id = %observer.id,
        function_id = %observer.function.1,
        target = %observer.target.1,
        size_bytes = size,
        limit_bytes = limit,
        "observer event not delivered (TOO_LARGE): {}",
        too_large.explain()
    );
}

/// Whether a warning last logged at `last` (ms) may be logged again: at
/// most once a minute.
fn once_a_minute(last: &AtomicU64) -> bool {
    let now = now_ms();
    let before = last.load(Ordering::Relaxed);
    now.saturating_sub(before) >= 60_000
        && last
            .compare_exchange(before, now, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
}

/// Adds what `observer` dropped since its last export to
/// `iii.observer.dropped.total`.
pub(crate) fn export_drops(observer: &Observer) {
    let Some(metrics) = super::metrics() else {
        return;
    };
    for (reason, count) in observer.drops.unexported() {
        metrics.observer_dropped_total.add(
            count,
            &[
                KeyValue::new("function_id", observer.function_label()),
                KeyValue::new("reason", reason),
            ],
        );
    }
}

/// The evaluator that takes `set`'s completions.
fn evaluator_of<'a>(
    evaluators: &'a [mpsc::Sender<Evaluation>],
    set: &ActiveSet,
) -> &'a mpsc::Sender<Evaluation> {
    &evaluators[(set.shard % evaluators.len() as u64) as usize]
}

/// At most one warning per observer per minute: a loop fires as fast as it
/// can, and the counter already has the numbers.
fn warn_loop_guard(observer: &Observer, depth: u32, max_depth: u32) {
    if !once_a_minute(&observer.last_loop_warn_ms) {
        return;
    }
    tracing::warn!(
        observer_id = %observer.id,
        function_id = %observer.function.1,
        target = %observer.target.1,
        depth,
        max_depth,
        "observer activation dropped by the loop guard: the chain of observers is deeper than \
         observers.max_observe_depth (a target that calls back into what it observes?)"
    );
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use serde_json::{Value, json};
    use uuid::Uuid;

    use super::super::{InvocationCtx, events::test_support};
    use crate::{
        engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
        function::FunctionResult,
        protocol::DEFAULT_NAMESPACE,
    };

    fn source(engine: &Engine) {
        engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: "test::src".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(|input: Value| async move { FunctionResult::Success(Some(input)) }),
        );
    }

    /// The routed hook hands a completion over and returns: evaluation runs
    /// later, on the evaluator task, so the caller's reply never waits for it.
    #[tokio::test]
    async fn completing_an_observed_call_does_not_evaluate_it_inline() {
        let engine = Arc::new(Engine::new());
        source(&engine);
        let _sink = test_support::capture(&engine, "test::dst");
        let _socket = test_support::observe(&engine, "obs", "test::src", "test::dst", None).await;
        let registry = engine.observers.clone();
        let set = registry
            .watch(DEFAULT_NAMESPACE, "test::src")
            .expect("observed");
        let ctx = Arc::new(InvocationCtx::engine_internal_at(0));
        let call = registry.begin(set, &json!({ "n": 1 }), &None, &None, &None);
        let before = registry.evaluated();
        registry.complete(
            call,
            DEFAULT_NAMESPACE,
            "test::src",
            Uuid::new_v4(),
            &Ok(Ok(Some(json!(true)))),
            None,
            &ctx,
        );
        assert_eq!(registry.evaluated(), before, "not evaluated inline");
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while registry.evaluated() == before {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("evaluated on the evaluator task");
    }

    /// An observed invocation cancelled before its result (its caller gave
    /// up) is counted, not silently lost.
    #[tokio::test]
    async fn a_cancelled_observed_call_counts_a_drop() {
        let engine = Arc::new(Engine::new());
        source(&engine);
        let _sink = test_support::capture(&engine, "test::dst");
        let _socket = test_support::observe(&engine, "obs", "test::src", "test::dst", None).await;
        let set = engine
            .observers
            .watch(DEFAULT_NAMESPACE, "test::src")
            .expect("observed");
        drop(engine.observers.begin(set, &json!({}), &None, &None, &None));
        let info = engine.observers.get_info(&engine, None, "obs").unwrap();
        assert_eq!(info.dropped.get("CANCELLED"), Some(&1));
    }

    /// A target that panics while handling an activation does not keep the
    /// observer's in-flight slot: the next activation runs.
    #[tokio::test]
    async fn a_target_that_panics_frees_its_slot() {
        let engine = Arc::new(Engine::new());
        source(&engine);
        let calls = Arc::new(AtomicUsize::new(0));
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        {
            let calls = calls.clone();
            engine.register_function_handler(
                RegisterFunctionRequest {
                    function_id: "test::panicky".to_string(),
                    description: None,
                    request_format: None,
                    response_format: None,
                    metadata: None,
                },
                Handler::new(move |input: Value| {
                    let calls = calls.clone();
                    let tx = tx.clone();
                    async move {
                        if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                            panic!("the first activation's handler panics");
                        }
                        let _ = tx.send(input);
                        FunctionResult::Success(None)
                    }
                }),
            );
        }
        let (worker, _socket) = test_support::worker(&engine);
        let mut request = test_support::request("obs", "test::src", "test::panicky", None);
        request.max_in_flight = Some(1);
        test_support::observe_request(&engine, &worker, request).await;

        for n in 1..=2 {
            engine.call("test::src", json!({ "n": n })).await.unwrap();
        }
        let delivered = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .expect("the second activation ran")
            .unwrap();
        assert_eq!(delivered, json!({ "n": 2 }));
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                while engine.observers.in_flight("obs") != Some((0, 0)) {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .is_ok()
        );
    }

    /// A call through an observer that policy refuses (expired before its
    /// timer disposed it, or its last fire taken meanwhile) is refused with
    /// `OBSERVER_NOT_ACTIVE`. Fire-and-forget, it counts that code as its
    /// drop reason (decision 13); awaited, the policy's reason, as an
    /// activation does.
    #[tokio::test]
    async fn a_refused_fire_and_forget_call_counts_the_refusal_code() {
        let engine = Arc::new(Engine::new());
        source(&engine);
        let _sink = test_support::capture(&engine, "test::dst");
        let (host, _socket) = test_support::worker(&engine);
        engine.function_owners.insert(
            (DEFAULT_NAMESPACE.to_string(), "test::src".to_string()),
            (host.id, crate::engine::InvocationKind::Regular),
        );
        let call = |observer_id: &str, fire_and_forget: bool| super::ObserverCall {
            invocation_id: None,
            data: json!({}),
            caller: None,
            observe_depth: None,
            activation: crate::protocol::Activation {
                observer_id: observer_id.to_string(),
                activation_id: None,
            },
            traceparent: None,
            baggage: None,
            fire_and_forget,
        };
        let dropped = |id: &str, reason: &str| {
            engine
                .observers
                .get_info(&engine, None, id)
                .unwrap()
                .dropped
                .get(reason)
                .copied()
        };

        let mut spent = test_support::request("spent", "test::src", "test::dst", None);
        spent.max_fires = Some(1);
        test_support::observe_request(&engine, &host, spent).await;
        engine
            .observers
            .get("spent")
            .unwrap()
            .fires
            .store(1, Ordering::SeqCst);
        for fire_and_forget in [true, false] {
            let refused = engine
                .observers
                .observer_call(&host, call("spent", fire_and_forget))
                .await
                .unwrap_err();
            assert_eq!(refused.code, "OBSERVER_NOT_ACTIVE");
        }
        assert_eq!(dropped("spent", "OBSERVER_NOT_ACTIVE"), Some(1));
        assert_eq!(dropped("spent", "MAX_FIRES"), Some(1));

        let mut expiring = test_support::request("expiring", "test::src", "test::dst", None);
        expiring.expires_in_ms = Some(20);
        test_support::observe_request(&engine, &host, expiring).await;
        // Disposed by the refusal, and forgotten: its counters are read
        // from here.
        let observer = engine.observers.get("expiring").unwrap();
        // Blocks the runtime's only thread: its expiry timer cannot run.
        std::thread::sleep(std::time::Duration::from_millis(30));
        let refused = engine
            .observers
            .observer_call(&host, call("expiring", true))
            .await
            .unwrap_err();
        assert_eq!(refused.code, "OBSERVER_NOT_ACTIVE");
        assert_eq!(observer.state(), crate::protocol::ObserverState::Disposed);
        assert_eq!(observer.drops.observer_not_active.load(Ordering::SeqCst), 1);
        assert_eq!(observer.drops.inactive.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn sdk_drop_reasons_are_bucketed_and_counts_saturate() {
        assert_eq!(super::sdk_drop_reason("buffer_full"), "BUFFER_FULL");
        assert_eq!(super::sdk_drop_reason("TOO_LARGE"), "TOO_LARGE");
        assert_eq!(super::sdk_drop_reason("disconnected"), "DISCONNECTED");
        assert_eq!(super::sdk_drop_reason("a-label-of-my-own"), "OTHER");
        let counter = std::sync::atomic::AtomicU64::new(u64::MAX - 1);
        super::saturating_add(&counter, 5);
        assert_eq!(counter.load(Ordering::SeqCst), u64::MAX);
    }

    /// Past the evaluator queue's capacity a completion is not evaluated and
    /// its payload is not copied: an `OVERFLOW` drop.
    #[tokio::test]
    async fn a_full_evaluator_queue_drops_with_overflow() {
        let engine = Arc::new(Engine::new());
        engine
            .observers
            .evaluator_capacity
            .store(1, Ordering::SeqCst);
        source(&engine);
        let _sink = test_support::capture(&engine, "test::dst");
        let _socket = test_support::observe(&engine, "obs", "test::src", "test::dst", None).await;
        let registry = engine.observers.clone();
        let ctx = Arc::new(InvocationCtx::engine_internal_at(0));
        let copies = registry.payload_copies();
        // No yield in between: the evaluator task cannot drain the queue.
        for n in 0..2 {
            let set = registry.watch(DEFAULT_NAMESPACE, "test::src").unwrap();
            let call = registry.begin(set, &json!({ "n": n }), &None, &None, &None);
            registry.complete(
                call,
                DEFAULT_NAMESPACE,
                "test::src",
                Uuid::new_v4(),
                &Ok(Ok(None)),
                None,
                &ctx,
            );
        }
        assert_eq!(
            registry.payload_copies(),
            copies + 1,
            "the second was not copied"
        );
        let info = registry.get_info(&engine, None, "obs").unwrap();
        assert_eq!(info.dropped.get("OVERFLOW"), Some(&1));
    }

    /// `test::src` hosted by a connection of its own, observed by `a`
    /// (whose filter no call here matches) and `b`, each with its own
    /// target. Returns the host and the targets' inputs.
    async fn hosted_source(
        engine: &Arc<Engine>,
    ) -> (
        Uuid,
        tokio::sync::mpsc::UnboundedReceiver<Value>,
        tokio::sync::mpsc::UnboundedReceiver<Value>,
        tokio::sync::mpsc::Receiver<crate::engine::Outbound>,
    ) {
        source(engine);
        let a = test_support::capture(engine, "test::a");
        let b = test_support::capture(engine, "test::b");
        let (worker, socket) = test_support::worker(engine);
        let filtered = test_support::request(
            "a",
            "test::src",
            "test::a",
            Some(json!({ "input.n": { "$eq": 99 } })),
        );
        test_support::observe_request(engine, &worker, filtered).await;
        let unfiltered = test_support::request("b", "test::src", "test::b", None);
        test_support::observe_request(engine, &worker, unfiltered).await;
        let host = Uuid::new_v4();
        engine.function_owners.insert(
            (DEFAULT_NAMESPACE.to_string(), "test::src".to_string()),
            (host, crate::engine::InvocationKind::Regular),
        );
        (host, a, b, socket)
    }

    /// Completes one observed call of `test::src` with `{ n }` as its input,
    /// answered by `host` listing `observed`.
    fn complete_delivered(engine: &Engine, n: u64, host: Uuid, observed: &[&str]) {
        let registry = &engine.observers;
        let ctx = Arc::new(InvocationCtx::engine_internal_at(0));
        let set = registry.watch(DEFAULT_NAMESPACE, "test::src").unwrap();
        let call = registry.begin(set, &json!({ "n": n }), &None, &None, &None);
        registry.complete(
            call,
            DEFAULT_NAMESPACE,
            "test::src",
            Uuid::new_v4(),
            &Ok(Ok(Some(json!({ "n": n })))),
            Some(super::HostDelivery {
                host,
                observed: observed.iter().map(|id| id.to_string()).collect(),
            }),
            &ctx,
        );
    }

    /// A completion its host delivered activates each active observer it
    /// lists once, whatever the observer's filters, and nothing else.
    #[tokio::test]
    async fn a_delivered_completion_activates_what_its_host_lists() {
        let engine = Arc::new(Engine::new());
        let (host, mut a, mut b, _socket) = hosted_source(&engine).await;
        let before = engine.observers.evaluated();
        complete_delivered(&engine, 1, host, &["a", "a", "not-an-observer"]);
        assert_eq!(test_support::next(&mut a).await, json!({ "n": 1 }));
        until(|| engine.observers.evaluated() == before + 1).await;
        assert_eq!(engine.observers.fires("a"), Some(1), "listed twice");
        assert_eq!(engine.observers.fires("b"), Some(0), "not listed");
        assert!(b.try_recv().is_err());

        // From anyone but the host, the list activates nothing.
        complete_delivered(&engine, 2, Uuid::new_v4(), &["a"]);
        let info = engine.observers.get_info(&engine, None, "a").unwrap();
        assert_eq!(info.dropped.get("REPORTER_NOT_OWNER"), Some(&1));
        assert_eq!(engine.observers.fires("a"), Some(1));
    }

    /// When the evaluator has no room for a delivered completion, only the
    /// observers its host listed count the `OVERFLOW`.
    #[tokio::test]
    async fn a_delivered_completion_without_room_charges_what_it_lists() {
        let engine = Arc::new(Engine::new());
        engine
            .observers
            .evaluator_capacity
            .store(1, Ordering::SeqCst);
        let (host, _a, _b, _socket) = hosted_source(&engine).await;
        // No yield in between: the evaluator task cannot drain the queue.
        for n in 0..2 {
            complete_delivered(&engine, n, host, &["a"]);
        }
        let dropped = |id: &str| {
            engine
                .observers
                .get_info(&engine, None, id)
                .unwrap()
                .dropped
                .get("OVERFLOW")
                .copied()
        };
        assert_eq!(dropped("a"), Some(1));
        assert_eq!(dropped("b"), None, "not listed");
    }

    /// Completes one observed call of `test::src` with `body` as its input.
    fn complete_call(engine: &Engine, body: Value) {
        let registry = &engine.observers;
        let ctx = Arc::new(InvocationCtx::engine_internal_at(0));
        let set = registry.watch(DEFAULT_NAMESPACE, "test::src").unwrap();
        let call = registry.begin(set, &body, &None, &None, &None);
        registry.complete(
            call,
            DEFAULT_NAMESPACE,
            "test::src",
            Uuid::new_v4(),
            &Ok(Ok(None)),
            None,
            &ctx,
        );
    }

    /// A completion that does not fit what is left of the evaluator's byte
    /// budget is not queued: an `OVERFLOW` drop.
    #[tokio::test]
    async fn a_completion_past_the_byte_budget_drops_with_overflow() {
        let engine = Arc::new(Engine::new());
        let body = || json!({ "big": "x".repeat(600) });
        let size = super::approx_size(&body());
        engine
            .observers
            .evaluator_bytes
            .store(2 * size + size / 2, Ordering::SeqCst);
        source(&engine);
        let _sink = test_support::capture(&engine, "test::dst");
        let _socket = test_support::observe(&engine, "obs", "test::src", "test::dst", None).await;
        // No yield in between: the evaluator task cannot drain the queue.
        for _ in 0..3 {
            complete_call(&engine, body());
        }
        let info = engine.observers.get_info(&engine, None, "obs").unwrap();
        assert_eq!(info.dropped.get("OVERFLOW"), Some(&1), "only the third");
        assert_eq!(info.dropped.get("TOO_LARGE"), None);
    }

    /// A completion whose payloads alone are larger than the evaluator's
    /// byte budget never fits, however empty the queue: a `TOO_LARGE` drop,
    /// not `OVERFLOW`, logged once a minute with the observer, the function,
    /// the size and the limit.
    #[tokio::test]
    async fn a_completion_larger_than_the_byte_budget_is_too_large() {
        let (_logs_guard, logs) = crate::deprecation::test_support::capture();
        // Another test may have hit the warning's callsite first and cached
        // that nobody listens: recompute it now that this thread does.
        tracing::callsite::rebuild_interest_cache();
        let engine = Arc::new(Engine::new());
        engine
            .observers
            .evaluator_bytes
            .store(512, Ordering::SeqCst);
        source(&engine);
        let _sink = test_support::capture(&engine, "test::dst");
        let _socket = test_support::observe(&engine, "obs", "test::src", "test::dst", None).await;
        let big = json!({ "big": "x".repeat(1024) });
        for body in [json!({ "small": 1 }), big.clone(), big.clone()] {
            complete_call(&engine, body);
        }
        let info = engine.observers.get_info(&engine, None, "obs").unwrap();
        assert_eq!(info.dropped.get("TOO_LARGE"), Some(&2), "both big ones");
        assert_eq!(info.dropped.get("OVERFLOW"), None, "not load");

        let logs = logs.contents();
        let lines: Vec<&str> = logs.lines().filter(|l| l.contains("TOO_LARGE")).collect();
        assert_eq!(lines.len(), 1, "rate limited: {logs}");
        for expected in [
            "observer_id=obs",
            "function_id=test::src",
            "target=test::dst",
            &format!("size_bytes={}", super::approx_size(&big)),
            "limit_bytes=512",
        ] {
            assert!(lines[0].contains(expected), "{expected} in {}", lines[0]);
        }
    }

    /// An in-process target that answers only when `gate` gives it a permit,
    /// and forwards each input it got.
    fn gated(
        engine: &Engine,
        function_id: &str,
    ) -> (
        Arc<tokio::sync::Semaphore>,
        tokio::sync::mpsc::UnboundedReceiver<Value>,
    ) {
        let gate = Arc::new(tokio::sync::Semaphore::new(0));
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let held = gate.clone();
        engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: function_id.to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(move |input: Value| {
                let (gate, tx) = (held.clone(), tx.clone());
                async move {
                    gate.acquire().await.unwrap().forget();
                    let _ = tx.send(input);
                    FunctionResult::Success(None)
                }
            }),
        );
        (gate, rx)
    }

    async fn until(mut done: impl FnMut() -> bool) {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while !done() {
                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            }
        })
        .await
        .expect("in time");
    }

    fn event(n: u64) -> Arc<super::Completed> {
        Arc::new(super::Completed {
            delivery: super::ENGINE_DELIVERY,
            namespace: DEFAULT_NAMESPACE.to_string(),
            function_id: "test::src".to_string(),
            invocation_id: n.to_string(),
            success: true,
            input: Some(Arc::new(json!({ "n": n }))),
            output: None,
            error: None,
            caller: None,
            metadata: None,
            started_at_ms: 0,
            duration_ms: 0.0,
            depth: 0,
            traceparent: None,
            baggage: None,
            queues: AtomicUsize::new(0),
            charged: AtomicUsize::new(0),
        })
    }

    /// `iii.observer.activations.total` labels an activation by who matched
    /// it (decision 12): `engine` when the engine evaluated the call, `host`
    /// when the call's host listed it on its answer or reported it.
    #[tokio::test]
    async fn activations_are_labelled_by_who_matched_them() {
        let engine = Arc::new(Engine::new());
        source(&engine);
        let (gate, mut delivered) = gated(&engine, "test::held");
        let (worker, _socket) = test_support::worker(&engine);
        let mut request = test_support::request("obs", "test::src", "test::held", None);
        request.max_in_flight = Some(1);
        test_support::observe_request(&engine, &worker, request).await;
        let host = Uuid::new_v4();
        engine.function_owners.insert(
            (DEFAULT_NAMESPACE.to_string(), "test::src".to_string()),
            (host, crate::engine::InvocationKind::Regular),
        );

        // The first one holds the observer's only slot: the rest queue.
        complete_call(&engine, json!({ "n": 0 }));
        until(|| engine.observers.in_flight("obs") == Some((1, 0))).await;
        complete_call(&engine, json!({ "n": 1 }));
        complete_delivered(&engine, 2, host, &["obs"]);
        let full = engine.observers.report(
            host,
            DEFAULT_NAMESPACE,
            &super::RoutedAnswers::default(),
            vec![crate::protocol::ObservedItem {
                function_id: "test::src".to_string(),
                namespace: None,
                invocation_id: Uuid::new_v4().to_string(),
                matched: vec!["obs".to_string()],
                status: crate::protocol::InvocationStatus::Success,
                input: Some(json!({ "n": 3 })),
                output: None,
                error: None,
                caller: None,
                metadata: None,
                started_at_ms: 1,
                duration_ms: 0.5,
                observe_depth: 0,
                traceparent: None,
                baggage: None,
                output_is_input: true,
            }]
            .into(),
        );
        assert!(full.is_empty());
        until(|| engine.observers.in_flight("obs") == Some((1, 3))).await;
        // The report activates as it is read, before the evaluator gets to
        // the others: sorted, by the call each came from.
        let mut labels: Vec<(u64, &str)> = {
            let observer = engine.observers.get("obs").unwrap();
            let flow = super::lock(&observer.flow);
            flow.queue
                .iter()
                .map(|pending| {
                    let n = pending.event.input.as_deref().unwrap()["n"]
                        .as_u64()
                        .unwrap();
                    (n, pending.event.delivery)
                })
                .collect()
        };
        labels.sort();
        assert_eq!(
            labels,
            [
                (1, super::ENGINE_DELIVERY),
                (2, super::HOST_DELIVERY),
                (3, super::HOST_DELIVERY)
            ]
        );

        gate.add_permits(4);
        for _ in 0..4 {
            test_support::next(&mut delivered).await;
        }
    }

    /// `iii.observer.activations.total` counts each activation under the
    /// delivery that started it (decision 12): `engine` for a call the engine
    /// evaluated, `host` for one whose host listed the observer, and
    /// `observer_call` for a call through the observer.
    #[tokio::test]
    async fn the_activation_counter_is_labelled_by_delivery() {
        let recorded = super::super::test_metrics::record();
        let engine = Arc::new(Engine::new());
        source(&engine);
        let mut delivered = test_support::capture(&engine, "test::dst");
        let (host, _socket) = test_support::worker(&engine);
        test_support::observe_request(
            &engine,
            &host,
            test_support::request("obs", "test::src", "test::dst", None),
        )
        .await;
        let count = |delivery: &str| {
            recorded.sum(
                "iii.observer.activations.total",
                &[("function_id", "test::src"), ("delivery", delivery)],
            )
        };

        complete_call(&engine, json!({ "n": 1 }));
        test_support::next(&mut delivered).await;
        assert_eq!(count(super::ENGINE_DELIVERY), 1);

        engine.function_owners.insert(
            (DEFAULT_NAMESPACE.to_string(), "test::src".to_string()),
            (host.id, crate::engine::InvocationKind::Regular),
        );
        complete_delivered(&engine, 2, host.id, &["obs"]);
        test_support::next(&mut delivered).await;
        assert_eq!(count(super::HOST_DELIVERY), 1);

        engine
            .observers
            .observer_call(
                &host,
                super::ObserverCall {
                    invocation_id: Some(Uuid::new_v4()),
                    data: json!({ "n": 3 }),
                    caller: None,
                    observe_depth: None,
                    activation: crate::protocol::Activation {
                        observer_id: "obs".to_string(),
                        activation_id: None,
                    },
                    traceparent: None,
                    baggage: None,
                    fire_and_forget: false,
                },
            )
            .await
            .expect("the target answers");
        assert_eq!(test_support::next(&mut delivered).await, json!({ "n": 3 }));
        assert_eq!(
            [
                count(super::ENGINE_DELIVERY),
                count(super::HOST_DELIVERY),
                count(super::OBSERVER_CALL_DELIVERY),
            ],
            [1, 1, 1]
        );
        assert_eq!(
            recorded.sum(
                "iii.observer.activations.total",
                &[("function_id", "test::src")]
            ),
            3,
            "no other label"
        );
    }

    /// Every observer an event matched queues the same event, not a copy of
    /// it; each target still gets its own argument, built when it runs.
    #[tokio::test]
    async fn observers_queue_one_shared_event() {
        let engine = Arc::new(Engine::new());
        source(&engine);
        let (gate, mut delivered) = gated(&engine, "test::held");
        let (worker, _socket) = test_support::worker(&engine);
        for id in ["a", "b"] {
            let mut request = test_support::request(id, "test::src", "test::held", None);
            request.max_in_flight = Some(1);
            test_support::observe_request(&engine, &worker, request).await;
        }
        for n in 1..=2 {
            engine.call("test::src", json!({ "n": n })).await.unwrap();
        }
        // Each observer runs the first event and queues the second.
        until(|| {
            ["a", "b"]
                .iter()
                .all(|id| engine.observers.in_flight(id) == Some((1, 1)))
        })
        .await;
        let queued = |id: &str| {
            let observer = engine.observers.get(id).unwrap();
            let flow = super::lock(&observer.flow);
            flow.queue[0].event.clone()
        };
        assert!(Arc::ptr_eq(&queued("a"), &queued("b")), "one event");

        gate.add_permits(4);
        let mut inputs = Vec::new();
        for _ in 0..4 {
            inputs.push(test_support::next(&mut delivered).await["n"].clone());
        }
        inputs.sort_by_key(|n| n.as_u64());
        assert_eq!(inputs, [json!(1), json!(1), json!(2), json!(2)]);
    }

    /// A queue that a burst grew gives the slots back once it drains.
    #[tokio::test]
    async fn a_drained_queue_gives_its_slots_back() {
        let engine = Arc::new(Engine::new());
        source(&engine);
        let (gate, _delivered) = gated(&engine, "test::held");
        let (worker, _socket) = test_support::worker(&engine);
        let mut request = test_support::request("obs", "test::src", "test::held", None);
        request.max_in_flight = Some(1);
        test_support::observe_request(&engine, &worker, request).await;
        for n in 0..200 {
            engine.call("test::src", json!({ "n": n })).await.unwrap();
        }
        until(|| engine.observers.in_flight("obs") == Some((1, 199))).await;
        let observer = engine.observers.get("obs").unwrap();
        assert!(super::lock(&observer.flow).queue.capacity() >= 199);

        gate.add_permits(200);
        until(|| engine.observers.in_flight("obs") == Some((0, 0))).await;
        assert!(super::lock(&observer.flow).queue.capacity() <= super::QUEUE_KEPT);
    }

    /// There is an evaluator per runtime worker thread, and a function's
    /// activations still arrive in the order its calls completed.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_functions_activations_keep_their_order_across_evaluators() {
        let engine = Arc::new(Engine::new());
        for id in ["test::a", "test::b"] {
            engine.register_function_handler(
                RegisterFunctionRequest {
                    function_id: id.to_string(),
                    description: None,
                    request_format: None,
                    response_format: None,
                    metadata: None,
                },
                Handler::new(|input: Value| async move { FunctionResult::Success(Some(input)) }),
            );
        }
        let (worker, _socket) = test_support::worker(&engine);
        let mut sinks = Vec::new();
        for (id, source, target) in [("oa", "test::a", "test::da"), ("ob", "test::b", "test::db")] {
            sinks.push(test_support::capture(&engine, target));
            let mut request = test_support::request(id, source, target, None);
            request.max_in_flight = Some(1);
            test_support::observe_request(&engine, &worker, request).await;
        }

        let mut callers = tokio::task::JoinSet::new();
        for caller in 0..4u64 {
            let engine = engine.clone();
            callers.spawn(async move {
                let function = if caller % 2 == 0 {
                    "test::a"
                } else {
                    "test::b"
                };
                for n in 0..50u64 {
                    let body = json!({ "caller": caller, "n": n });
                    engine.call(function, body).await.unwrap();
                }
            });
        }
        while let Some(done) = callers.join_next().await {
            done.unwrap();
        }
        assert_eq!(engine.observers.evaluators.get().map(Vec::len), Some(4));

        for mut sink in sinks {
            let mut last = std::collections::HashMap::new();
            for _ in 0..100 {
                let input = test_support::next(&mut sink).await;
                let n = input["n"].as_u64().unwrap();
                if let Some(before) = last.insert(input["caller"].as_u64().unwrap(), n) {
                    assert!(before < n, "{n} arrived after {before}");
                }
            }
        }
    }

    /// The observer `obs` of `test::src` with a queue of 4 that holds
    /// `queued` events, one more running.
    async fn full_queue(engine: &Arc<Engine>, queued: u64) -> Arc<super::Observer> {
        engine.observers.set_config(super::super::ObserversConfig {
            max_queue_per_observer: 4,
            ..Default::default()
        });
        source(engine);
        let _sink = test_support::capture(engine, "test::dst");
        let (worker, _socket) = test_support::worker(engine);
        let request = test_support::request("obs", "test::src", "test::dst", None);
        test_support::observe_request(engine, &worker, request).await;
        let observer = engine.observers.get("obs").unwrap();
        let budget = engine.observers.queue_budget.clone();
        let mut flow = super::lock(&observer.flow);
        flow.running = 1;
        for n in 0..queued {
            let event = event(n);
            assert!(budget.charge(&event));
            flow.queue
                .push_back(super::PendingActivation::queued(event, budget.clone()));
        }
        drop(flow);
        observer
    }

    /// A reader waits until the queue it overflowed is back to half its
    /// limit.
    #[tokio::test(start_paused = true)]
    async fn a_reader_waits_until_the_queue_is_half_empty() {
        let engine = Arc::new(Engine::new());
        let observer = full_queue(&engine, 4).await;
        let registry = engine.observers.clone();
        let started = tokio::time::Instant::now();
        let waiting = tokio::spawn({
            let (registry, observer) = (registry.clone(), observer.clone());
            async move {
                registry
                    .wait_for_room(&mut super::ReportHold::default(), &[observer])
                    .await
            }
        });
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }
        assert!(!waiting.is_finished(), "4 of 4 queued");
        assert!(registry.next_queued(&observer).is_some());
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }
        assert!(!waiting.is_finished(), "3 of 4 queued");
        assert!(registry.next_queued(&observer).is_some());
        waiting.await.unwrap();
        assert!(started.elapsed() < super::REPORT_BACKPRESSURE_WAIT);
        assert_eq!(registry.report_waits(), 1);
    }

    /// With no room coming, the reader reads on after
    /// `REPORT_BACKPRESSURE_WAIT`; at half or below it does not wait at all.
    #[tokio::test(start_paused = true)]
    async fn a_reader_waits_a_bounded_time_and_only_for_a_full_queue() {
        let engine = Arc::new(Engine::new());
        let observer = full_queue(&engine, 4).await;
        let registry = engine.observers.clone();
        let started = tokio::time::Instant::now();
        registry
            .wait_for_room(
                &mut super::ReportHold::default(),
                std::slice::from_ref(&observer),
            )
            .await;
        assert!(started.elapsed() >= super::REPORT_BACKPRESSURE_WAIT);

        registry.next_queued(&observer);
        registry.next_queued(&observer);
        let started = tokio::time::Instant::now();
        registry
            .wait_for_room(&mut super::ReportHold::default(), &[observer])
            .await;
        assert_eq!(started.elapsed(), std::time::Duration::ZERO);
        assert_eq!(registry.report_waits(), 1);
    }

    /// After a hold, the reader is not held again, though the queue is still
    /// full, until it has read for a quarter as long as it waited: a worker
    /// whose reports keep overflowing is read at least a fifth of the time.
    #[tokio::test(start_paused = true)]
    async fn a_reader_is_not_held_again_until_it_read_for_a_quarter_as_long() {
        let engine = Arc::new(Engine::new());
        let observer = full_queue(&engine, 4).await;
        let registry = engine.observers.clone();
        let mut hold = super::ReportHold::default();
        let wait = super::REPORT_BACKPRESSURE_WAIT;
        let read = wait / super::REPORT_HOLD_PER_READ;

        let started = tokio::time::Instant::now();
        registry
            .wait_for_room(&mut hold, std::slice::from_ref(&observer))
            .await;
        assert_eq!(started.elapsed(), wait);
        for _ in 0..3 {
            let started = tokio::time::Instant::now();
            registry
                .wait_for_room(&mut hold, std::slice::from_ref(&observer))
                .await;
            assert_eq!(started.elapsed(), std::time::Duration::ZERO);
        }
        assert_eq!(registry.report_waits(), 1);

        tokio::time::advance(read - std::time::Duration::from_millis(1)).await;
        registry
            .wait_for_room(&mut hold, std::slice::from_ref(&observer))
            .await;
        assert_eq!(registry.report_waits(), 1, "read for less than a quarter");

        tokio::time::advance(std::time::Duration::from_millis(1)).await;
        let started = tokio::time::Instant::now();
        registry.wait_for_room(&mut hold, &[observer]).await;
        assert!(started.elapsed() >= wait);
        assert_eq!(registry.report_waits(), 2);
    }

    /// A hold that ends because the queue got back to half counts too: the
    /// reader then reads for a quarter as long before it can be held again.
    #[tokio::test(start_paused = true)]
    async fn a_hold_that_ends_with_room_also_paces_the_reader() {
        let engine = Arc::new(Engine::new());
        let observer = full_queue(&engine, 4).await;
        let registry = engine.observers.clone();
        let waiting = tokio::spawn({
            let (registry, observer) = (registry.clone(), observer.clone());
            async move {
                let mut hold = super::ReportHold::default();
                registry.wait_for_room(&mut hold, &[observer]).await;
                hold
            }
        });
        let held = std::time::Duration::from_millis(40);
        tokio::time::sleep(held).await;
        registry.next_queued(&observer);
        registry.next_queued(&observer);
        let mut hold = waiting.await.unwrap();
        assert_eq!(registry.report_waits(), 1);

        // Full again: not held before the reader has read for 10 ms.
        let budget = registry.queue_budget.clone();
        {
            let mut flow = super::lock(&observer.flow);
            for n in 4..6 {
                let event = event(n);
                assert!(budget.charge(&event));
                flow.queue
                    .push_back(super::PendingActivation::queued(event, budget.clone()));
            }
        }
        let read = held / super::REPORT_HOLD_PER_READ;
        tokio::time::advance(read - std::time::Duration::from_millis(1)).await;
        registry
            .wait_for_room(&mut hold, std::slice::from_ref(&observer))
            .await;
        assert_eq!(registry.report_waits(), 1);
        tokio::time::advance(std::time::Duration::from_millis(1)).await;
        registry.wait_for_room(&mut hold, &[observer]).await;
        assert_eq!(registry.report_waits(), 2);
    }

    /// A disposed observer's queue is gone: the reader reads on at once.
    #[tokio::test(start_paused = true)]
    async fn a_reader_waiting_on_a_disposed_observer_reads_on() {
        let engine = Arc::new(Engine::new());
        let observer = full_queue(&engine, 4).await;
        let registry = engine.observers.clone();
        let started = tokio::time::Instant::now();
        let waiting = tokio::spawn({
            let (registry, observer) = (registry.clone(), observer.clone());
            async move {
                registry
                    .wait_for_room(&mut super::ReportHold::default(), &[observer])
                    .await
            }
        });
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }
        registry.dispose(
            &observer,
            crate::protocol::observe_codes::UNOBSERVED,
            "test",
        );
        waiting.await.unwrap();
        assert!(started.elapsed() < super::REPORT_BACKPRESSURE_WAIT);
        assert_eq!(
            observer.drops.snapshot().get("INACTIVE"),
            Some(&4),
            "the queue's events"
        );
    }

    /// An event waiting in several queues counts once against the queues'
    /// budget, and gives its share back when the last queue lets it go.
    #[tokio::test]
    async fn a_queued_event_counts_once_against_the_budget() {
        let engine = Arc::new(Engine::new());
        source(&engine);
        let (gate, _delivered) = gated(&engine, "test::held");
        let (worker, _socket) = test_support::worker(&engine);
        for id in ["a", "b"] {
            let mut request = test_support::request(id, "test::src", "test::held", None);
            request.max_in_flight = Some(1);
            test_support::observe_request(&engine, &worker, request).await;
        }
        for n in 1..=2 {
            engine.call("test::src", json!({ "n": n })).await.unwrap();
        }
        until(|| {
            ["a", "b"]
                .iter()
                .all(|id| engine.observers.in_flight(id) == Some((1, 1)))
        })
        .await;
        let budget = &engine.observers.queue_budget;
        let queued = {
            let observer = engine.observers.get("a").unwrap();
            let flow = super::lock(&observer.flow);
            flow.queue[0].event.clone()
        };
        assert_eq!(budget.bytes.load(Ordering::SeqCst), queued.cost());

        gate.add_permits(4);
        until(|| {
            ["a", "b"]
                .iter()
                .all(|id| engine.observers.in_flight(id) == Some((0, 0)))
        })
        .await;
        assert_eq!(budget.bytes.load(Ordering::SeqCst), 0);
    }

    /// Past the queues' budget an activation that would queue is dropped as
    /// `OVERFLOW`, though its own queue has room.
    #[tokio::test]
    async fn past_the_queues_budget_activations_overflow() {
        let engine = Arc::new(Engine::new());
        source(&engine);
        let (gate, _delivered) = gated(&engine, "test::held");
        let (worker, _socket) = test_support::worker(&engine);
        let mut request = test_support::request("obs", "test::src", "test::held", None);
        request.max_in_flight = Some(1);
        test_support::observe_request(&engine, &worker, request).await;
        // The first runs, the second queues: the budget is then made room
        // for two events like it.
        for n in 0..2 {
            engine.call("test::src", json!({ "n": n })).await.unwrap();
        }
        until(|| engine.observers.in_flight("obs") == Some((1, 1))).await;
        let cost = {
            let observer = engine.observers.get("obs").unwrap();
            let flow = super::lock(&observer.flow);
            flow.queue[0].event.cost()
        };
        let limit = 2 * cost + cost / 2;
        engine
            .observers
            .queue_budget
            .limit
            .store(limit, Ordering::SeqCst);
        for n in 2..6 {
            engine.call("test::src", json!({ "n": n })).await.unwrap();
        }
        until(|| engine.observers.evaluated() >= 6).await;
        assert_eq!(engine.observers.in_flight("obs"), Some((1, 2)));
        let info = engine.observers.get_info(&engine, None, "obs").unwrap();
        assert_eq!(info.dropped.get("OVERFLOW"), Some(&3));
        assert!(engine.observers.queue_budget.bytes.load(Ordering::SeqCst) <= limit);
        gate.add_permits(3);
        until(|| engine.observers.in_flight("obs") == Some((0, 0))).await;
        assert_eq!(
            engine.observers.queue_budget.bytes.load(Ordering::SeqCst),
            0
        );
    }

    /// An event larger than the whole of the queues' budget never queues:
    /// while the observer is busy it is a `TOO_LARGE` drop, not `OVERFLOW`,
    /// and a report's reader does not wait for room that cannot come. Events
    /// that fit still queue, then overflow as the budget fills.
    #[tokio::test]
    async fn an_event_larger_than_the_queues_budget_is_too_large() {
        let engine = Arc::new(Engine::new());
        let observer = full_queue(&engine, 1).await;
        let registry = engine.observers.clone();
        let cost = event(0).cost();
        registry
            .queue_budget
            .limit
            .store(3 * cost, Ordering::SeqCst);
        let mut big = Arc::into_inner(event(9)).unwrap();
        big.input = Some(Arc::new(json!({ "big": "x".repeat(4 * cost) })));
        let big = Arc::new(big);
        assert!(!registry.activate(&observer, &big), "no room to wait for");
        for n in 1..=2 {
            assert!(!registry.activate(&observer, &event(n)), "queued");
        }
        assert!(
            registry.activate(&observer, &event(3)),
            "the budget is full"
        );
        let dropped = observer.drops.snapshot();
        assert_eq!(dropped.get("TOO_LARGE"), Some(&1));
        assert_eq!(dropped.get("OVERFLOW"), Some(&1));
        assert_eq!(registry.in_flight("obs"), Some((1, 3)));
    }

    /// A reader also waits while the queues together are over half their
    /// budget, though the queue it overflowed is not.
    #[tokio::test(start_paused = true)]
    async fn a_reader_waits_for_the_queues_budget() {
        let engine = Arc::new(Engine::new());
        let observer = full_queue(&engine, 1).await;
        let registry = engine.observers.clone();
        let budget = registry.queue_budget.clone();
        // The one queued event is over half of this budget.
        budget.limit.store(event(0).cost() + 1, Ordering::SeqCst);
        let waiting = tokio::spawn({
            let (registry, observer) = (registry.clone(), observer.clone());
            async move {
                registry
                    .wait_for_room(&mut super::ReportHold::default(), &[observer])
                    .await
            }
        });
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }
        assert!(!waiting.is_finished(), "the budget is more than half full");
        drop(registry.next_queued(&observer));
        waiting.await.unwrap();
        assert_eq!(budget.bytes.load(Ordering::SeqCst), 0);
    }
}

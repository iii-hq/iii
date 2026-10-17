// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! The routed hook, reports, owner deliveries and activations (protocol.md
//! §3.2, §3.4, §3.5).
//!
//! `InvocationHandler::handle_invocation_inner` asks [`ObserverRegistry::watch`]
//! before running a handler: one atomic load and, only when some function is
//! observed at all, one hash lookup. Unobserved, nothing is copied. Observed,
//! the hook keeps the parts of the invocation the active observers need, and
//! once the result is known [`ObserverRegistry::complete`] hands it to the
//! registry's evaluator task, so the caller's reply never waits for filters or
//! activations. The evaluator handles completions one at a time, in the order
//! they completed, which keeps an observer's activations in engine arrival
//! order.

use std::{
    collections::HashSet,
    sync::{Arc, atomic::Ordering},
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
    owner::ActiveSet,
};
use crate::{
    protocol::{
        Activation, Caller, Deliver, Delivery, DroppedReportStats, ErrorBody, InvocationStatus,
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

/// Completions waiting for the evaluator. Past this, or past
/// [`EVALUATOR_BYTES`] of payloads, a completion is not evaluated (nor its
/// payload copied): each active observer of the function counts an
/// `OVERFLOW` drop.
pub(crate) const EVALUATOR_QUEUE: usize = 65_536;

/// Approximate bytes of payloads waiting for the evaluator.
pub(crate) const EVALUATOR_BYTES: usize = 64 * 1024 * 1024;

/// How an event source's event is evaluated.
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

/// A completed observed invocation (or a reported one), as its activations
/// see it. Payloads are shared: each activation clones what it ships.
pub(crate) struct Completed {
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
}

/// One routed completion waiting for the evaluator.
pub(crate) struct Evaluation {
    /// Approximate size of the payloads it holds, against the evaluator's
    /// byte budget.
    bytes: usize,
    set: Arc<ActiveSet>,
    completed: Completed,
    /// Turned into the document's `error` only when an observer needs it.
    error: Option<ErrorBody>,
    /// Resolved into the document's `caller` only when an observer needs it.
    ctx: Arc<InvocationCtx>,
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
    /// The active observers of `(namespace, function_id)` when it is an auto
    /// function with any. The routed hot path: when nothing is observed this
    /// is one atomic load, and otherwise one more hash lookup.
    #[inline]
    pub fn watch(&self, namespace: &str, function_id: &str) -> Option<Arc<ActiveSet>> {
        let observed = self.observed.load();
        if observed.is_empty() {
            return None;
        }
        observed
            .get(&(namespace, function_id) as &dyn crate::function::FunctionKey)
            .cloned()
    }

    /// [`Self::watch`] for the routed hook: an event source's own handler
    /// only refuses direct calls, which are not its events.
    #[inline]
    pub fn watch_routed(&self, namespace: &str, function_id: &str) -> Option<Arc<ActiveSet>> {
        self.watch(namespace, function_id)
            .filter(|set| !set.event_source)
    }

    /// Whether `(namespace, function_id)` has an active observer: what an
    /// event source checks before building an event.
    #[inline]
    pub fn is_observed(&self, namespace: &str, function_id: &str) -> bool {
        self.watch(namespace, function_id).is_some()
    }

    /// One event of an engine-hosted event source (see
    /// [`super::events`]): evaluated like a successful invocation of
    /// `(namespace, function_id)` whose input is the event, with the engine
    /// as the caller and the depth of the invocation that caused it.
    /// `build` runs only when an active observer needs the event and the
    /// evaluator has room; unobserved, this costs what [`Self::watch`] costs.
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
        let Some(set) = self.watch(namespace, function_id) else {
            return;
        };
        if !self.evaluator_has_room() {
            for observer in &set.observers {
                drop_event(observer, observe_codes::OVERFLOW);
            }
            return;
        }
        let input = if set.needs.input {
            self.events_built.fetch_add(1, Ordering::Relaxed);
            match build() {
                Some(event) => Some(Arc::new(event)),
                None => return,
            }
        } else {
            None
        };
        let depth = emission.depth.unwrap_or_else(InvocationCtx::current_depth);
        let evaluation = Evaluation {
            bytes: input.as_deref().map_or(0, approx_size),
            completed: Completed {
                namespace: namespace.to_string(),
                function_id: function_id.to_string(),
                invocation_id: Uuid::new_v4().to_string(),
                success: true,
                input,
                output: None,
                error: None,
                caller: None,
                metadata: None,
                started_at_ms: now_ms(),
                duration_ms: 0.0,
                depth,
                traceparent: emission.traceparent,
                baggage: None,
            },
            set,
            error: None,
            ctx: Arc::new(InvocationCtx::engine_internal_at(depth)),
        };
        self.enqueue(evaluation);
    }

    /// Starts tracking an observed invocation: copies the input and metadata
    /// only when an active observer needs them, and only when the evaluator
    /// has room (otherwise the call counts an `OVERFLOW` drop when it
    /// completes, without having copied anything).
    ///
    /// Observers see the payload as the caller sent it: a call routed from a
    /// worker connection carries the engine's `_caller_worker_id` stamp, which
    /// the copy leaves out (the caller is `caller.worker_id`).
    pub fn begin(
        &self,
        set: Arc<ActiveSet>,
        body: &Value,
        metadata: &Option<Value>,
        traceparent: &Option<String>,
        baggage: &Option<String>,
        ctx: &InvocationCtx,
    ) -> ObservedCall {
        let admitted = self.evaluator_has_room();
        let input = (admitted && set.needs.input).then(|| {
            self.payload_copies.fetch_add(1, Ordering::Relaxed);
            let mut input = body.clone();
            if ctx.is_from_connection()
                && let Value::Object(map) = &mut input
            {
                map.remove("_caller_worker_id");
            }
            Arc::new(input)
        });
        let metadata = if admitted && set.needs.metadata {
            metadata.clone().map(Arc::new)
        } else {
            None
        };
        ObservedCall {
            inner: Some(Tracked {
                set,
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

    /// Observed routed invocations evaluated so far.
    pub fn evaluated(&self) -> u64 {
        self.evaluated.load(Ordering::Acquire)
    }

    /// Hands the completed invocation to the evaluator: the caller's reply
    /// goes out without waiting for filters or activations. Keeps the output
    /// only when an active observer needs it.
    #[allow(clippy::too_many_arguments)]
    pub fn complete(
        self: &Arc<Self>,
        mut call: ObservedCall,
        namespace: &str,
        function_id: &str,
        invocation_id: Uuid,
        result: &Result<Result<Option<Value>, ErrorBody>, RecvError>,
        ctx: &Arc<InvocationCtx>,
    ) {
        let Some(call) = call.inner.take() else {
            return;
        };
        if !call.admitted || !self.evaluator_has_room() {
            for observer in &call.set.observers {
                drop_event(observer, observe_codes::OVERFLOW);
            }
            return;
        }
        let needs = call.set.needs;
        let (success, output, error) = match result {
            Ok(Ok(output)) => (
                true,
                if needs.output {
                    output.clone().map(Arc::new)
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
            },
            set: call.set,
            error,
            ctx: ctx.clone(),
        };
        self.enqueue(evaluation);
    }

    /// Whether the evaluator can take one more completion: under both its
    /// count ([`EVALUATOR_QUEUE`]) and its byte budget
    /// ([`EVALUATOR_BYTES`], approximate payload sizes).
    fn evaluator_has_room(&self) -> bool {
        self.evaluator
            .get()
            .is_none_or(|evaluator| evaluator.capacity() > 0)
            && self.queued_bytes.load(Ordering::Acquire)
                < self.evaluator_bytes.load(Ordering::Relaxed)
    }

    /// Hands `evaluation` to the evaluator task; inline when there is none.
    /// Past the queue's bounds the completion is dropped as `OVERFLOW` for
    /// each active observer of the function.
    fn enqueue(self: &Arc<Self>, evaluation: Evaluation) {
        let Some(evaluator) = self.evaluator() else {
            self.evaluate(evaluation);
            return;
        };
        let bytes = evaluation.bytes;
        // Reserved, not checked then added: concurrent completions cannot
        // overshoot the budget together.
        let budget = self.evaluator_bytes.load(Ordering::Relaxed);
        let reserved = self
            .queued_bytes
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |queued| {
                queued.checked_add(bytes).filter(|total| *total <= budget)
            })
            .is_ok();
        if !reserved {
            for observer in &evaluation.set.observers {
                drop_event(observer, observe_codes::OVERFLOW);
            }
            return;
        }
        match evaluator.try_send(evaluation) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(evaluation)) => {
                self.queued_bytes.fetch_sub(bytes, Ordering::AcqRel);
                for observer in &evaluation.set.observers {
                    drop_event(observer, observe_codes::OVERFLOW);
                }
            }
            Err(mpsc::error::TrySendError::Closed(evaluation)) => {
                self.queued_bytes.fetch_sub(bytes, Ordering::AcqRel);
                self.evaluate(evaluation);
            }
        }
    }

    /// The evaluator task's queue, started on first use.
    fn evaluator(self: &Arc<Self>) -> Option<mpsc::Sender<Evaluation>> {
        if let Some(evaluator) = self.evaluator.get() {
            return Some(evaluator.clone());
        }
        let runtime = self.runtime()?;
        let capacity = self.evaluator_capacity.load(Ordering::Relaxed).max(1);
        let (tx, mut rx) = mpsc::channel::<Evaluation>(capacity);
        if self.evaluator.set(tx).is_ok() {
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
        self.evaluator.get().cloned()
    }

    /// Evaluates the active observers' `on` and filters against one completed
    /// invocation and activates the matches.
    fn evaluate(self: &Arc<Self>, evaluation: Evaluation) {
        let Evaluation {
            set,
            mut completed,
            error,
            ctx,
            ..
        } = evaluation;
        let needs = set.needs;
        completed.error = error
            .and_then(|e| serde_json::to_value(e).ok())
            .map(Arc::new);
        if needs.caller {
            completed.caller = ctx
                .caller()
                .and_then(|c| serde_json::to_value(c).ok())
                .map(Arc::new);
        }
        let doc = Doc {
            input: completed.input.as_deref(),
            output: completed.output.as_deref(),
            error: completed.error.as_deref(),
            caller: completed.caller.as_deref(),
            metadata: completed.metadata.as_deref(),
        };
        for observer in &set.observers {
            if accepts(&observer.on, completed.success) && observer.filter.matches(&doc) {
                self.activate(observer, &completed);
            }
        }
        self.evaluated.fetch_add(1, Ordering::AcqRel);
    }

    /// Policy for one matched observer (protocol.md §3.4): state, loop guard,
    /// expiry, `max_fires`, then an in-flight slot or the bounded queue.
    fn activate(self: &Arc<Self>, observer: &Arc<Observer>, completed: &Completed) {
        if !observer.is_active() {
            drop_event(observer, observe_codes::INACTIVE);
            return;
        }
        let max_depth = self.config().max_observe_depth;
        if completed.depth.saturating_add(1) > max_depth {
            drop_event(observer, observe_codes::LOOP_GUARD);
            warn_loop_guard(observer, completed.depth, max_depth);
            return;
        }
        if observer.is_expired() {
            self.dispose(
                observer,
                observe_codes::EXPIRED,
                "the observer's expires_in_ms elapsed",
            );
            drop_event(observer, observe_codes::INACTIVE);
            return;
        }
        let Some(fired) = take_fire(observer) else {
            return;
        };
        // Admission is checked again below, but first without projecting: an
        // observer with no room left costs no copy of the payload.
        let no_room = {
            let flow = lock(&observer.flow);
            let can_run = flow.running < observer.policy.max_in_flight && flow.queue.is_empty();
            !can_run && flow.queue.len() >= observer.policy.max_queue
        };
        if no_room {
            observer.fires.fetch_sub(1, Ordering::AcqRel);
            drop_event(observer, observe_codes::OVERFLOW);
            return;
        }

        let activation_id = Uuid::new_v4().to_string();
        let pending = PendingActivation {
            envelope: project(observer, completed, &activation_id),
            activation_id,
            observed_function: completed.function_id.clone(),
            observed_invocation: completed.invocation_id.clone(),
            depth: completed.depth,
            traceparent: completed.traceparent.clone(),
            baggage: completed.baggage.clone(),
        };
        let run_now = {
            let mut flow = lock(&observer.flow);
            if flow.running < observer.policy.max_in_flight && flow.queue.is_empty() {
                flow.running += 1;
                Some(pending)
            } else if flow.queue.len() < observer.policy.max_queue {
                flow.queue.push_back(pending);
                None
            } else {
                drop(flow);
                observer.fires.fetch_sub(1, Ordering::AcqRel);
                drop_event(observer, observe_codes::OVERFLOW);
                return;
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
                    KeyValue::new("function_id", observer.function.1.clone()),
                    KeyValue::new("delivery", "auto"),
                ],
            );
        }
        let ctx = InvocationCtx::activation(
            Caller {
                worker_name: Some(ENGINE_CALLER_NAME.to_string()),
                function_id: Some(pending.observed_function),
                invocation_id: Some(pending.observed_invocation),
                ..Caller::default()
            },
            pending.depth.saturating_add(1),
            Activation {
                observer_id: observer.id.clone(),
                activation_id: Some(pending.activation_id),
            },
        );
        let invocation_id = Uuid::new_v4();
        let call = invocations.handle_invocation_with_handler(
            Some(invocation_id),
            None,
            target_function.clone(),
            target_namespace,
            pending.envelope,
            handler,
            pending.traceparent,
            pending.baggage,
            None,
            observer.metadata.clone(),
            ctx,
        );
        let timeout = Duration::from_millis(self.config().activation_timeout_ms);
        let outcome = tokio::time::timeout(timeout, call).await;
        if outcome.is_err() {
            self.abandon(&observer.target, invocation_id).await;
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
            Some(pending) => Some(pending),
            None => {
                flow.running = flow.running.saturating_sub(1);
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

    /// `invocationsobserved` from `connection` (protocol.md §3.4): matched
    /// local invocations of auto functions and `obs.emit` events of owner
    /// functions. The reporter must own each item's function; each `matched`
    /// id must be an active observer of it. The engine does not re-evaluate
    /// filters: the owner is authoritative for its own function's data.
    pub fn report(
        self: &Arc<Self>,
        connection: Uuid,
        connection_namespace: &str,
        items: ObservedItems,
    ) {
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
            let completed = Completed {
                namespace: key.0.clone(),
                function_id: key.1.clone(),
                invocation_id,
                success,
                input: input.map(Arc::new),
                output: output.map(Arc::new),
                error: error.map(Arc::new),
                caller: serde_json::to_value(&caller).ok().map(Arc::new),
                metadata: metadata.map(Arc::new),
                started_at_ms,
                duration_ms,
                depth: observe_depth,
                traceparent,
                baggage,
            };
            // An auto function's report is an invocation: `on` applies to its
            // status, as on the routed path. An owner function's is an event
            // the owner picked; `on` does not apply (protocol.md §3.4).
            let auto = self
                .function_entry(&key)
                .is_none_or(|entry| entry.is_auto());
            for id in &matched {
                match self.get(id).filter(|o| o.function == key) {
                    Some(observer) if auto && !accepts(&observer.on, success) => {}
                    Some(observer) => self.activate(&observer, &completed),
                    None => count_function_drop(&key.1, observe_codes::INACTIVE, 1),
                }
            }
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

    /// An owner delivery (`obs.call`, protocol.md §3.5): an `InvokeFunction`
    /// from `worker` carrying `activation`. Validated against the observer,
    /// then routed to the observer's stored target (D13: the request's
    /// namespace is ignored) with the observer's metadata, a fresh
    /// `activation_id` and depth + 1. The result, or the rejection code, goes
    /// back to the owner.
    pub async fn owner_call(
        self: &Arc<Self>,
        worker: &WorkerConnection,
        call: OwnerCall,
    ) -> Result<Option<Value>, ErrorBody> {
        let reject = |code: &str, message: String| Err(ErrorBody::new(code, message));
        let Some(observer) = self.get(&call.activation.observer_id) else {
            return reject(
                observe_codes::OBSERVER_NOT_FOUND,
                format!("no observer `{}`", call.activation.observer_id),
            );
        };
        if self
            .owner_of(&observer.function)
            .map(|owner| owner.connection)
            != Some(worker.id)
        {
            return reject(
                observe_codes::NOT_FUNCTION_OWNER,
                format!(
                    "only the worker hosting `{}` delivers for its observers",
                    observer.function.1
                ),
            );
        }
        let owner_delivered = self
            .function_entry(&observer.function)
            .and_then(|entry| entry.descriptor())
            .is_some_and(|descriptor| descriptor.delivery() == Delivery::Owner);
        if !owner_delivered {
            return reject(
                observe_codes::NOT_OWNER_DELIVERED,
                format!("`{}` is not owner-delivered", observer.function.1),
            );
        }
        if !observer.is_active() {
            return reject(
                observe_codes::OBSERVER_NOT_ACTIVE,
                format!("observer `{}` is not active", observer.id),
            );
        }
        if call.function_id != observer.target.1 {
            return reject(
                observe_codes::TARGET_MISMATCH,
                format!(
                    "observer `{}` targets `{}`, not `{}`",
                    observer.id, observer.target.1, call.function_id
                ),
            );
        }

        // Policy, in order: depth, expiry, max_fires, max_in_flight (no
        // queue: the owner is waiting for the result).
        let depth = call.observe_depth.unwrap_or(0);
        let max_depth = self.config().max_observe_depth;
        if depth.saturating_add(1) > max_depth {
            drop_event(&observer, observe_codes::LOOP_GUARD);
            warn_loop_guard(&observer, depth, max_depth);
            return reject(
                observe_codes::LOOP_GUARD,
                format!("the chain of observers is deeper than {max_depth}"),
            );
        }
        if observer.is_expired() {
            self.dispose(
                &observer,
                observe_codes::EXPIRED,
                "the observer's expires_in_ms elapsed",
            );
            drop_event(&observer, observe_codes::INACTIVE);
            return reject(
                observe_codes::OBSERVER_NOT_ACTIVE,
                format!("observer `{}` expired", observer.id),
            );
        }
        let Some(fired) = take_fire(&observer) else {
            return reject(
                observe_codes::OBSERVER_NOT_ACTIVE,
                format!("observer `{}` reached its max_fires", observer.id),
            );
        };
        {
            let mut flow = lock(&observer.flow);
            if flow.running >= observer.policy.max_in_flight {
                drop(flow);
                observer.fires.fetch_sub(1, Ordering::AcqRel);
                drop_event(&observer, observe_codes::OVERFLOW);
                return reject(
                    observe_codes::OVERFLOW,
                    format!(
                        "observer `{}` already has {} deliveries in flight",
                        observer.id, observer.policy.max_in_flight
                    ),
                );
            }
            flow.running += 1;
        }
        let _slot = Slot {
            registry: self.clone(),
            observer: observer.clone(),
            held: true,
        };
        if observer.policy.max_fires == Some(fired) {
            self.dispose(
                &observer,
                observe_codes::MAX_FIRES,
                format!("the observer fired {fired} times, its max_fires"),
            );
        }
        self.deliver_owner_call(worker, &observer, call, depth)
            .await
    }

    async fn deliver_owner_call(
        &self,
        worker: &WorkerConnection,
        observer: &Arc<Observer>,
        call: OwnerCall,
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
                    KeyValue::new("delivery", "owner"),
                ],
            );
        }
        let ctx = InvocationCtx::owner_delivery(
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
        // The owner going away halts its calls, this one included: that is
        // not the target's failure.
        let owner_gone = worker
            .cleanup_claimed
            .load(std::sync::atomic::Ordering::SeqCst);
        if outcome.is_err() || owner_gone {
            self.abandon(&observer.target, invocation_id).await;
        }
        // Tracked on the owner's connection whether or not it gets an answer
        // (an `action: void` call does not).
        worker.remove_invocation(&invocation_id).await;
        match failure_of(&outcome) {
            Some(failure_reasons::TARGET_UNAVAILABLE) if owner_gone => {}
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

/// An owner's `obs.call`, as it arrived.
pub struct OwnerCall {
    pub invocation_id: Option<Uuid>,
    pub function_id: String,
    pub data: Value,
    pub caller: Option<Box<Caller>>,
    pub observe_depth: Option<u32>,
    pub activation: Activation,
    pub traceparent: Option<String>,
    pub baggage: Option<String>,
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
    if reason.eq_ignore_ascii_case(observe_codes::BUFFER_FULL) {
        observe_codes::BUFFER_FULL
    } else if reason.eq_ignore_ascii_case(observe_codes::TOO_LARGE) {
        observe_codes::TOO_LARGE
    } else {
        "OTHER"
    }
}

fn saturating_add(counter: &std::sync::atomic::AtomicU64, count: u64) {
    let _ = counter.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |total| {
        Some(total.saturating_add(count))
    });
}

/// Counts one fire against `max_fires`; `None` (and a `MAX_FIRES` drop) when
/// the observer has none left. Returns the fire's number.
fn take_fire(observer: &Observer) -> Option<u64> {
    match observer.policy.max_fires {
        Some(max) => {
            match observer
                .fires
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |fires| {
                    (fires < max).then_some(fires + 1)
                }) {
                Ok(previous) => Some(previous + 1),
                Err(_) => {
                    drop_event(observer, observe_codes::MAX_FIRES);
                    None
                }
            }
        }
        None => Some(observer.fires.fetch_add(1, Ordering::AcqRel) + 1),
    }
}

/// Why an activation failed, if it did.
fn failure_of(
    outcome: &Result<
        Result<Result<Option<Value>, ErrorBody>, RecvError>,
        tokio::time::error::Elapsed,
    >,
) -> Option<&'static str> {
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
                KeyValue::new("function_id", observer.function.1.clone()),
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

/// Counts one dropped event for `observer` under `reason`.
fn drop_event(observer: &Observer, reason: &str) {
    let counter = match reason {
        observe_codes::LOOP_GUARD => &observer.drops.loop_guard,
        observe_codes::MAX_FIRES => &observer.drops.max_fires,
        observe_codes::OVERFLOW => &observer.drops.overflow,
        observe_codes::REPORTER_NOT_OWNER => &observer.drops.reporter_not_owner,
        CANCELLED => &observer.drops.cancelled,
        INVALID_REPORT => &observer.drops.invalid_report,
        _ => &observer.drops.inactive,
    };
    counter.fetch_add(1, Ordering::Relaxed);
    count_drops(observer, reason, 1);
}

/// Records `count` dropped events in `iii.observer.dropped.total`.
pub(crate) fn count_drops(observer: &Observer, reason: &str, count: u64) {
    if let Some(metrics) = super::metrics() {
        metrics.observer_dropped_total.add(
            count,
            &[
                KeyValue::new("function_id", observer.function.1.clone()),
                KeyValue::new("reason", reason.to_string()),
            ],
        );
    }
}

/// At most one warning per observer per minute: a loop fires as fast as it
/// can, and the counter already has the numbers.
fn warn_loop_guard(observer: &Observer, depth: u32, max_depth: u32) {
    let now = now_ms();
    let last = observer.last_loop_warn_ms.load(Ordering::Relaxed);
    if now.saturating_sub(last) < 60_000
        || observer
            .last_loop_warn_ms
            .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
            .is_err()
    {
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
            .watch_routed(DEFAULT_NAMESPACE, "test::src")
            .expect("observed");
        let ctx = InvocationCtx::engine_internal_at(0);
        let call = registry.begin(set, &json!({ "n": 1 }), &None, &None, &None, &ctx);
        let before = registry.evaluated();
        registry.complete(
            call,
            DEFAULT_NAMESPACE,
            "test::src",
            Uuid::new_v4(),
            &Ok(Ok(Some(json!(true)))),
            &Arc::new(ctx),
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
            .watch_routed(DEFAULT_NAMESPACE, "test::src")
            .expect("observed");
        let ctx = InvocationCtx::engine_internal_at(0);
        drop(
            engine
                .observers
                .begin(set, &json!({}), &None, &None, &None, &ctx),
        );
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

    #[test]
    fn sdk_drop_reasons_are_bucketed_and_counts_saturate() {
        assert_eq!(super::sdk_drop_reason("buffer_full"), "BUFFER_FULL");
        assert_eq!(super::sdk_drop_reason("TOO_LARGE"), "TOO_LARGE");
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
            let set = registry
                .watch_routed(DEFAULT_NAMESPACE, "test::src")
                .unwrap();
            let call = registry.begin(set, &json!({ "n": n }), &None, &None, &None, &ctx);
            registry.complete(
                call,
                DEFAULT_NAMESPACE,
                "test::src",
                Uuid::new_v4(),
                &Ok(Ok(None)),
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

    /// A completion whose payloads do not fit the evaluator's byte budget is
    /// not queued: an `OVERFLOW` drop.
    #[tokio::test]
    async fn a_completion_past_the_byte_budget_drops_with_overflow() {
        let engine = Arc::new(Engine::new());
        engine
            .observers
            .evaluator_bytes
            .store(512, Ordering::SeqCst);
        source(&engine);
        let _sink = test_support::capture(&engine, "test::dst");
        let _socket = test_support::observe(&engine, "obs", "test::src", "test::dst", None).await;
        let registry = engine.observers.clone();
        let ctx = Arc::new(InvocationCtx::engine_internal_at(0));
        for body in [json!({ "small": 1 }), json!({ "big": "x".repeat(1024) })] {
            let set = registry
                .watch_routed(DEFAULT_NAMESPACE, "test::src")
                .unwrap();
            let call = registry.begin(set, &body, &None, &None, &None, &ctx);
            registry.complete(
                call,
                DEFAULT_NAMESPACE,
                "test::src",
                Uuid::new_v4(),
                &Ok(Ok(None)),
                &ctx,
            );
        }
        let info = registry.get_info(&engine, None, "obs").unwrap();
        assert_eq!(info.dropped.get("OVERFLOW"), Some(&1), "only the big one");
    }
}

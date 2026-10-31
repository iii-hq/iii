// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Observer states and transitions (protocol.md §3.3).
//!
//! An observer's state is never set directly. It is derived from a few facts
//! (is its observing worker connected, is its function registered, is its
//! target registered, did it fail, was it disposed) every time one of them
//! changes, so two causes overlapping — the observing worker gone *and* the
//! target unregistered — resolve the same way whatever order they arrive in.

use std::{
    collections::{BTreeMap, VecDeque},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering},
    },
    time::Instant,
};

use iii_filter::Filter;
use serde_json::Value;
use tokio::sync::mpsc;
use uuid::Uuid;

use super::{
    FnKey, Held, ObserverRegistry,
    dispatch::{Completed, QueueBudget},
};
use crate::{
    engine::Outbound,
    protocol::{Capture, Deliver, ErrorBody, Message, ObserverState, On, observe_codes},
};

/// What an activation ships to the target, from the observer's `capture`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct CaptureSet {
    pub input: bool,
    pub output: bool,
    pub error: bool,
    pub caller: bool,
    pub metadata: bool,
}

impl CaptureSet {
    /// What an observer that names no `capture` captures:
    /// [`crate::protocol::default_capture`].
    pub const DEFAULT: Self = Self {
        input: true,
        output: true,
        error: false,
        caller: true,
        metadata: false,
    };

    /// Known values only: `observe` refuses unknown ones before this runs.
    pub fn from_list(list: &[Capture]) -> Self {
        let mut set = Self::default();
        for capture in list {
            match capture {
                Capture::Input => set.input = true,
                Capture::Output => set.output = true,
                Capture::Error => set.error = true,
                Capture::Caller => set.caller = true,
                Capture::Metadata => set.metadata = true,
                Capture::Unknown(_) => {}
            }
        }
        set
    }

    /// The parts the host of the observed function sends the engine for an
    /// observer: what it captures, and the part `deliver` hands the target
    /// (`input` or `output`), which the engine has no other way to get for
    /// a call the host reports.
    pub fn with_delivered(mut self, deliver: &Deliver) -> Self {
        match deliver {
            Deliver::Input => self.input = true,
            Deliver::Output => self.output = true,
            Deliver::Envelope | Deliver::Unknown(_) => {}
        }
        self
    }

    pub fn to_list(self) -> Vec<Capture> {
        [
            (self.input, Capture::Input),
            (self.output, Capture::Output),
            (self.error, Capture::Error),
            (self.caller, Capture::Caller),
            (self.metadata, Capture::Metadata),
        ]
        .into_iter()
        .filter_map(|(on, capture)| on.then_some(capture))
        .collect()
    }
}

/// An observer's spec as its observing worker asked for it, with the defaults
/// applied, the namespaces resolved and the filters normalized: a replay with
/// the same id must carry an equal one (`OBSERVER_ID_CONFLICT` otherwise), and
/// spelling out a default is not a different spec.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ObserveSpec {
    pub function: FnKey,
    pub target: FnKey,
    pub args: Value,
    pub filters: Value,
    pub capture: Vec<Capture>,
    pub on: On,
    pub deliver: Deliver,
    pub metadata: Option<Value>,
    pub max_fires: Option<u64>,
    pub expires_in_ms: Option<u64>,
    pub max_in_flight: u32,
}

#[derive(Debug, Clone)]
pub(crate) struct Policy {
    pub max_fires: Option<u64>,
    pub expires_in_ms: Option<u64>,
    pub expires_at: Option<Instant>,
    pub max_in_flight: u32,
    pub max_queue: usize,
}

/// The facts an observer's state is derived from.
#[derive(Debug, Clone)]
pub(crate) struct Flags {
    pub observing_connected: bool,
    pub function_available: bool,
    pub target_available: bool,
    /// The function's host has the observer: it acked it, or the host does
    /// not speak observe (an older SDK, engine-hosted, HTTP-invoked), so there
    /// is nothing to wait for. `active` needs it (protocol.md §3.3).
    pub owner_acked: bool,
    /// How the current owner was told about the observer
    /// (`functionregistered` or `observeradded`); `None` while it was not.
    /// An ack or an ack timeout counts only for the announcement it answers,
    /// checked under the same lock that a re-announcement takes.
    pub announced: Option<Announcement>,
    /// The owner rejected the observer itself, so it never added it and
    /// needs no `observerremoved`.
    pub rejected_by_owner: bool,
    /// Set the first time the observer is active: a target that goes away
    /// after that suspends it, one that was never there keeps it pending.
    pub activated_once: bool,
    pub failed: Option<Arc<ErrorBody>>,
    pub disposed: Option<Arc<ErrorBody>>,
    /// Its disposal leaves a tombstone (see [`Tombstone`]): not when its
    /// observing worker asked for it, since that worker replays no handle it
    /// disposed.
    pub tombstone: bool,
}

impl Flags {
    /// The state these facts give, and why. Nothing is allocated: the
    /// reason is built from [`Why`] when it is read.
    fn state(&self) -> (ObserverState, Why) {
        if self.disposed.is_some() {
            return (ObserverState::Disposed, Why::Disposed);
        }
        if self.failed.is_some() {
            return (ObserverState::Error, Why::Failed);
        }
        if !self.observing_connected {
            return (ObserverState::Suspended, Why::Disconnected);
        }
        if !self.function_available {
            return (ObserverState::Pending, Why::OwnerUnavailable);
        }
        if !self.target_available {
            let state = if self.activated_once {
                ObserverState::Suspended
            } else {
                ObserverState::Pending
            };
            return (state, Why::TargetUnavailable);
        }
        if !self.owner_acked {
            // Waiting for the owner's `observerack`.
            return (ObserverState::Pending, Why::Nothing);
        }
        (ObserverState::Active, Why::Nothing)
    }
}

/// Why an observer is in its state: what its state reason is built from
/// ([`Observer::reason`]). Each names one reason code, so two observers'
/// reasons have the same code exactly when their `Why` is the same (a
/// disposal or a failure is recorded once and never changes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Why {
    /// No reason: `active`, or `pending` its owner's ack.
    Nothing,
    /// `flags.disposed`.
    Disposed,
    /// `flags.failed`.
    Failed,
    /// `OBSERVER_DISCONNECTED`.
    Disconnected,
    /// `OWNER_UNAVAILABLE`.
    OwnerUnavailable,
    /// `TARGET_UNAVAILABLE`.
    TargetUnavailable,
}

struct Status {
    flags: Flags,
    state: ObserverState,
    why: Why,
    /// Who observes, read with the state (a transition tells this
    /// connection) and changed under the same lock (a `Reattach`).
    observing: Observing,
}

impl Status {
    /// The state reason of `observer`, whose status this is.
    fn reason(&self, observer: &Observer) -> Option<ErrorBody> {
        match self.why {
            Why::Nothing => None,
            Why::Disposed => self.flags.disposed.as_deref().cloned(),
            Why::Failed => self.flags.failed.as_deref().cloned(),
            Why::Disconnected => Some(ErrorBody::new(
                observe_codes::OBSERVER_DISCONNECTED,
                "the observing worker disconnected; the observer is kept until it reattaches or \
                 the grace period ends",
            )),
            Why::OwnerUnavailable => Some(ErrorBody::new(
                observe_codes::OWNER_UNAVAILABLE,
                format!(
                    "no worker hosts `{}` in namespace `{}`",
                    observer.function.1, observer.function.0
                ),
            )),
            Why::TargetUnavailable => Some(ErrorBody::new(
                observe_codes::TARGET_UNAVAILABLE,
                format!(
                    "the target `{}` is not registered in namespace `{}`",
                    observer.target.1, observer.target.0
                ),
            )),
        }
    }

    /// Whether the state reason's code is `code`, read without building it.
    fn reason_is(&self, code: &str) -> bool {
        match self.why {
            Why::Nothing => false,
            Why::Disposed => self.flags.disposed.as_ref().is_some_and(|r| r.code == code),
            Why::Failed => self.flags.failed.as_ref().is_some_and(|r| r.code == code),
            Why::Disconnected => code == observe_codes::OBSERVER_DISCONNECTED,
            Why::OwnerUnavailable => code == observe_codes::OWNER_UNAVAILABLE,
            Why::TargetUnavailable => code == observe_codes::TARGET_UNAVAILABLE,
        }
    }
}

/// Who observes: the connection that created the observer (re-parented on
/// `Reattach`), the worker name and namespace that connection registered
/// with, and the RBAC session it was authorized under, which every later
/// registration of the observed function or the target is checked against
/// again. The name and namespace identify the worker: a new connection
/// under both may replace the observer while its connection is away
/// (decision 51).
#[derive(Clone)]
pub(crate) struct Observing {
    pub connection: Uuid,
    pub worker_name: Option<Arc<str>>,
    pub namespace: Arc<str>,
    pub session: Option<Arc<crate::workers::worker::rbac_session::Session>>,
}

/// Disposed observers remembered at most (see `Tombstone`).
pub(crate) const MAX_TOMBSTONES: usize = 10_000;

/// A disposed observer, remembered for `observers.observer_grace_ms`: its
/// observing worker replaying it learns why it is gone instead of creating it
/// again with fresh counters.
pub(crate) struct Tombstone {
    pub connection: Uuid,
    pub reason: Arc<ErrorBody>,
    pub until: Instant,
}

/// Observers held in `Expired` records at most, across all of them.
pub(crate) const MAX_EXPIRED_OBSERVERS: usize = 10_000;

/// An observing connection whose grace period ended, remembered for
/// `observers.disposed_retention_ms`: a worker that reattaches later,
/// presenting `token`, has its replayed observers answered with their
/// disposal instead of creating them again with fresh counters.
pub(crate) struct Expired {
    pub token: Uuid,
    /// Each observer the connection owned, with why it was disposed.
    pub disposed: Vec<(Arc<str>, Arc<ErrorBody>)>,
    pub until: Instant,
}

/// `Expired` records by expiry, oldest first, and the observers they hold.
#[derive(Default)]
pub(crate) struct ExpiredOrder {
    pub queue: std::collections::VecDeque<(Instant, Uuid, usize)>,
    /// Observers counted in `queue`; a record a reattach consumed still
    /// counts until its entry is dropped.
    pub held: usize,
}

/// Events dropped for one observer, by drop reason (protocol.md §4).
#[derive(Debug, Default)]
pub(crate) struct DropCounters {
    pub inactive: AtomicU64,
    pub loop_guard: AtomicU64,
    pub max_fires: AtomicU64,
    pub overflow: AtomicU64,
    pub reporter_not_owner: AtomicU64,
    /// The invocation was cancelled (its caller gave up, e.g. an activation
    /// timeout) before its result could be evaluated.
    pub cancelled: AtomicU64,
    /// A report item naming the observer was malformed (an unknown
    /// `status`), or reported a routed call.
    pub invalid_report: AtomicU64,
    /// The event alone was larger than a limit it must fit in whole: what
    /// the engine holds for observers, or what the target worker receives.
    /// No amount of waiting makes room for it, unlike `OVERFLOW`.
    pub too_large: AtomicU64,
    /// A fire-and-forget call through the observer from a connection that
    /// does not host its function.
    pub not_function_owner: AtomicU64,
    /// A fire-and-forget call through the observer while it was not active.
    pub observer_not_active: AtomicU64,
    /// Each counter's value when it last went to the metric, in
    /// [`Self::counters`] order (see [`Self::unexported`]).
    exported: [AtomicU64; 10],
}

impl DropCounters {
    fn counters(&self) -> [(&'static str, &AtomicU64); 10] {
        [
            (observe_codes::INACTIVE, &self.inactive),
            (observe_codes::LOOP_GUARD, &self.loop_guard),
            (observe_codes::MAX_FIRES, &self.max_fires),
            (observe_codes::OVERFLOW, &self.overflow),
            (observe_codes::REPORTER_NOT_OWNER, &self.reporter_not_owner),
            (CANCELLED, &self.cancelled),
            (INVALID_REPORT, &self.invalid_report),
            (observe_codes::TOO_LARGE, &self.too_large),
            (observe_codes::NOT_FUNCTION_OWNER, &self.not_function_owner),
            (
                observe_codes::OBSERVER_NOT_ACTIVE,
                &self.observer_not_active,
            ),
        ]
    }

    pub fn snapshot(&self) -> BTreeMap<String, u64> {
        self.counters()
            .into_iter()
            .map(|(reason, count)| (reason.to_string(), count.load(Ordering::Relaxed)))
            .filter(|(_, count)| *count > 0)
            .collect()
    }

    /// The drops counted since the last call, by reason: what
    /// `iii.observer.dropped.total` has not been told yet. One caller at a
    /// time (the registry's export task, or the observer's drop).
    pub(crate) fn unexported(&self) -> impl Iterator<Item = (&'static str, u64)> + '_ {
        self.counters()
            .into_iter()
            .zip(&self.exported)
            .filter_map(|((reason, count), exported)| {
                let total = count.load(Ordering::Relaxed);
                let before = exported.swap(total, Ordering::Relaxed);
                (total > before).then_some((reason, total - before))
            })
    }
}

/// Drop reason for an observed invocation cancelled before its result was
/// known (not in protocol.md §4, which has no such case).
pub(crate) const CANCELLED: &str = "CANCELLED";

/// Drop reason for report items that do not decode, carry a `status` this
/// engine does not know, or report a routed call (decision 38).
pub(crate) const INVALID_REPORT: &str = "INVALID_REPORT";

/// Activations that started and did not complete, by reason. Not drops: the
/// event reached the target, or was on its way.
#[derive(Debug, Default)]
pub(crate) struct FailureCounters {
    /// No answer within `observers.activation_timeout_ms`.
    pub timeout: AtomicU64,
    /// The target was gone when the activation started, or went away before
    /// answering.
    pub target_unavailable: AtomicU64,
    /// The target answered with an error.
    pub target_error: AtomicU64,
}

/// Activation failure reasons (`iii.observer.activation.failures.total`).
pub(crate) mod failure_reasons {
    pub const TIMEOUT: &str = "TIMEOUT";
    pub const TARGET_UNAVAILABLE: &str = "TARGET_UNAVAILABLE";
    pub const TARGET_ERROR: &str = "TARGET_ERROR";
}

impl FailureCounters {
    pub fn count(&self, reason: &str) {
        let counter = match reason {
            failure_reasons::TIMEOUT => &self.timeout,
            failure_reasons::TARGET_UNAVAILABLE => &self.target_unavailable,
            _ => &self.target_error,
        };
        counter.fetch_add(1, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> BTreeMap<String, u64> {
        [
            (failure_reasons::TIMEOUT, &self.timeout),
            (
                failure_reasons::TARGET_UNAVAILABLE,
                &self.target_unavailable,
            ),
            (failure_reasons::TARGET_ERROR, &self.target_error),
        ]
        .into_iter()
        .map(|(reason, count)| (reason.to_string(), count.load(Ordering::Relaxed)))
        .filter(|(_, count)| *count > 0)
        .collect()
    }
}

/// One announcement of an observer to its function's owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Announcement {
    /// The owner connection that was told.
    pub owner: Uuid,
    /// The revision of the message that told it; an ack echoing an older one
    /// is stale.
    pub revision: u64,
    /// Unique per announcement: the timeout of an earlier one, even at the
    /// same revision (an owner re-registering), does not count.
    pub generation: u64,
}

/// An activation accepted by policy, running or waiting for an in-flight
/// slot. The event is shared with every other observer it matched; the
/// target's argument is built from it when the activation starts, so a queued
/// activation costs a pointer, not a copy of the event.
pub(crate) struct PendingActivation {
    pub event: Arc<Completed>,
    /// Set while it waits in a queue: the event is charged to the queues'
    /// budget until then.
    queued: Option<Arc<QueueBudget>>,
}

impl PendingActivation {
    /// One that starts now.
    pub(crate) fn running(event: Arc<Completed>) -> Self {
        Self {
            event,
            queued: None,
        }
    }

    /// One that waits in a queue, its event already charged to `budget`.
    pub(crate) fn queued(event: Arc<Completed>, budget: Arc<QueueBudget>) -> Self {
        Self {
            event,
            queued: Some(budget),
        }
    }
}

impl Drop for PendingActivation {
    /// Out of the queue (to run, or with the queue dropped): the queue's
    /// share of the budget goes back.
    fn drop(&mut self) {
        if let Some(budget) = self.queued.take() {
            budget.release(&self.event);
        }
    }
}

/// In-flight accounting: `running` activations hold a slot each; the rest
/// wait in `queue`, in engine arrival order.
#[derive(Default)]
pub(crate) struct Flow {
    pub running: u32,
    pub queue: VecDeque<PendingActivation>,
}

#[derive(Default)]
struct Timers {
    expiry: Option<tokio::task::AbortHandle>,
    /// `OWNER_TIMEOUT` for the current announcement.
    ack: Option<tokio::task::AbortHandle>,
}

/// One `iii.observe(source, target)`.
pub struct Observer {
    pub id: String,
    /// The observed function, `(namespace, function_id)`.
    pub function: FnKey,
    /// `function.1`, shared: the `function_id` label of its metrics.
    function_label: Arc<str>,
    /// The function each activation invokes.
    pub target: FnKey,
    /// The spec its observing worker asked for, kept only when it differs
    /// from what the observer's own fields give (a registration hook or a
    /// function prefix rewrote it): see [`Observer::spec_matches`].
    spec: Option<Box<ObserveSpec>>,
    /// `{}` when the observer has none.
    pub args: Value,
    pub filter: Filter,
    pub(crate) capture: CaptureSet,
    pub on: On,
    pub deliver: Deliver,
    /// The activation's metadata sidecar.
    pub metadata: Option<Value>,
    pub(crate) policy: Policy,
    pub created_at_ms: u64,
    /// Filter paths that could not be verified at creation.
    pub(crate) warnings: Vec<String>,
    status: Mutex<Status>,
    pub(crate) fires: AtomicU64,
    pub(crate) drops: DropCounters,
    pub(crate) failures: FailureCounters,
    pub(crate) flow: Mutex<Flow>,
    timers: Mutex<Timers>,
    /// A timer was ever set in `timers`: until then, cancelling them takes
    /// no lock.
    timed: AtomicBool,
    pub(crate) last_loop_warn_ms: AtomicU64,
    /// When a `TOO_LARGE` drop was last logged (see
    /// [`super::dispatch::drop_too_large`]).
    pub(crate) last_too_large_warn_ms: AtomicU64,
    /// Its place in its function's creation order, given when it joins the
    /// function's observers ([`ObserverRegistry::attach_to_function`]): `0`
    /// until then.
    seq: AtomicU64,
    /// It is one of its function's live observers: set as it joins them and
    /// cleared as it leaves, under the function's observers lock.
    member: AtomicBool,
    /// The function entry it joined (see [`Observer::joined_entry`]).
    joined: std::sync::OnceLock<std::sync::Weak<super::owner::FunctionEntry>>,
    /// Its function's active set in the routed view holds it: set and
    /// cleared with the set, under the registry's `rebuild` lock.
    in_active_set: AtomicBool,
    /// The engine created it for one of its own modules
    /// ([`super::ENGINE_CONNECTION`] observes it, for its whole life).
    engine_owned: bool,
}

impl Drop for Observer {
    /// The drops the registry's export task has not sent to the metric yet.
    fn drop(&mut self) {
        super::dispatch::export_drops(self);
    }
}

/// Everything `Observer::new` needs; the registry builds it after validation.
pub(crate) struct NewObserver {
    pub id: String,
    pub function: FnKey,
    pub target: FnKey,
    /// The spec its observing worker asked for; `None` when the observer's
    /// own fields are its spec.
    pub spec: Option<ObserveSpec>,
    pub args: Value,
    pub filter: Filter,
    pub capture: CaptureSet,
    pub on: On,
    pub deliver: Deliver,
    pub metadata: Option<Value>,
    pub policy: Policy,
    pub warnings: Vec<String>,
    pub observing: Observing,
    pub flags: Flags,
}

impl Observer {
    pub(crate) fn new(new: NewObserver) -> Self {
        let engine_owned = new.observing.connection == super::ENGINE_CONNECTION;
        let spec = new.spec;
        let mut observer = Self {
            id: new.id,
            function_label: Arc::from(new.function.1.as_str()),
            function: new.function,
            target: new.target,
            spec: None,
            args: new.args,
            filter: new.filter,
            capture: new.capture,
            on: new.on,
            deliver: new.deliver,
            metadata: new.metadata,
            policy: new.policy,
            created_at_ms: now_ms(),
            warnings: new.warnings,
            status: Mutex::new(Status {
                flags: new.flags,
                state: ObserverState::Pending,
                why: Why::Nothing,
                observing: new.observing,
            }),
            fires: AtomicU64::new(0),
            drops: DropCounters::default(),
            failures: FailureCounters::default(),
            flow: Mutex::new(Flow::default()),
            timers: Mutex::new(Timers::default()),
            timed: AtomicBool::new(false),
            last_loop_warn_ms: AtomicU64::new(0),
            last_too_large_warn_ms: AtomicU64::new(0),
            seq: AtomicU64::new(0),
            member: AtomicBool::new(false),
            joined: std::sync::OnceLock::new(),
            in_active_set: AtomicBool::new(false),
            engine_owned,
        };
        if let Some(spec) = spec
            && !observer.has_spec(&spec)
        {
            observer.spec = Some(Box::new(spec));
        }
        {
            let status = observer.status.get_mut().unwrap_or_else(|e| e.into_inner());
            let (state, why) = status.flags.state();
            if state == ObserverState::Active {
                status.flags.activated_once = true;
            }
            status.state = state;
            status.why = why;
        }
        observer
    }

    /// Whether `spec` is the one this observer was created with: a replay
    /// with the same id must carry it (`OBSERVER_ID_CONFLICT` otherwise).
    pub(crate) fn spec_matches(&self, spec: &ObserveSpec) -> bool {
        match &self.spec {
            Some(own) => **own == *spec,
            None => self.has_spec(spec),
        }
    }

    /// Whether the observer's own fields give `spec`.
    fn has_spec(&self, spec: &ObserveSpec) -> bool {
        spec.function == self.function
            && spec.target == self.target
            && spec.args == self.args
            && spec.filters == *self.filter.normalized()
            && spec.capture == self.capture.to_list()
            && spec.on == self.on
            && spec.deliver == self.deliver
            && spec.metadata == self.metadata
            && spec.max_fires == self.policy.max_fires
            && spec.expires_in_ms == self.policy.expires_in_ms
            && spec.max_in_flight == self.policy.max_in_flight
    }

    pub fn state(&self) -> ObserverState {
        lock(&self.status).state.clone()
    }

    pub fn reason(&self) -> Option<ErrorBody> {
        lock(&self.status).reason(self)
    }

    /// Whether its state reason's code is `code`, read without building the
    /// reason.
    pub(crate) fn reason_is(&self, code: &str) -> bool {
        lock(&self.status).reason_is(code)
    }

    pub fn is_active(&self) -> bool {
        lock(&self.status).state == ObserverState::Active
    }

    /// Whether a fire it has already counted runs: it is `active`, or it
    /// was disposed for reaching `max_fires`. That disposal keeps the queue,
    /// so every fire counted within `max_fires` runs, including one whose
    /// activation the last fire overtook. Any other state drops the fire.
    pub(crate) fn runs_counted_fires(&self) -> bool {
        let status = lock(&self.status);
        match status.state {
            ObserverState::Active => true,
            ObserverState::Disposed => status
                .flags
                .disposed
                .as_deref()
                .is_some_and(|reason| reason.code == observe_codes::MAX_FIRES),
            _ => false,
        }
    }

    /// Whether it was disposed: set before its disposal takes it out of any
    /// index.
    pub(crate) fn is_disposed(&self) -> bool {
        lock(&self.status).flags.disposed.is_some()
    }

    /// Runs `f` with the state and what builds its reason, holding the
    /// status lock: no transition happens meanwhile.
    pub(crate) fn with_state<R>(
        &self,
        f: impl FnOnce(&ObserverState, &dyn Fn() -> Option<ErrorBody>) -> R,
    ) -> R {
        let status = lock(&self.status);
        f(&status.state, &|| status.reason(self))
    }

    /// State and reason read together.
    pub fn snapshot_state(&self) -> (ObserverState, Option<ErrorBody>) {
        let status = lock(&self.status);
        (status.state.clone(), status.reason(self))
    }

    pub(crate) fn observing(&self) -> Observing {
        lock(&self.status).observing.clone()
    }

    /// The observing connection, read without copying the rest of
    /// [`Self::observing`].
    pub(crate) fn observing_connection(&self) -> Uuid {
        lock(&self.status).observing.connection
    }

    /// The `function_id` label of its metrics: its function's id, shared.
    pub(crate) fn function_label(&self) -> Arc<str> {
        self.function_label.clone()
    }

    /// Whether the engine created this observer for one of its own modules.
    pub fn is_engine_owned(&self) -> bool {
        self.engine_owned
    }

    /// Its place in its function's creation order; `None` before it joined
    /// its function's observers.
    pub(crate) fn seq(&self) -> Option<u64> {
        match self.seq.load(Ordering::Acquire) {
            0 => None,
            seq => Some(seq),
        }
    }

    pub(crate) fn set_seq(&self, seq: u64) {
        self.seq.store(seq, Ordering::Release);
    }

    /// Whether it is one of its function's live observers.
    pub(crate) fn is_member(&self) -> bool {
        self.member.load(Ordering::Acquire)
    }

    pub(crate) fn set_member(&self, member: bool) {
        self.member.store(member, Ordering::Release);
    }

    /// The function entry it joined, while that entry is kept: the live one
    /// for its function as long as the observer is one of its members.
    pub(crate) fn joined_entry(&self) -> Option<Arc<super::owner::FunctionEntry>> {
        self.joined.get().and_then(std::sync::Weak::upgrade)
    }

    pub(crate) fn set_joined_entry(&self, entry: &Arc<super::owner::FunctionEntry>) {
        let _ = self.joined.set(Arc::downgrade(entry));
    }

    /// Whether its function's active set holds it (read under `rebuild`).
    pub(crate) fn in_active_set(&self) -> bool {
        self.in_active_set.load(Ordering::Acquire)
    }

    pub(crate) fn set_in_active_set(&self, held: bool) {
        self.in_active_set.store(held, Ordering::Release);
    }

    pub fn fires(&self) -> u64 {
        self.fires.load(Ordering::Relaxed)
    }

    pub(crate) fn set_observing(&self, observing: Observing) {
        lock(&self.status).observing = observing;
    }

    pub(crate) fn set_expiry_timer(&self, handle: tokio::task::AbortHandle) {
        let mut timers = lock(&self.timers);
        timers.expiry = Some(handle);
        self.timed.store(true, Ordering::Release);
    }

    pub(crate) fn set_ack_timer(&self, handle: tokio::task::AbortHandle) {
        let mut timers = lock(&self.timers);
        self.timed.store(true, Ordering::Release);
        if let Some(previous) = timers.ack.replace(handle) {
            previous.abort();
        }
    }

    pub(crate) fn cancel_ack_timer(&self) {
        if !self.timed.load(Ordering::Acquire) {
            return;
        }
        if let Some(handle) = lock(&self.timers).ack.take() {
            handle.abort();
        }
    }

    fn cancel_timers(&self) {
        if !self.timed.load(Ordering::Acquire) {
            return;
        }
        let mut timers = lock(&self.timers);
        for handle in [timers.expiry.take(), timers.ack.take()]
            .into_iter()
            .flatten()
        {
            handle.abort();
        }
    }

    pub(crate) fn is_expired(&self) -> bool {
        self.policy
            .expires_at
            .is_some_and(|expires_at| Instant::now() >= expires_at)
    }

    /// The flags, for decisions that depend on more than the state.
    #[cfg(test)]
    pub(crate) fn flags(&self) -> Flags {
        lock(&self.status).flags.clone()
    }

    /// Runs `f` with the flags, holding the status lock: for decisions that
    /// depend on more than the state, without copying them.
    pub(crate) fn with_flags<R>(&self, f: impl FnOnce(&Flags) -> R) -> R {
        f(&lock(&self.status).flags)
    }

    /// The announcement the observer waits for its owner to answer: told,
    /// and neither acked, failed nor disposed since. `None` otherwise.
    pub(crate) fn awaited_announcement(&self) -> Option<Announcement> {
        let status = lock(&self.status);
        let flags = &status.flags;
        flags
            .announced
            .filter(|_| !flags.owner_acked && flags.failed.is_none() && flags.disposed.is_none())
    }

    /// Clears the owner announcement (no state depends on it) and returns
    /// it, with whether the owner had rejected the observer, whether it had
    /// acked it, and whether it failed for not acking it in time.
    pub(crate) fn take_announcement(&self) -> (Option<Announcement>, bool, bool, bool) {
        let mut status = lock(&self.status);
        let flags = &mut status.flags;
        let announced = flags.announced.take();
        let rejected = std::mem::take(&mut flags.rejected_by_owner);
        let timed_out = flags
            .failed
            .as_ref()
            .is_some_and(|error| error.code == observe_codes::OWNER_TIMEOUT);
        (announced, rejected, flags.owner_acked, timed_out)
    }
}

/// Poisoning only means another thread panicked mid-update; the data is
/// still the best answer there is.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

/// Live observers by state, for the `iii.observers` gauge: counted on every
/// transition, and sent to the gauge every
/// [`DROP_EXPORT_INTERVAL`](super::dispatch::DROP_EXPORT_INTERVAL) with the
/// drop counts ([`ObserverRegistry::export_drops`]).
#[derive(Default)]
pub(crate) struct StateCounts {
    pending: AtomicI64,
    active: AtomicI64,
    suspended: AtomicI64,
    error: AtomicI64,
}

impl StateCounts {
    fn slot(&self, state: &ObserverState) -> Option<&AtomicI64> {
        match state {
            ObserverState::Pending => Some(&self.pending),
            ObserverState::Active => Some(&self.active),
            ObserverState::Suspended => Some(&self.suspended),
            ObserverState::Error => Some(&self.error),
            ObserverState::Disposed | ObserverState::Unknown(_) => None,
        }
    }

    pub fn enter(&self, state: &ObserverState) {
        if let Some(slot) = self.slot(state) {
            slot.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn leave(&self, state: &ObserverState) {
        if let Some(slot) = self.slot(state) {
            slot.fetch_sub(1, Ordering::Relaxed);
        }
    }

    pub fn get(&self, state: &ObserverState) -> i64 {
        self.slot(state)
            .map(|slot| slot.load(Ordering::Relaxed))
            .unwrap_or_default()
    }

    /// Sends each count to the `iii.observers` gauge.
    pub fn record(&self) {
        let Some(metrics) = super::metrics() else {
            return;
        };
        for (label, slot) in [
            ("pending", &self.pending),
            ("active", &self.active),
            ("suspended", &self.suspended),
            ("error", &self.error),
        ] {
            metrics.observers.record(
                slot.load(Ordering::Relaxed),
                &[opentelemetry::KeyValue::new("state", label)],
            );
        }
    }
}

/// A state change made under an observer's status lock
/// ([`ObserverRegistry::transition`]), for [`ObserverRegistry::settle`] once
/// the lock is released.
#[must_use = "a transition is settled once the status lock is released"]
pub(super) struct Transition {
    became: ObserverState,
    unqueued: Option<super::events::UnqueuedEvent>,
    activeness_changed: bool,
}

/// What [`ObserverRegistry::settle_deferring_refresh`] leaves its caller.
#[must_use = "the active set follows, then a disposed observer is forgotten"]
pub(super) struct Settled {
    /// Activeness changed: the observer joins its function's active set or
    /// leaves it.
    pub refresh: bool,
    /// It was disposed: [`ObserverRegistry::forget`] drops it.
    pub disposed: bool,
}

impl ObserverRegistry {
    /// Applies `change` to `observer`'s flags and re-derives its state. On a
    /// change, the observing worker hears about it (`observerstatechanged`),
    /// `engine::on-observers-changed` is called, the observer joins or leaves
    /// its function's active set when activeness changed, and a disposed
    /// observer is forgotten.
    pub(crate) fn update(&self, observer: &Arc<Observer>, change: impl FnOnce(&mut Flags)) {
        if let Some(transition) = self.transition(observer, change) {
            self.settle(observer, transition);
        }
    }

    /// The part of [`Self::update`] done under `observer`'s status lock:
    /// applies `change`, re-derives the state and, on a change, tells the
    /// observing worker (`observerstatechanged`) and hands
    /// `engine::on-observers-changed` its call, so one observer's transitions
    /// reach both in the order they happened. `None` when the state and its
    /// reason stayed the same. The caller settles a transition
    /// ([`Self::settle`]).
    pub(super) fn transition(
        &self,
        observer: &Arc<Observer>,
        change: impl FnOnce(&mut Flags),
    ) -> Option<Transition> {
        self.transition_telling(observer, change, true)
    }

    /// [`Self::transition`], telling the observing worker only when `tell`:
    /// not when its connection is gone (it learns its observers' states
    /// again when it reattaches or replays them).
    pub(super) fn transition_telling(
        &self,
        observer: &Arc<Observer>,
        change: impl FnOnce(&mut Flags),
        tell: bool,
    ) -> Option<Transition> {
        let mut status = lock(&observer.status);
        change(&mut status.flags);
        let (state, why) = status.flags.state();
        if state == status.state && why == status.why {
            return None;
        }
        let was_active = status.state == ObserverState::Active;
        self.state_counts.leave(&status.state);
        self.state_counts.enter(&state);
        if state == ObserverState::Active {
            status.flags.activated_once = true;
        }
        status.state = state.clone();
        status.why = why;
        let unqueued = self.observer_changed(observer, &state, &|| status.reason(observer));
        // The engine hears nothing about the observers it owns.
        if tell && !observer.is_engine_owned() {
            self.notify(
                status.observing.connection,
                Message::ObserverStateChanged {
                    id: observer.id.clone(),
                    state: state.clone(),
                    reason: status.reason(observer),
                    fires: observer.fires(),
                },
            );
        }
        Some(Transition {
            activeness_changed: was_active != (state == ObserverState::Active),
            became: state,
            unqueued,
        })
    }

    /// The part of [`Self::update`] done once `observer`'s status lock is
    /// released: the `engine::on-observers-changed` call no evaluator took is
    /// evaluated, an observer in `disposed` or `error` leaves its owner's set
    /// (`observerremoved`), the observer joins or leaves its function's
    /// active set when activeness changed, and a disposed observer is
    /// forgotten.
    pub(super) fn settle(&self, observer: &Arc<Observer>, transition: Transition) {
        let settled = self.settle_deferring_refresh(observer, transition);
        if settled.refresh {
            self.refresh_observer(observer);
        }
        if settled.disposed {
            self.forget(observer);
        }
    }

    /// [`Self::settle`] but for the change of its function's active set and
    /// the disposed observer's forgetting, which the caller does, in that
    /// order: put the observer in its active set or take it out when
    /// activeness changed ([`Self::refresh_observers`] does many at once),
    /// then forget it if it was disposed.
    pub(super) fn settle_deferring_refresh(
        &self,
        observer: &Arc<Observer>,
        transition: Transition,
    ) -> Settled {
        let Transition {
            became,
            unqueued,
            activeness_changed,
        } = transition;
        tracing::debug!(observer_id = %observer.id, state = %became, "observer state changed");
        if let Some(event) = unqueued {
            event.evaluate();
        }

        if became == ObserverState::Disposed || became == ObserverState::Error {
            // Out of the owner's set: it hears `observerremoved`.
            self.withdraw_from_owner(observer);
            if !observer.reason_is(observe_codes::MAX_FIRES) {
                let dropped = std::mem::take(&mut lock(&observer.flow).queue).len() as u64;
                observer
                    .drops
                    .inactive
                    .fetch_add(dropped, Ordering::Relaxed);
            }
        }
        if became != ObserverState::Active {
            // A reporting connection waiting for room in this observer's
            // queue reads on: its events no longer reach the queue.
            self.queue_budget.room.notify_waiters();
        }
        Settled {
            refresh: activeness_changed,
            disposed: became == ObserverState::Disposed,
        }
    }

    /// Disposes `observer` with `code` as the state reason.
    pub(crate) fn dispose(&self, observer: &Arc<Observer>, code: &str, message: impl Into<String>) {
        let reason = Arc::new(ErrorBody::new(code, message));
        self.update(observer, |flags| {
            if flags.disposed.is_none() {
                flags.disposed = Some(reason);
            }
        });
    }

    /// Disposes `observer` at the request of whoever observes with it (its
    /// observing worker, or the engine for its own): as [`Self::dispose`],
    /// with no tombstone, since that observer replays no handle it disposed.
    /// The reason is shared by every such disposal.
    pub(crate) fn dispose_unobserved(&self, observer: &Arc<Observer>) {
        static BY_WORKER: std::sync::OnceLock<Arc<ErrorBody>> = std::sync::OnceLock::new();
        static BY_ENGINE: std::sync::OnceLock<Arc<ErrorBody>> = std::sync::OnceLock::new();
        let (reason, message) = if observer.is_engine_owned() {
            (&BY_ENGINE, "disposed by the engine")
        } else {
            (&BY_WORKER, "disposed by its observing worker")
        };
        let reason = reason
            .get_or_init(|| Arc::new(ErrorBody::new(observe_codes::UNOBSERVED, message)))
            .clone();
        self.update(observer, |flags| {
            if flags.disposed.is_none() {
                flags.disposed = Some(reason);
                flags.tombstone = false;
            }
        });
    }

    /// Moves `observer` to `error`: a rejection that arrived after creation.
    pub(crate) fn fail(&self, observer: &Arc<Observer>, error: ErrorBody) {
        self.update(observer, |flags| {
            if flags.failed.is_none() {
                flags.failed = Some(Arc::new(error));
            }
        });
    }

    /// Files `observer`, just given its id in `by_id`, under `connection` and
    /// under its target. Called under its status lock (see
    /// [`Self::publish`]): its disposal, which takes that lock first, always
    /// finds both entries to take out.
    pub(super) fn index(&self, observer: &Arc<Observer>, connection: Uuid) {
        let held = Held(observer.clone());
        self.by_connection
            .entry(connection)
            .or_default()
            .insert(held.clone());
        match self.by_target.get_mut(&observer.target) {
            Some(mut targeting) => {
                targeting.insert(held);
            }
            None => {
                self.by_target
                    .entry(observer.target.clone())
                    .or_default()
                    .insert(held);
            }
        }
    }

    /// Takes `observer`'s entries out of the connection and target indexes:
    /// its own, not those of another observer under the same id.
    fn unindex(&self, observer: &Arc<Observer>, connection: Uuid) {
        let held = Held(observer.clone());
        self.by_connection
            .remove_if_mut(&connection, |_, observing| {
                observing.remove(&held);
                observing.is_empty()
            });
        self.by_target
            .remove_if_mut(&observer.target, |_, targeting| {
                targeting.remove(&held);
                targeting.is_empty()
            });
    }

    /// Drops every index entry of a disposed observer. Its observing worker
    /// replaying it later learns why it is gone from its tombstone, unless
    /// that worker disposed of it, or the engine owns it (the engine replays
    /// nothing).
    pub(super) fn forget(&self, observer: &Arc<Observer>) {
        observer.cancel_timers();
        self.by_id
            .remove_if(&observer.id, |_, current| Arc::ptr_eq(current, observer));
        let connection = observer.observing_connection();
        let (tombstone, reason) =
            observer.with_flags(|flags| (flags.tombstone, flags.disposed.clone()));
        if tombstone && !observer.is_engine_owned() {
            self.remember_tombstone(
                Arc::from(observer.id.as_str()),
                Tombstone {
                    connection,
                    reason: reason.unwrap_or_else(|| {
                        Arc::new(ErrorBody::new(
                            observe_codes::UNOBSERVED,
                            "the observer was disposed",
                        ))
                    }),
                    until: Instant::now()
                        + std::time::Duration::from_millis(self.config.load().observer_grace_ms),
                },
            );
        }
        self.unindex(observer, connection);
        self.detach_from_function(observer);
    }

    /// Keeps `tombstone` for its grace period. Tombstones expire in the order
    /// they were made, so the oldest are dropped first, as they expire or
    /// when there are more than [`MAX_TOMBSTONES`]: amortized constant work
    /// per disposal, however many observers churn. One that expired and was
    /// not dropped yet is ignored where it is read.
    pub(super) fn remember_tombstone(&self, id: Arc<str>, tombstone: Tombstone) {
        let until = tombstone.until;
        self.tombstones.insert(id.clone(), tombstone);
        let mut order = lock(&self.tombstone_order);
        order.push_back((until, id));
        let now = Instant::now();
        while let Some((oldest, _)) = order.front() {
            if *oldest > now && self.tombstones.len() <= MAX_TOMBSTONES {
                break;
            }
            let Some((oldest, id)) = order.pop_front() else {
                break;
            };
            // A newer tombstone for the same id stays.
            self.tombstones
                .remove_if(&id, |_, current| current.until == oldest);
        }
    }

    /// Keeps what `connection`'s observers became when its grace period
    /// ended (see [`Expired`]), for `observers.disposed_retention_ms`. Like
    /// tombstones, records expire oldest first, and the oldest are dropped
    /// early when they hold more than [`MAX_EXPIRED_OBSERVERS`] observers.
    pub(crate) fn remember_expired(
        &self,
        connection: Uuid,
        token: Uuid,
        disposed: Vec<(Arc<str>, Arc<ErrorBody>)>,
    ) {
        let retention = std::time::Duration::from_millis(self.config().disposed_retention_ms);
        if disposed.is_empty() || retention.is_zero() {
            return;
        }
        let now = Instant::now();
        let until = now + retention;
        let count = disposed.len();
        self.expired.insert(
            connection,
            Expired {
                token,
                disposed,
                until,
            },
        );
        let mut order = lock(&self.expired_order);
        order.queue.push_back((until, connection, count));
        order.held += count;
        while let Some((oldest, _, _)) = order.queue.front() {
            if *oldest > now && order.held <= MAX_EXPIRED_OBSERVERS {
                break;
            }
            let Some((oldest, connection, count)) = order.queue.pop_front() else {
                break;
            };
            order.held -= count;
            self.expired
                .remove_if(&connection, |_, current| current.until == oldest);
        }
    }

    /// Sends `msg` to `connection`, after everything sent to it before. Each
    /// connection gets one forwarding task, so a slow worker never holds up
    /// another's notifications. A connection that is gone drops the message.
    ///
    /// The queue is bounded ([`NOTIFY_QUEUE`]): a worker that stops reading
    /// while its notifications pile up is disconnected, so it reconnects and
    /// resyncs (replayed observers, a fresh `functionregistered`) instead of
    /// the engine holding an ever-growing backlog for it.
    pub(crate) fn notify(&self, connection: Uuid, msg: Message) {
        let existing = self
            .notifiers
            .get(&connection)
            .map(|n| (n.tx.clone(), n.evict.clone()));
        let (tx, evict) = match existing {
            Some(notifier) => notifier,
            None => {
                let Some(deps) = self.deps.get() else {
                    return;
                };
                let Some(worker) = deps.workers.get_worker(&connection) else {
                    return;
                };
                // A connection in teardown gets no new forwarder: it would
                // outlive the connection in `notifiers`.
                if worker
                    .cleanup_claimed
                    .load(std::sync::atomic::Ordering::SeqCst)
                {
                    return;
                }
                let Some(runtime) = self.runtime() else {
                    return;
                };
                let (tx, mut rx) = mpsc::channel::<Message>(NOTIFY_QUEUE);
                let notifier = self.notifiers.entry(connection).or_insert(Notifier {
                    tx,
                    evict: worker.evict.clone(),
                });
                let current = (notifier.tx.clone(), notifier.evict.clone());
                drop(notifier);
                // A losing racer's receiver ends with its sender, so only the
                // winner's task outlives this call.
                runtime.spawn(async move {
                    while let Some(msg) = rx.recv().await {
                        if worker.channel.send(Outbound::Protocol(msg)).await.is_err() {
                            break;
                        }
                    }
                });
                current
            }
        };
        match tx.try_send(msg) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                tracing::warn!(
                    connection = %connection,
                    queued = NOTIFY_QUEUE,
                    "a worker is not reading its observe notifications; closing its connection \
                     so it reconnects and resyncs"
                );
                self.notifiers.remove(&connection);
                evict.notify_one();
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                self.notifiers.remove(&connection);
            }
        }
    }
}

/// Observe notifications queued for one connection before the engine
/// disconnects it.
pub(crate) const NOTIFY_QUEUE: usize = 4096;

/// One connection's notification queue.
pub(crate) struct Notifier {
    tx: mpsc::Sender<Message>,
    evict: Arc<tokio::sync::Notify>,
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use dashmap::DashMap;

    use super::*;
    use crate::{
        function::FunctionsRegistry,
        invocation::InvocationHandler,
        worker_connections::{WorkerConnection, WorkerConnectionRegistry},
    };

    /// A worker that stops reading its notifications is disconnected once its
    /// queue is full, instead of the engine buffering for it without bound.
    #[tokio::test]
    async fn a_worker_that_stops_reading_its_notifications_is_evicted() {
        let registry = Arc::new(ObserverRegistry::new());
        let workers = Arc::new(WorkerConnectionRegistry::new());
        let (tx, _never_drained) = mpsc::channel(1);
        let worker = WorkerConnection::new(tx);
        let (id, evict) = (worker.id, worker.evict.clone());
        workers.register_worker(worker);
        let invocations = Arc::new(InvocationHandler::new());
        registry.attach(
            Arc::new(FunctionsRegistry::new()),
            workers,
            Arc::downgrade(&invocations),
            Arc::new(DashMap::new()),
        );

        for revision in 0..=NOTIFY_QUEUE as u64 {
            registry.notify(
                id,
                Message::ObserverRemoved {
                    function_id: "orders::create".into(),
                    namespace: "default".into(),
                    revision,
                    id: "obs".into(),
                },
            );
        }
        tokio::time::timeout(Duration::from_secs(5), evict.notified())
            .await
            .expect("the connection is evicted");
        assert!(
            registry.notifiers.get(&id).is_none(),
            "its backlog is dropped"
        );
    }

    #[test]
    fn the_default_capture_set_is_the_default_capture() {
        assert_eq!(
            CaptureSet::from_list(&crate::protocol::default_capture()),
            CaptureSet::DEFAULT
        );
    }

    /// The metric is told each drop once: what was counted since the last
    /// export, nothing when there was none.
    #[test]
    fn drops_are_exported_once() {
        let drops = DropCounters::default();
        drops.overflow.fetch_add(3, Ordering::Relaxed);
        drops.inactive.fetch_add(1, Ordering::Relaxed);
        let mut first: Vec<_> = drops.unexported().collect();
        first.sort();
        assert_eq!(
            first,
            [(observe_codes::INACTIVE, 1), (observe_codes::OVERFLOW, 3)]
        );
        assert_eq!(drops.unexported().count(), 0);
        drops.overflow.fetch_add(2, Ordering::Relaxed);
        assert_eq!(
            drops.unexported().collect::<Vec<_>>(),
            [(observe_codes::OVERFLOW, 2)]
        );
        assert_eq!(drops.snapshot().get("OVERFLOW"), Some(&5));
    }
}

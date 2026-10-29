//! The host's side of observe: the observers of the functions this worker
//! hosts, and the task that runs their hooks.
//!
//! Each registered function has a [`FunctionEntry`]: its compiled set of
//! active observers (read with one atomic load on every call), its deliver
//! hook, and a hook task that applies the engine's announcements
//! (`functionregistered`, `observeradded`, `observerremoved`) one at a time,
//! running the [`AddedHook`](crate::AddedHook)s and
//! [`RemovedHook`](crate::RemovedHook)s set in [`hooks`](crate::hooks).

use std::{
    collections::{BTreeMap, HashSet},
    fmt,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    time::Duration,
};

use arc_swap::{ArcSwap, ArcSwapOption};
use futures_util::FutureExt;
use iii_filter::{Doc, Filter, Roots};
use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::sync::{mpsc, watch};

use crate::{
    error::Error,
    hooks::{AddedSlot, Call, DeliverHook, Keyed, Level, RemovedHook},
    iii::{IIIClient, IIIInner},
    protocol::{Capture, ErrorBody, InvocationStatus, Message, ObserverView, On, observe_codes},
    reporter::CaptureMask,
};

/// How long the added hooks of an observer may run before it is rejected
/// with `OWNER_TIMEOUT` (the engine enforces the same timeout).
pub(crate) const HOOK_TIMEOUT: Duration = Duration::from_secs(30);

/// An [`AddedHook`](crate::AddedHook)'s refusal of an observer, with the code
/// the observer's `ready()` fails with. Any other error is `OBSERVE_REJECTED`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObserveRejection {
    pub code: String,
    pub message: String,
}

impl ObserveRejection {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for ObserveRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ObserveRejection {}

/// A remote error keeps its code; anything else is `OBSERVE_REJECTED`.
impl From<Error> for ObserveRejection {
    fn from(error: Error) -> Self {
        match error {
            Error::Remote { code, message, .. } => Self { code, message },
            other => Self::new(observe_codes::OBSERVE_REJECTED, other.to_string()),
        }
    }
}

impl From<String> for ObserveRejection {
    fn from(message: String) -> Self {
        Self::new(observe_codes::OBSERVE_REJECTED, message)
    }
}

impl From<&str> for ObserveRejection {
    fn from(message: &str) -> Self {
        Self::new(observe_codes::OBSERVE_REJECTED, message)
    }
}

/// So `?` works on [`Observer::args`] inside a handler returning [`Error`].
impl From<ObserveRejection> for Error {
    fn from(rejection: ObserveRejection) -> Self {
        Error::Remote {
            code: rejection.code,
            message: rejection.message,
            stacktrace: None,
        }
    }
}

pub(crate) fn coded_error(code: &str, message: impl Into<String>) -> Error {
    Error::Remote {
        code: code.to_string(),
        message: message.into(),
        stacktrace: None,
    }
}

pub(crate) fn capture_mask(capture: &[Capture]) -> CaptureMask {
    let mut mask = CaptureMask::default();
    for capture in capture {
        match capture {
            Capture::Input => mask.input = true,
            Capture::Output => mask.output = true,
            Capture::Error => mask.error = true,
            Capture::Caller => mask.caller = true,
            Capture::Metadata => mask.metadata = true,
            Capture::Unknown(_) => {}
        }
    }
    mask
}

/// One observer of a function this worker hosts, as its host sees it: its
/// id, args, filters, target, capture and `on`. Never its metadata, policy or
/// observing worker.
///
/// Cheap to clone (an `Arc`), so hooks take it by value and a worker keeps it
/// in its own index. Two `Observer`s are equal when their ids are.
#[derive(Clone)]
pub struct Observer {
    inner: Arc<ObserverInner>,
}

/// Where an observer stands with the SDK's ack.
#[derive(Debug, Clone, PartialEq)]
enum Acceptance {
    Pending,
    Accepted,
    Refused(ErrorBody),
}

struct ObserverInner {
    view: ObserverView,
    filter: Filter,
    mask: CaptureMask,
    function_id: String,
    namespace: String,
    /// The revision the engine last announced it in (`observeradded`, or a
    /// `functionregistered` that listed it): what an ack must echo, or the
    /// engine ignores it as stale.
    revision: AtomicU64,
    /// In the function's compiled set: acked and neither removed nor rejected.
    active: AtomicBool,
    /// Set by [`Observer::reject`]: re-sent whenever the engine lists the
    /// observer again, until it is removed.
    rejection: Mutex<Option<ErrorBody>>,
    acceptance: watch::Sender<Acceptance>,
    client: Weak<IIIInner>,
}

impl fmt::Debug for Observer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Observer")
            .field("id", &self.inner.view.id)
            .field("function_id", &self.inner.function_id)
            .field("args", &self.inner.view.args)
            .field("filters", &self.inner.view.filters)
            .field("target", &self.inner.view.target)
            .field("active", &self.is_active())
            .finish()
    }
}

impl PartialEq for Observer {
    fn eq(&self, other: &Self) -> bool {
        self.inner.view.id == other.inner.view.id
    }
}

impl Eq for Observer {}

impl std::hash::Hash for Observer {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.inner.view.id.hash(state)
    }
}

impl Observer {
    fn new(
        view: ObserverView,
        filter: Filter,
        function_id: &str,
        namespace: String,
        revision: u64,
        client: Weak<IIIInner>,
    ) -> Self {
        let mask = capture_mask(&view.capture);
        let (acceptance, _) = watch::channel(Acceptance::Pending);
        Self {
            inner: Arc::new(ObserverInner {
                view,
                filter,
                mask,
                function_id: function_id.to_string(),
                namespace,
                revision: AtomicU64::new(revision),
                active: AtomicBool::new(false),
                rejection: Mutex::new(None),
                acceptance,
                client,
            }),
        }
    }

    /// The observer's id (client-generated, stable across replays).
    pub fn id(&self) -> &str {
        &self.inner.view.id
    }

    /// The observed function: the one this worker registered.
    pub fn function_id(&self) -> &str {
        &self.inner.function_id
    }

    /// The observed function's namespace.
    pub fn namespace(&self) -> &str {
        &self.inner.namespace
    }

    /// The observer's args as `T`. The engine validated them against the
    /// function's `args_format`; a mismatch with `T` is `INVALID_ARGS`.
    pub fn args<T: DeserializeOwned>(&self) -> Result<T, ObserveRejection> {
        serde_json::from_value(self.inner.view.args.clone()).map_err(|error| {
            ObserveRejection::new(
                observe_codes::INVALID_ARGS,
                format!("observer {} args: {error}", self.id()),
            )
        })
    }

    /// The observer's args as JSON (`{}` when it has none).
    pub fn args_value(&self) -> &Value {
        &self.inner.view.args
    }

    /// The observer's filters in normal form (`{}` matches every call).
    /// [`Next`](crate::Next) and [`matches`](Self::matches) apply them.
    pub fn filters(&self) -> &Value {
        &self.inner.view.filters
    }

    /// The function each delivery invokes.
    pub fn target(&self) -> &str {
        &self.inner.view.target
    }

    /// What deliveries ship to the target.
    pub fn capture(&self) -> &[Capture] {
        &self.inner.view.capture
    }

    /// Which outcomes of a call reach it.
    pub fn on(&self) -> &On {
        &self.inner.view.on
    }

    /// Whether it is still delivered to: accepted, and neither removed nor
    /// rejected since.
    pub fn is_active(&self) -> bool {
        self.inner.active.load(Ordering::Acquire)
    }

    /// Whether `call` reaches this observer: its `on` accepts the call's
    /// status and its filters match the call. A deliver hook that calls
    /// targets itself applies them with this.
    pub fn matches(&self, call: &Call) -> bool {
        if !self.accepts(call.status == InvocationStatus::Success) {
            return false;
        }
        let filter = &self.inner.filter;
        if filter.is_match_all() {
            return true;
        }
        let parts = DocParts::of(filter.roots(), call.error.as_ref(), call.caller.as_ref());
        filter.matches(&parts.doc(
            Some(&call.input),
            call.output.as_ref(),
            call.metadata.as_ref(),
        ))
    }

    /// Resolves once the SDK has acked the observer, which makes it active:
    /// a call through it made after this resolves reaches an active observer,
    /// since the engine handles a connection's frames in order. A provider
    /// sends an observer its initial state this way. Fails with the
    /// rejection's code when the observer is rejected first.
    ///
    /// ```rust,no_run
    /// # use iii_sdk::{CallAction, CallRequest, IIIClient, Observer};
    /// # use serde_json::json;
    /// # fn demo(iii: &IIIClient) {
    /// let iii = iii.clone();
    /// iii.clone().on_observer_added_for("stock::on-changed", move |obs: Observer| {
    ///     let iii = iii.clone();
    ///     async move {
    ///         tokio::spawn(async move {
    ///             if obs.accepted().await.is_ok() {
    ///                 let initial = CallRequest::observer(&obs, json!({ "available": 12 }));
    ///                 let _ = iii.call(initial.action(CallAction::Void)).await;
    ///             }
    ///         });
    ///         Ok(())
    ///     }
    /// });
    /// # }
    /// ```
    pub async fn accepted(&self) -> Result<(), Error> {
        let mut acceptance = self.inner.acceptance.subscribe();
        let settled = acceptance
            .wait_for(|state| *state != Acceptance::Pending)
            .await
            .map(|state| state.clone());
        match settled {
            Ok(Acceptance::Refused(error)) => Err(coded_error(&error.code, error.message)),
            Ok(_) => Ok(()),
            Err(_) => Err(Error::NotConnected),
        }
    }

    /// Rejects this observer after it was accepted (a config reload removed
    /// what it needs). It stops receiving calls now; the engine moves it to
    /// `error` with this code and then removes it, which runs its removed
    /// hooks. The rejection holds across a reconnect: the SDK sends it again
    /// whenever the engine lists the observer, until a fresh `observeradded`
    /// brings it back.
    pub fn reject(&self, code: impl Into<String>, message: impl Into<String>) {
        let error = ErrorBody::new(code, message);
        // Held to the end: ordered against an added hook finishing.
        let mut rejection = self
            .inner
            .rejection
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        *rejection = Some(error.clone());
        if !self.inner.active.swap(false, Ordering::AcqRel) {
            // Not active (yet): its pending ack, or the next replay, carries it.
            return;
        }
        let Some(iii) = self.inner.client.upgrade().map(IIIClient::from_inner) else {
            return;
        };
        if let Some(entry) = iii.function_entry(&self.inner.function_id) {
            // Out of the set now; the hook task rebuilds it in order too.
            entry.drop_inactive();
            entry.enqueue(&iii, HookOp::Republish);
        }
        let sent = iii.send_message(Message::ObserverAck {
            function_id: self.inner.function_id.clone(),
            namespace: Some(self.inner.namespace.clone()),
            id: self.id().to_string(),
            revision: self.inner.revision.load(Ordering::Acquire),
            error: Some(error),
        });
        if let Err(error) = sent {
            // Sent again with the next `functionregistered` that lists it.
            tracing::warn!(observer_id = %self.id(), error = %error, "late rejection not sent");
        }
    }

    /// Acks an observer this SDK already holds again at `revision`, with its
    /// rejection if it has one, when `wanted(rejected)` says so. The check and
    /// the ack happen under the rejection lock, ordered with `reject()`.
    fn ack_again(
        &self,
        iii: &IIIClient,
        entry: &FunctionEntry,
        namespace: &str,
        revision: u64,
        wanted: impl FnOnce(bool) -> bool,
    ) -> bool {
        let rejection = self
            .inner
            .rejection
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if !wanted(rejection.is_some()) {
            return false;
        }
        self.inner.revision.store(revision, Ordering::Release);
        send_ack(
            iii,
            entry,
            namespace,
            self.id(),
            revision,
            rejection.clone(),
        );
        true
    }

    fn settle(&self, acceptance: Acceptance) {
        self.inner.acceptance.send_replace(acceptance);
    }

    pub(crate) fn mask(&self) -> CaptureMask {
        self.inner.mask
    }

    pub(crate) fn filter(&self) -> &Filter {
        &self.inner.filter
    }

    pub(crate) fn view(&self) -> &ObserverView {
        &self.inner.view
    }

    /// Whether a call that ended with `success` (or not) reaches it.
    pub(crate) fn accepts(&self, success: bool) -> bool {
        match self.inner.view.on {
            On::Success => success,
            On::Error => !success,
            On::Complete => true,
            On::Unknown(_) => false,
        }
    }
}

/// The parts of a filter document that are built for it: `error` and
/// `caller` as JSON, each only when a filter reads it.
pub(crate) struct DocParts {
    error: Option<Value>,
    caller: Option<Value>,
}

impl DocParts {
    pub(crate) fn of(
        roots: Roots,
        error: Option<&crate::observe::ObservedError>,
        caller: Option<&crate::protocol::Caller>,
    ) -> Self {
        Self {
            error: error
                .filter(|_| roots.error)
                .and_then(|error| serde_json::to_value(error).ok()),
            caller: caller
                .filter(|_| roots.caller)
                .and_then(|caller| serde_json::to_value(caller).ok()),
        }
    }

    pub(crate) fn doc<'a>(
        &'a self,
        input: Option<&'a Value>,
        output: Option<&'a Value>,
        metadata: Option<&'a Value>,
    ) -> Doc<'a> {
        Doc {
            input,
            output,
            error: self.error.as_ref(),
            caller: self.caller.as_ref(),
            metadata,
        }
    }
}

/// The active observers of a function, swapped whole on every change so the
/// hot path reads it with one load.
#[derive(Default)]
pub(crate) struct CompiledSet {
    pub observers: Vec<Observer>,
    /// The roots any observer's filter reads.
    pub roots: Roots,
    /// What any observer captures.
    pub capture: CaptureMask,
}

impl CompiledSet {
    fn new(observers: Vec<Observer>) -> Self {
        let mut roots = Roots::default();
        let mut capture = CaptureMask::default();
        for obs in &observers {
            let filter = obs.filter().roots();
            roots.input |= filter.input;
            roots.output |= filter.output;
            roots.error |= filter.error;
            roots.caller |= filter.caller;
            roots.metadata |= filter.metadata;
            capture = capture.union(obs.mask());
        }
        Self {
            observers,
            roots,
            capture,
        }
    }
}

/// Lock-free counters of local invocations, sent in `localinvocationstats`.
#[derive(Debug, Default)]
pub(crate) struct LocalStats {
    count: AtomicU64,
    errors: AtomicU64,
    duration_ns: AtomicU64,
}

impl LocalStats {
    pub(crate) fn record(&self, ok: bool, elapsed: Duration) {
        self.count.fetch_add(1, Ordering::Relaxed);
        if !ok {
            self.errors.fetch_add(1, Ordering::Relaxed);
        }
        self.duration_ns
            .fetch_add(elapsed.as_nanos() as u64, Ordering::Relaxed);
    }

    /// `(count, errors, duration_ms_sum)` since the last take.
    pub(crate) fn take(&self) -> (u64, u64, f64) {
        let count = self.count.swap(0, Ordering::Relaxed);
        let errors = self.errors.swap(0, Ordering::Relaxed);
        let duration_ns = self.duration_ns.swap(0, Ordering::Relaxed);
        (count, errors, duration_ns as f64 / 1_000_000.0)
    }
}

/// Observe state of one registered function.
pub(crate) struct FunctionEntry {
    pub(crate) id: String,
    /// Declares `args_format`, `private` or `observable: false`.
    declares_v2: bool,
    /// Registered with [`RegisterFunction::standby`](crate::RegisterFunction::standby).
    pub(crate) standby: bool,
    /// Has a handler here and was not registered with `local_dispatch(false)`.
    wants_local: bool,
    /// The namespace the engine registered it in (`functionregistered`).
    namespace: ArcSwapOption<String>,
    local_dispatch: AtomicBool,
    /// The connection on which `functionregistered` last arrived; 0 = never.
    confirmed_epoch: AtomicU64,
    /// Bumped by a `FUNCTION_NAMESPACE_CONFLICT`: a `functionregistered`
    /// queued before it must not turn local dispatch back on.
    revocation: AtomicU64,
    /// Another worker hosts the function since a conflict, until the engine
    /// confirms it again.
    revoked: AtomicBool,
    compiled: ArcSwap<CompiledSet>,
    /// The number of active observers, shifted left by one, with the low bit
    /// set while a deliver hook is: what a call reads first, in one atomic
    /// load.
    delivery: AtomicUsize,
    /// The function's deliver hook, mirrored from the hook store.
    deliver: ArcSwapOption<Arc<dyn DeliverHook>>,
    /// Serializes the rebuilds of `compiled` and `delivery`.
    publishing: Mutex<()>,
    pub(crate) stats: LocalStats,
    ops: Mutex<Option<mpsc::UnboundedSender<HookOp>>>,
    /// Opened when `register_function` returns: no observer is acked before.
    registered: watch::Sender<bool>,
    /// The connection its `registerfunction` was last sent on, or
    /// [`NOT_SENT`].
    sent_epoch: AtomicU64,
}

/// [`FunctionEntry::sent_epoch`] of a registration not sent on any
/// connection (epoch 0 is the one before the first connection).
const NOT_SENT: u64 = u64::MAX;

impl FunctionEntry {
    pub(crate) fn new(
        id: String,
        declares_v2: bool,
        has_handler: bool,
        local_option: bool,
        standby: bool,
        deliver: Option<Arc<dyn DeliverHook>>,
    ) -> Self {
        let (registered, _) = watch::channel(false);
        let delivery = usize::from(deliver.is_some());
        Self {
            id,
            declares_v2,
            standby,
            wants_local: has_handler && local_option,
            namespace: ArcSwapOption::empty(),
            local_dispatch: AtomicBool::new(false),
            confirmed_epoch: AtomicU64::new(0),
            revocation: AtomicU64::new(0),
            revoked: AtomicBool::new(false),
            compiled: ArcSwap::from_pointee(CompiledSet::default()),
            delivery: AtomicUsize::new(delivery),
            deliver: ArcSwapOption::new(deliver.map(Arc::new)),
            publishing: Mutex::new(()),
            stats: LocalStats::default(),
            ops: Mutex::new(None),
            registered,
            sent_epoch: AtomicU64::new(NOT_SENT),
        }
    }

    /// One atomic load.
    #[inline]
    pub(crate) fn is_observed(&self) -> bool {
        self.delivery.load(Ordering::Acquire) >> 1 != 0
    }

    /// Whether a call is delivered at all (an active observer, or a deliver
    /// hook): one atomic load.
    #[inline]
    pub(crate) fn delivers(&self) -> bool {
        self.delivery.load(Ordering::Acquire) != 0
    }

    /// Whether `call` would reach anyone: with a [`Keyed`] deliver hook, an
    /// observer is filed under one of its keys; otherwise any is active.
    pub(crate) fn is_observed_for(&self, call: &Call) -> bool {
        if !self.is_observed() {
            return false;
        }
        let deliver = self.deliver.load();
        match deliver.as_deref().and_then(|hook| hook.as_keyed()) {
            Some(keyed) => keyed.contains(call),
            None => true,
        }
    }

    pub(crate) fn deliver_hook(&self) -> Option<Arc<dyn DeliverHook>> {
        self.deliver.load().as_deref().cloned()
    }

    fn has_deliver(&self) -> bool {
        self.delivery.load(Ordering::Acquire) & 1 != 0
    }

    pub(crate) fn set_deliver(&self, hook: Option<Arc<dyn DeliverHook>>) {
        let _serialized = self.publishing.lock().unwrap_or_else(|e| e.into_inner());
        self.deliver.store(hook.map(Arc::new));
        self.refresh_delivery();
    }

    /// Runs `f` with the function's [`Keyed`] index, when it has one.
    fn with_keyed(&self, f: impl FnOnce(&Keyed)) {
        let deliver = self.deliver.load();
        if let Some(keyed) = deliver.as_deref().and_then(|hook| hook.as_keyed()) {
            f(keyed);
        }
    }

    /// Needs an engine that advertised `observe.v2`: it declares
    /// `args_format`, `private` or `observable: false`, or has a deliver hook.
    pub(crate) fn needs_v2(&self) -> bool {
        self.declares_v2 || self.has_deliver()
    }

    /// Records that its `registerfunction` goes out on connection `epoch`;
    /// `false` when it already went out there.
    pub(crate) fn mark_sent(&self, epoch: u64) -> bool {
        self.sent_epoch.swap(epoch, Ordering::AcqRel) != epoch
    }

    pub(crate) fn sent_on(&self, epoch: u64) -> bool {
        self.sent_epoch.load(Ordering::Acquire) == epoch
    }

    pub(crate) fn unmark_sent(&self) {
        self.sent_epoch.store(NOT_SENT, Ordering::Release);
    }

    /// `register_function` returned: its observers may be acked.
    pub(crate) fn open(&self) {
        self.registered.send_replace(true);
    }

    pub(crate) fn compiled(&self) -> Arc<CompiledSet> {
        self.compiled.load_full()
    }

    pub(crate) fn observers(&self) -> Vec<Observer> {
        self.compiled
            .load()
            .observers
            .iter()
            .filter(|o| o.is_active())
            .cloned()
            .collect()
    }

    /// Local dispatch is on for it: confirmed by the engine with
    /// `local_dispatch: true`, wanted, and not revoked by a conflict.
    #[inline]
    pub(crate) fn local_dispatch(&self) -> bool {
        self.local_dispatch.load(Ordering::Acquire)
    }

    pub(crate) fn namespace(&self) -> Option<Arc<String>> {
        self.namespace.load_full()
    }

    /// Turns local dispatch off and forgets the confirmation: a
    /// `FUNCTION_NAMESPACE_CONFLICT`, or the function was unregistered.
    pub(crate) fn revoke(&self) {
        self.local_dispatch.store(false, Ordering::Release);
        self.confirmed_epoch.store(0, Ordering::Release);
    }

    /// The function is not this worker's any more: another worker hosts it
    /// (`FUNCTION_NAMESPACE_CONFLICT`) or it was unregistered. Ordered
    /// against `functionregistered` messages already queued, and against
    /// added hooks still running.
    pub(crate) fn release(&self) {
        self.revocation.fetch_add(1, Ordering::AcqRel);
        self.revoked.store(true, Ordering::Release);
        self.revoke();
    }

    pub(crate) fn revocation(&self) -> u64 {
        self.revocation.load(Ordering::Acquire)
    }

    pub(crate) fn is_revoked(&self) -> bool {
        self.revoked.load(Ordering::Acquire)
    }

    pub(crate) fn confirmed_on(&self, epoch: u64) -> bool {
        epoch != 0 && self.confirmed_epoch.load(Ordering::Acquire) == epoch
    }

    fn refresh_delivery(&self) {
        let observers = self.compiled.load().observers.len();
        let hook = usize::from(self.deliver.load().is_some());
        self.delivery
            .store(observers << 1 | hook, Ordering::Release);
    }

    /// Removes deactivated observers from the compiled set.
    fn drop_inactive(&self) {
        let _serialized = self.publishing.lock().unwrap_or_else(|e| e.into_inner());
        let observers: Vec<Observer> = self
            .compiled
            .load()
            .observers
            .iter()
            .filter(|o| o.is_active())
            .cloned()
            .collect();
        self.compiled.store(Arc::new(CompiledSet::new(observers)));
        self.refresh_delivery();
    }

    fn publish(&self, members: &BTreeMap<String, Member>) {
        let _serialized = self.publishing.lock().unwrap_or_else(|e| e.into_inner());
        let observers: Vec<Observer> = members
            .values()
            .map(|member| &member.observer)
            .filter(|o| o.is_active())
            .cloned()
            .collect();
        self.compiled.store(Arc::new(CompiledSet::new(observers)));
        self.refresh_delivery();
    }

    /// Queues `op` on this function's hook task, spawning it on first use.
    /// Hooks of one function run one at a time, in arrival order.
    pub(crate) fn enqueue(self: &Arc<Self>, iii: &IIIClient, op: HookOp) {
        let mut ops = self.ops.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(tx) = ops.as_ref() {
            match tx.send(op) {
                Ok(()) => return,
                Err(mpsc::error::SendError(op)) => {
                    *ops = None;
                    return self.spawn_and_send(&mut ops, iii, op);
                }
            }
        }
        self.spawn_and_send(&mut ops, iii, op)
    }

    fn spawn_and_send(
        self: &Arc<Self>,
        ops: &mut Option<mpsc::UnboundedSender<HookOp>>,
        iii: &IIIClient,
        op: HookOp,
    ) {
        if matches!(
            op,
            HookOp::Reset { .. } | HookOp::Republish | HookOp::CatchUp(_)
        ) {
            // Nothing was ever added: there is nothing to reset or catch up.
            return;
        }
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            tracing::warn!(function_id = %self.id, "observe hooks need a tokio runtime; message dropped");
            return;
        };
        let (tx, rx) = mpsc::unbounded_channel();
        let _ = tx.send(op);
        runtime.spawn(run_hooks(self.clone(), Arc::downgrade(&iii.inner), rx));
        *ops = Some(tx);
    }
}

/// What a hook set late runs for the observers already active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CatchUp {
    /// An added hook of this level was set: it runs for each active observer
    /// that did not run it.
    Added(Level),
    /// A [`Keyed`] deliver hook was set: every active observer is filed in it.
    Index,
}

/// One step of a function's observer set, processed in order by its hook
/// task.
pub(crate) enum HookOp {
    /// `functionregistered`: always accepted, replaces the set.
    Registered {
        /// [`FunctionEntry::revocation`] when it arrived.
        revocation: u64,
        epoch: u64,
        namespace: String,
        revision: u64,
        local_dispatch: bool,
        observers: Vec<ObserverView>,
    },
    Added {
        namespace: String,
        revision: u64,
        observer: ObserverView,
    },
    Removed {
        revision: u64,
        id: String,
    },
    /// An observer was rejected late: rebuild the compiled set.
    Republish,
    /// Drop the whole set: a `FUNCTION_NAMESPACE_CONFLICT`, or the function
    /// was unregistered (`last`).
    Reset {
        last: bool,
    },
    CatchUp(CatchUp),
}

/// An accepted observer, and the added hooks that ran for it, by the
/// generation of the hook at each level.
struct Member {
    observer: Observer,
    worker: Option<u64>,
    function: Option<u64>,
}

impl Member {
    fn ran(&self, level: Level) -> Option<u64> {
        match level {
            Level::Worker => self.worker,
            Level::Function => self.function,
        }
    }
}

/// The added hooks that resolved for an observer being added.
#[derive(Default, Clone, Copy)]
struct Resolved {
    worker: Option<u64>,
    function: Option<u64>,
}

async fn run_hooks(
    entry: Arc<FunctionEntry>,
    client: Weak<IIIInner>,
    mut ops: mpsc::UnboundedReceiver<HookOp>,
) {
    // No observer is acked before `register_function` returned.
    let mut registered = entry.registered.subscribe();
    if registered.wait_for(|open| *open).await.is_err() {
        return;
    }
    let mut members: BTreeMap<String, Member> = BTreeMap::new();
    let mut revision: u64 = 0;
    while let Some(op) = ops.recv().await {
        let Some(inner) = client.upgrade() else {
            return;
        };
        let iii = IIIClient::from_inner(inner);
        match op {
            HookOp::Registered {
                revocation,
                epoch,
                namespace,
                revision: rev,
                local_dispatch,
                observers,
            } => {
                if entry.revocation() != revocation {
                    // A conflict arrived after it: the function is not ours.
                    tracing::debug!(function_id = %entry.id, "functionregistered superseded by a conflict");
                    continue;
                }
                entry.revoked.store(false, Ordering::Release);
                entry.namespace.store(Some(Arc::new(namespace.clone())));
                entry
                    .local_dispatch
                    .store(local_dispatch && entry.wants_local, Ordering::Release);

                let listed: HashSet<String> = observers.iter().map(|v| v.id.clone()).collect();
                // The ones this SDK holds first, without hooks: acked again
                // (the engine holds a replayed set pending until it is). A
                // late rejection is sent again and keeps its observer out,
                // whatever the replayed view says, until a fresh
                // `observeradded`.
                let mut fresh = Vec::new();
                for view in observers {
                    let acked = members.get(&view.id).is_some_and(|held| {
                        held.observer
                            .ack_again(&iii, &entry, &namespace, rev, |rejected| {
                                rejected || held.observer.view() == &view
                            })
                    });
                    if !acked {
                        fresh.push(view);
                    }
                }
                // Reports made while disconnected matched those observers
                // only: they go out now, behind the acks and before any hook
                // runs, as does everything reported from here on.
                entry.confirmed_epoch.store(epoch, Ordering::Release);
                iii.reporter().unpark(&entry.id);

                let gone: Vec<String> = members
                    .keys()
                    .filter(|id| !listed.contains(*id))
                    .cloned()
                    .collect();
                for id in gone {
                    remove_member(&iii, &entry, &mut members, &id).await;
                }
                for view in fresh {
                    // a changed view: removed, then added as a new observer
                    if members.contains_key(&view.id) {
                        remove_member(&iii, &entry, &mut members, &view.id).await;
                    }
                    add_member(&iii, &entry, &mut members, view, &namespace, rev).await;
                }
                revision = rev;
            }
            HookOp::Added {
                namespace,
                revision: rev,
                observer,
            } => {
                if rev <= revision {
                    tracing::debug!(function_id = %entry.id, revision = rev, held = revision, "stale observeradded ignored");
                    continue;
                }
                let acked = members.get(&observer.id).is_some_and(|held| {
                    held.observer
                        .ack_again(&iii, &entry, &namespace, rev, |rejected| {
                            !rejected && held.observer.view() == &observer
                        })
                });
                if !acked {
                    // New, changed, or rejected earlier: a fresh observer.
                    if members.contains_key(&observer.id) {
                        remove_member(&iii, &entry, &mut members, &observer.id).await;
                    }
                    add_member(&iii, &entry, &mut members, observer, &namespace, rev).await;
                }
                revision = rev;
            }
            HookOp::Removed { revision: rev, id } => {
                if rev <= revision {
                    tracing::debug!(function_id = %entry.id, revision = rev, held = revision, "stale observerremoved ignored");
                    continue;
                }
                remove_member(&iii, &entry, &mut members, &id).await;
                revision = rev;
            }
            HookOp::Republish => entry.publish(&members),
            HookOp::Reset { last } => {
                entry.revoke();
                let ids: Vec<String> = members.keys().cloned().collect();
                for id in ids {
                    remove_member(&iii, &entry, &mut members, &id).await;
                }
                revision = 0;
                if last {
                    return;
                }
            }
            HookOp::CatchUp(CatchUp::Added(level)) => {
                catch_up(&iii, &entry, &mut members, level).await;
            }
            HookOp::CatchUp(CatchUp::Index) => entry.with_keyed(|keyed| {
                for member in members.values() {
                    if member.observer.is_active() {
                        keyed.file(&member.observer);
                    }
                }
            }),
        }
    }
}

fn send_ack(
    iii: &IIIClient,
    entry: &FunctionEntry,
    namespace: &str,
    id: &str,
    revision: u64,
    error: Option<ErrorBody>,
) {
    let _ = iii.send_message(Message::ObserverAck {
        function_id: entry.id.clone(),
        namespace: Some(namespace.to_string()),
        id: id.to_string(),
        revision,
        error,
    });
}

/// Runs the added hooks that apply (the worker's, then the function's),
/// puts the observer in the compiled set, then acks it: the first call after
/// the engine marks it active is delivered. A hook that rejects it, or a
/// `reject()` or release while they ran, undoes the levels that resolved.
async fn add_member(
    iii: &IIIClient,
    entry: &Arc<FunctionEntry>,
    members: &mut BTreeMap<String, Member>,
    view: ObserverView,
    namespace: &str,
    revision: u64,
) {
    // Already validated and normalized by the engine: no size limit here.
    let filter = match Filter::from_normalized(&view.filters) {
        Ok(filter) => filter,
        Err(error) => {
            let body = ErrorBody::new(error.code.as_str(), error.message);
            return send_ack(iii, entry, namespace, &view.id, revision, Some(body));
        }
    };
    let id = view.id.clone();
    let observer = Observer::new(
        view,
        filter,
        &entry.id,
        namespace.to_string(),
        revision,
        Arc::downgrade(&iii.inner),
    );
    let revocation = entry.revocation();
    let deadline = tokio::time::Instant::now() + iii.hook_timeout();
    let mut resolved = Resolved::default();
    let [worker, function] = iii.hooks().added(&entry.id);
    for (level, slot) in [(Level::Worker, worker), (Level::Function, function)] {
        let Some(slot) = slot else {
            continue;
        };
        match run_added(iii, &slot, &observer, deadline).await {
            Ok(()) => match level {
                Level::Worker => resolved.worker = Some(slot.generation),
                Level::Function => resolved.function = Some(slot.generation),
            },
            Err(rejection) => {
                tracing::debug!(function_id = %entry.id, observer_id = %id, code = %rejection.code, "observer rejected by an added hook");
                let body = ErrorBody::new(rejection.code, rejection.message);
                unwind(iii, entry, &observer, resolved).await;
                observer.settle(Acceptance::Refused(body.clone()));
                return send_ack(iii, entry, namespace, &id, revision, Some(body));
            }
        }
    }
    if entry.revocation() != revocation {
        // The function was released while the hooks ran: undo the add (no
        // ack, never active), as if it had been removed.
        tracing::debug!(function_id = %entry.id, observer_id = %id, "function released during the added hooks");
        unwind(iii, entry, &observer, resolved).await;
        observer.settle(Acceptance::Refused(ErrorBody::new(
            observe_codes::OBSERVER_NOT_ACTIVE,
            format!("{} is not hosted here any more", entry.id),
        )));
        return;
    }
    // A `reject()` made while the hooks ran wins. Holding the rejection lock
    // through the ack keeps a concurrent `reject()` strictly after it (a late
    // rejection the engine handles), never between activation and ack.
    let rejected = {
        let rejection = observer
            .inner
            .rejection
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if rejection.is_none() {
            // Filed, acked, then published, in one synchronous step: the ack
            // is queued ahead of any report the new observer matches, and the
            // connection loop (same thread) only sends it after this task
            // yields, by when the observer is compiled in.
            observer.inner.active.store(true, Ordering::Release);
            entry.with_keyed(|keyed| keyed.file(&observer));
            send_ack(iii, entry, namespace, &id, revision, None);
            members.insert(
                id.clone(),
                Member {
                    observer: observer.clone(),
                    worker: resolved.worker,
                    function: resolved.function,
                },
            );
            entry.publish(members);
        }
        rejection.clone()
    };
    match rejected {
        None => observer.settle(Acceptance::Accepted),
        Some(error) => {
            tracing::debug!(function_id = %entry.id, observer_id = %id, code = %error.code, "observer rejected during its added hooks");
            unwind(iii, entry, &observer, resolved).await;
            observer.settle(Acceptance::Refused(error.clone()));
            send_ack(iii, entry, namespace, &id, revision, Some(error));
        }
    }
}

/// Runs one added hook within the observer's deadline. A panic is
/// `OBSERVE_REJECTED`, a hook past the deadline `OWNER_TIMEOUT`.
async fn run_added(
    iii: &IIIClient,
    slot: &AddedSlot,
    observer: &Observer,
    deadline: tokio::time::Instant,
) -> Result<(), ObserveRejection> {
    let added = std::panic::AssertUnwindSafe(slot.hook.on_added(observer.clone())).catch_unwind();
    match tokio::time::timeout_at(deadline, added).await {
        Ok(Ok(result)) => result,
        Ok(Err(panic)) => Err(ObserveRejection::new(
            observe_codes::OBSERVE_REJECTED,
            format!("on_observer_added panicked: {}", panic_message(&panic)),
        )),
        Err(_) => Err(ObserveRejection::new(
            observe_codes::OWNER_TIMEOUT,
            format!(
                "on_observer_added for observer {} did not finish within {:?}",
                observer.id(),
                iii.hook_timeout()
            ),
        )),
    }
}

/// The removed hooks of the levels whose added hook resolved, the
/// function's then the worker's: what they set up for an observer that was
/// not accepted is released.
async fn unwind(iii: &IIIClient, entry: &FunctionEntry, observer: &Observer, resolved: Resolved) {
    let [function, worker] = iii.hooks().removed(&entry.id);
    let undo = [
        function.filter(|_| resolved.function.is_some()),
        worker.filter(|_| resolved.worker.is_some()),
    ];
    for hook in undo.into_iter().flatten() {
        run_removed(iii, entry, &hook, observer).await;
    }
}

/// Takes an accepted observer out, then runs the removed hooks, the
/// function's then the worker's: exactly once per accepted observer.
async fn remove_member(
    iii: &IIIClient,
    entry: &Arc<FunctionEntry>,
    members: &mut BTreeMap<String, Member>,
    id: &str,
) {
    let Some(member) = members.remove(id) else {
        return;
    };
    member.observer.inner.active.store(false, Ordering::Release);
    entry.with_keyed(|keyed| keyed.unfile(id));
    entry.publish(members);
    for hook in iii.hooks().removed(&entry.id).into_iter().flatten() {
        run_removed(iii, entry, &hook, &member.observer).await;
    }
}

async fn run_removed(
    iii: &IIIClient,
    entry: &FunctionEntry,
    hook: &Arc<dyn RemovedHook>,
    observer: &Observer,
) {
    let removed = std::panic::AssertUnwindSafe(hook.on_removed(observer.clone())).catch_unwind();
    match tokio::time::timeout(iii.hook_timeout(), removed).await {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => tracing::warn!(
            function_id = %entry.id,
            observer_id = %observer.id(),
            "on_observer_removed panicked: {}",
            panic_message(&panic)
        ),
        Err(_) => tracing::warn!(
            function_id = %entry.id,
            observer_id = %observer.id(),
            "on_observer_removed did not finish within {:?}",
            iii.hook_timeout()
        ),
    }
}

/// Runs the added hook of `level`, set once observers were already active,
/// for each of them that did not run it. One that fails is rejected late.
/// Logs one warning with the number of observers it ran for.
async fn catch_up(
    iii: &IIIClient,
    entry: &Arc<FunctionEntry>,
    members: &mut BTreeMap<String, Member>,
    level: Level,
) {
    let Some(slot) = iii.hooks().added_at(level, &entry.id) else {
        return;
    };
    let behind: Vec<Observer> = members
        .values()
        .filter(|member| member.observer.is_active() && member.ran(level) != Some(slot.generation))
        .map(|member| member.observer.clone())
        .collect();
    if behind.is_empty() {
        return;
    }
    tracing::warn!(
        function_id = %entry.id,
        observers = behind.len(),
        "{} observer(s) of {} were active before its on_observer_added hook was set; it runs \
         for each of them now. Set hooks by function id before registering the function.",
        behind.len(),
        entry.id
    );
    for observer in behind {
        let deadline = tokio::time::Instant::now() + iii.hook_timeout();
        match run_added(iii, &slot, &observer, deadline).await {
            Ok(()) => {
                if let Some(member) = members.get_mut(observer.id()) {
                    match level {
                        Level::Worker => member.worker = Some(slot.generation),
                        Level::Function => member.function = Some(slot.generation),
                    }
                }
            }
            Err(rejection) => observer.reject(rejection.code, rejection.message),
        }
    }
}

pub(crate) fn panic_message(panic: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = panic.downcast_ref::<&str>() {
        message.to_string()
    } else if let Some(message) = panic.downcast_ref::<String>() {
        message.clone()
    } else {
        "unknown panic".to_string()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use serde_json::{Value, json};
    use tokio::sync::mpsc::UnboundedReceiver;

    use super::*;
    use crate::{
        CallRequest, RegisterFunction,
        iii::{IIIConnectionState, MutexExt, Outbound},
    };

    /// A client that is "running" but not connected: what it sends lands in
    /// the returned receiver, and the test plays the engine through
    /// `handle_message`.
    fn client() -> (IIIClient, UnboundedReceiver<Outbound>) {
        let iii = IIIClient::new("ws://127.0.0.1:0");
        iii.inner.running.store(true, Ordering::SeqCst);
        let rx = iii.inner.receiver.lock_or_recover().take().unwrap();
        (iii, rx)
    }

    fn sent(rx: &mut UnboundedReceiver<Outbound>) -> Vec<Value> {
        let mut out = Vec::new();
        while let Ok(Outbound::Message(envelope)) = rx.try_recv() {
            out.push(serde_json::from_str(envelope.payload.as_str()).unwrap());
        }
        out
    }

    async fn next_ack(rx: &mut UnboundedReceiver<Outbound>) -> Value {
        loop {
            let Some(Outbound::Message(envelope)) = rx.recv().await else {
                panic!("sender gone");
            };
            let value: Value = serde_json::from_str(envelope.payload.as_str()).unwrap();
            if value["type"] == "observerack" {
                return value;
            }
        }
    }

    fn feed(iii: &IIIClient, message: Value) {
        iii.handle_message(&message.to_string()).unwrap();
    }

    fn echo() -> RegisterFunction {
        RegisterFunction::new(|v: Value| Ok::<_, Error>(v))
    }

    fn added(revision: u64, id: &str) -> Value {
        json!({
            "type": "observeradded", "function_id": "h::f", "revision": revision,
            "observer": { "id": id, "target": "t", "filters": {} }
        })
    }

    #[tokio::test(start_paused = true)]
    async fn a_hook_past_its_timeout_is_owner_timeout_and_a_panic_is_rejected() {
        let (iii, mut rx) = client();
        iii.on_observer_added_for("h::f", |obs: Observer| async move {
            match obs.id() {
                "hangs" => std::future::pending::<()>().await,
                "panics" => panic!("hook blew up"),
                _ => {}
            }
            Ok(())
        });
        iii.register_function("h::f", echo());
        feed(&iii, added(1, "hangs"));
        let ack = next_ack(&mut rx).await;
        assert_eq!(ack["id"], "hangs");
        assert_eq!(ack["error"]["code"], "OWNER_TIMEOUT");
        feed(&iii, added(2, "panics"));
        let ack = next_ack(&mut rx).await;
        assert_eq!(ack["error"]["code"], "OBSERVE_REJECTED");
        assert!(
            ack["error"]["message"]
                .as_str()
                .unwrap()
                .contains("hook blew up")
        );
        feed(&iii, added(3, "fine"));
        let ack = next_ack(&mut rx).await;
        assert_eq!(ack["id"], "fine");
        assert!(ack.get("error").is_none());
    }

    /// The worker's and the function's added hooks share one 30 s budget.
    #[tokio::test(start_paused = true)]
    async fn the_timeout_spans_both_levels() {
        let (iii, mut rx) = client();
        iii.on_observer_added(|_obs: Observer| async move {
            tokio::time::sleep(Duration::from_secs(20)).await;
            Ok(())
        });
        iii.on_observer_added_for("h::f", |_obs: Observer| async move {
            tokio::time::sleep(Duration::from_secs(20)).await;
            Ok(())
        });
        iii.register_function("h::f", echo());
        feed(&iii, added(1, "slow"));
        let ack = next_ack(&mut rx).await;
        assert_eq!(ack["error"]["code"], "OWNER_TIMEOUT");
    }

    /// Functions that need `observe.v2` stay unregistered on an engine
    /// without it, logged once each; the next engine with it registers them.
    #[tokio::test]
    #[tracing_test::traced_test]
    async fn an_engine_without_observe_v2_leaves_v2_functions_unregistered() {
        let (iii, mut rx) = client();
        iii.register_function("o::private", echo().private());
        iii.register_function("o::plain", echo());
        iii.on_observer_deliver_for(
            "o::hooked",
            |call: Call, observers: Vec<Observer>, next: crate::Next| async move {
                next.run(&call, &observers)
            },
        );
        iii.register_function("o::hooked", echo());
        let registered = |frames: &[Value]| -> Vec<String> {
            frames
                .iter()
                .filter(|m| m["type"] == "registerfunction")
                .map(|m| m["id"].as_str().unwrap().to_string())
                .collect()
        };
        assert_eq!(
            registered(&sent(&mut rx)),
            vec!["o::plain"],
            "held until the hello"
        );

        let v1 = json!({ "type": "workerregistered", "worker_id": "w",
                         "features": ["observe.v1", "local-dispatch.v1"] });
        feed(&iii, v1.clone());
        feed(&iii, v1.clone());
        assert!(registered(&sent(&mut rx)).is_empty());
        // registered once the engine said it has no observe.v2: warned too
        iii.register_function(
            "o::late",
            echo().args_format_value(json!({ "type": "object" })),
        );
        assert!(registered(&sent(&mut rx)).is_empty());
        let warned = |lines: &[&str]| {
            let named = |id: &str| {
                lines
                    .iter()
                    .filter(|l| l.contains("UNSUPPORTED") && l.contains(&format!("{id} needs")))
                    .count()
            };
            match (
                named("o::private"),
                named("o::hooked"),
                named("o::late"),
                named("o::plain"),
            ) {
                (1, 1, 1, 0) => Ok(()),
                other => Err(format!("expected one line each, got {other:?}")),
            }
        };
        logs_assert(warned);

        // a plain function that gets a deliver hook is withdrawn
        iii.on_observer_deliver_for(
            "o::plain",
            |call: Call, observers: Vec<Observer>, next: crate::Next| async move {
                next.run(&call, &observers)
            },
        );
        let frames = sent(&mut rx);
        assert_eq!(frames.len(), 1);
        assert_eq!(
            frames[0],
            json!({ "type": "unregisterfunction", "id": "o::plain" })
        );

        // an engine with observe.v2 registers them all
        feed(
            &iii,
            json!({ "type": "workerregistered", "worker_id": "w",
                    "features": ["observe.v1", "observe.v2", "local-dispatch.v1"] }),
        );
        let mut ids = registered(&sent(&mut rx));
        ids.sort();
        assert_eq!(ids, vec!["o::hooked", "o::late", "o::plain", "o::private"]);
        let private = iii
            .functions()
            .get("o::private")
            .map(|data| serde_json::to_value(data.message.to_message()).unwrap())
            .unwrap();
        assert_eq!(private["private"], true);
    }

    /// Once the connection is registered, the SDK names each hook id this
    /// worker has not registered, once.
    #[tokio::test]
    #[tracing_test::traced_test]
    async fn hooks_for_an_unregistered_id_are_named_once() {
        let (iii, _rx) = client();
        iii.on_observer_added_for("typo::f", |_obs: Observer| async move { Ok(()) });
        iii.on_observer_removed_for("real::f", |_obs: Observer| async move {});
        iii.register_function("real::f", echo());
        let hello = json!({ "type": "workerregistered", "worker_id": "w",
                            "features": ["observe.v1", "observe.v2"] });
        feed(&iii, hello.clone());
        feed(&iii, hello);
        logs_assert(|lines: &[&str]| {
            let named: Vec<&&str> = lines
                .iter()
                .filter(|l| l.contains("which this worker has not registered"))
                .collect();
            match named.as_slice() {
                [line] if line.contains("typo::f") => Ok(()),
                other => Err(format!("expected one line naming typo::f, got {other:?}")),
            }
        });
    }

    /// Records the warnings of the current thread.
    #[derive(Clone, Default)]
    struct Warnings(Arc<Mutex<Vec<String>>>);

    impl tracing::Subscriber for Warnings {
        fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
            *metadata.level() <= tracing::Level::WARN
        }
        fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }
        fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
        fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
        fn event(&self, event: &tracing::Event<'_>) {
            struct Line(String);
            impl tracing::field::Visit for Line {
                fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn fmt::Debug) {
                    use std::fmt::Write;
                    let _ = write!(self.0, "{}={:?} ", field.name(), value);
                }
            }
            let mut line = Line(String::new());
            event.record(&mut line);
            self.0.lock().unwrap().push(line.0);
        }
        fn enter(&self, _: &tracing::span::Id) {}
        fn exit(&self, _: &tracing::span::Id) {}
    }

    /// An added hook set once observers are active runs for each of them,
    /// with one warning naming the function and how many.
    #[tokio::test]
    async fn a_late_hook_is_caught_up_with_one_warning() {
        let warnings = Warnings::default();
        let _capture = tracing::subscriber::set_default(warnings.clone());
        let (iii, mut rx) = client();
        let function = iii.register_function("late::f", echo());
        feed(
            &iii,
            json!({ "type": "functionregistered", "id": "late::f", "revision": 1, "local_dispatch": true,
                    "observers": [{ "id": "o1", "target": "t", "filters": {} },
                                  { "id": "o2", "target": "t", "filters": {} }] }),
        );
        next_ack(&mut rx).await;
        next_ack(&mut rx).await;
        while function.observers().len() < 2 {
            tokio::task::yield_now().await;
        }
        let ran = Arc::new(AtomicUsize::new(0));
        function.on_observer_added({
            let ran = ran.clone();
            move |_obs: Observer| {
                let ran = ran.clone();
                async move {
                    ran.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                }
            }
        });
        while ran.load(Ordering::SeqCst) < 2 {
            tokio::task::yield_now().await;
        }
        let lines = warnings.0.lock().unwrap().clone();
        let caught: Vec<&String> = lines
            .iter()
            .filter(|l| l.contains("were active before its on_observer_added hook was set"))
            .collect();
        assert_eq!(caught.len(), 1, "{lines:#?}");
        assert!(
            caught[0].contains("2 observer(s) of late::f"),
            "{}",
            caught[0]
        );

        // cleared: nothing runs again
        function.clear_observer_added();
        tokio::task::yield_now().await;
        assert_eq!(ran.load(Ordering::SeqCst), 2);
    }

    fn deliver_and_keep_next(
        kept: Arc<Mutex<Option<(Call, Vec<Observer>, crate::Next)>>>,
    ) -> impl crate::DeliverHook {
        move |call: Call, observers: Vec<Observer>, next: crate::Next| {
            let kept = kept.clone();
            async move {
                // twice: each observer is delivered at most once per call
                next.run(&call, &observers)?;
                let result = next.run(&call, &observers);
                *kept.lock().unwrap() = Some((call, observers, next));
                result
            }
        }
    }

    fn v2_hello(iii: &IIIClient) {
        feed(
            iii,
            json!({ "type": "workerregistered", "worker_id": "w",
                    "features": ["observe.v1", "observe.v2", "local-dispatch.v1"] }),
        );
    }

    async fn next_frame(rx: &mut UnboundedReceiver<Outbound>, ty: &str) -> Value {
        loop {
            let Some(Outbound::Message(envelope)) = rx.recv().await else {
                panic!("sender gone");
            };
            let value: Value = serde_json::from_str(envelope.payload.as_str()).unwrap();
            if value["type"] == ty {
                return value;
            }
        }
    }

    /// A routed call delivers each observer once, however many times `next`
    /// runs; a `next` after the hook settled delivers nothing and counts as
    /// `LATE_DELIVERY`.
    #[tokio::test]
    async fn a_next_after_the_hook_settled_is_late() {
        let (iii, mut rx) = client();
        v2_hello(&iii);
        let kept = Arc::new(Mutex::new(None));
        iii.on_observer_deliver_for("late::routed", deliver_and_keep_next(kept.clone()));
        let function = iii.register_function("late::routed", echo());
        feed(
            &iii,
            json!({ "type": "functionregistered", "id": "late::routed", "revision": 1,
                    "local_dispatch": true,
                    "observers": [{ "id": "o", "target": "t", "filters": {} }] }),
        );
        next_ack(&mut rx).await;
        while !function.is_observed() {
            tokio::task::yield_now().await;
        }
        feed(
            &iii,
            json!({ "type": "invokefunction", "invocation_id": "11111111-2222-4333-8444-555555555555",
                    "function_id": "late::routed", "data": { "n": 1 } }),
        );
        let answer = next_frame(&mut rx, "invocationresult").await;
        assert_eq!(answer["result"], json!({ "n": 1 }));
        assert_eq!(answer["observed"], json!(["o"]), "once");

        let (call, observers, next) = kept.lock().unwrap().take().unwrap();
        assert_eq!(next.run(&call, &observers).unwrap(), json!({ "n": 1 }));
        assert_eq!(
            iii.reporter().take_drops(),
            vec![("late::routed".to_string(), "LATE_DELIVERY", 1)]
        );
        assert!(sent(&mut rx).is_empty(), "a late next sends nothing");
    }

    /// The report items of a routed call whose connection is gone are not
    /// sent: they count as `DISCONNECTED`.
    #[tokio::test]
    async fn routed_items_of_a_closed_connection_are_disconnected() {
        let (iii, mut rx) = client();
        v2_hello(&iii);
        iii.on_observer_deliver_for(
            "gone::f",
            |call: Call, observers: Vec<Observer>, next: crate::Next| async move {
                next.run(&call, &observers)?;
                Ok(json!("the hook's own answer"))
            },
        );
        let function = iii.register_function("gone::f", echo());
        feed(
            &iii,
            json!({ "type": "functionregistered", "id": "gone::f", "revision": 1,
                    "local_dispatch": true,
                    "observers": [{ "id": "o", "target": "t", "filters": {} }] }),
        );
        next_ack(&mut rx).await;
        while !function.is_observed() {
            tokio::task::yield_now().await;
        }
        feed(
            &iii,
            json!({ "type": "invokefunction", "invocation_id": "21111111-2222-4333-8444-555555555555",
                    "function_id": "gone::f", "data": { "n": 1 } }),
        );
        let answer = next_frame(&mut rx, "invocationresult").await;
        assert_eq!(answer["result"], "the hook's own answer");
        assert!(answer.get("observed").is_none());
        assert_eq!(
            iii.reporter().take_drops(),
            vec![("gone::f".to_string(), "DISCONNECTED", 1)]
        );
    }

    /// A peer that stops reading: flushed frames wait in the outbound channel,
    /// and their bytes keep counting against the 8 MiB bound.
    #[tokio::test]
    async fn frames_a_peer_does_not_read_stay_within_the_bound() {
        let (iii, _unread) = client();
        let function =
            iii.register_function("b::f", RegisterFunction::new(|v: Value| Ok::<_, Error>(v)));
        iii.set_connection_state(IIIConnectionState::Connected);
        iii.inner.connection_epoch.store(1, Ordering::SeqCst);
        feed(
            &iii,
            json!({ "type": "functionregistered", "id": "b::f", "revision": 1, "local_dispatch": true,
                    "observers": [{ "id": "o", "target": "t", "filters": {}, "capture": ["input"] }] }),
        );
        while !function.is_observed() || !function.local_dispatch_active() {
            tokio::task::yield_now().await;
        }
        let payload = json!({ "blob": "x".repeat(10_000) });
        for i in 0..3_000 {
            iii.call(CallRequest::new("b::f", payload.clone()))
                .await
                .unwrap();
            if i % 100 == 0 {
                iii.flush_reports();
            }
        }
        iii.flush_reports();
        let in_flight = iii.reporter().in_flight();
        assert!(in_flight > 0, "frames were flushed into the unread channel");
        assert!(
            in_flight + iii.reporter().len() * 10_000
                <= crate::reporter::MAX_BYTES + 2 * 1024 * 1024,
            "{in_flight} bytes in flight"
        );
        let drops = iii.reporter().take_drops();
        assert!(
            drops
                .iter()
                .any(|(f, reason, n)| f == "b::f" && *reason == "BUFFER_FULL" && *n > 0),
            "{drops:?}"
        );
    }

    /// A `FUNCTION_NAMESPACE_CONFLICT` after a `functionregistered` still
    /// queued for the hook task wins: local dispatch stays off.
    #[tokio::test]
    async fn a_conflict_is_ordered_after_a_queued_functionregistered() {
        let (iii, _rx) = client();
        let function =
            iii.register_function("r::f", RegisterFunction::new(|v: Value| Ok::<_, Error>(v)));
        feed(
            &iii,
            json!({ "type": "functionregistered", "id": "r::f", "revision": 1,
                    "local_dispatch": true, "observers": [] }),
        );
        feed(
            &iii,
            json!({ "type": "registrationrejected", "code": "FUNCTION_NAMESPACE_CONFLICT",
                    "namespace": "default", "function_id": "r::f", "owner_worker_id": "w2" }),
        );
        for _ in 0..50 {
            tokio::task::yield_now().await;
        }
        assert!(!function.local_dispatch_active());
        // A later confirmation (the function is ours again) turns it back on.
        feed(
            &iii,
            json!({ "type": "functionregistered", "id": "r::f", "revision": 2,
                    "local_dispatch": true, "observers": [] }),
        );
        while !function.local_dispatch_active() {
            tokio::task::yield_now().await;
        }
    }

    #[tokio::test]
    async fn unknown_messages_are_not_errors() {
        let (iii, _rx) = client();
        assert!(
            iii.handle_message(r#"{"type":"fromthefuture","x":1}"#)
                .is_ok()
        );
    }

    #[tokio::test]
    async fn stats_are_sent_only_when_non_zero() {
        let (iii, mut rx) = client();
        let function =
            iii.register_function("s::f", RegisterFunction::new(|v: Value| Ok::<_, Error>(v)));
        sent(&mut rx); // the registration
        let since = std::time::Instant::now();
        iii.send_stats(since);
        assert!(sent(&mut rx).is_empty(), "nothing to say: nothing sent");

        let entry = iii.function_entry(&function.id).unwrap();
        entry.stats.record(true, Duration::from_micros(1500));
        entry.stats.record(false, Duration::from_micros(500));
        iii.reporter()
            .count_drop("s::f", observe_codes::BUFFER_FULL);
        iii.send_stats(since);
        let frames = sent(&mut rx);
        assert_eq!(frames.len(), 1);
        let stats = &frames[0];
        assert_eq!(stats["type"], "localinvocationstats");
        assert_eq!(
            stats["functions"],
            json!([{ "function_id": "s::f", "count": 2, "errors": 1, "duration_ms_sum": 2.0 }])
        );
        assert_eq!(
            stats["dropped_reports"],
            json!([{ "function_id": "s::f", "reason": "BUFFER_FULL", "count": 1 }])
        );
        assert!(stats["window_ms"].is_u64());
        iii.send_stats(since);
        assert!(sent(&mut rx).is_empty(), "counters were reset");
    }

    #[tokio::test]
    async fn is_observed_follows_the_compiled_set() {
        let (iii, mut rx) = client();
        let function =
            iii.register_function("i::f", RegisterFunction::new(|v: Value| Ok::<_, Error>(v)));
        assert!(!function.is_observed());
        assert!(function.observers().is_empty());
        iii.set_connection_state(IIIConnectionState::Connected);
        feed(
            &iii,
            json!({ "type": "functionregistered", "id": "i::f", "revision": 1, "local_dispatch": true,
                    "observers": [{ "id": "o", "target": "t", "filters": {} }] }),
        );
        next_ack(&mut rx).await;
        assert!(function.is_observed());
        assert_eq!(function.observers()[0].id(), "o");
        feed(
            &iii,
            json!({ "type": "observerremoved", "function_id": "i::f", "revision": 2, "id": "o" }),
        );
        while function.is_observed() {
            tokio::task::yield_now().await;
        }
        assert!(function.observers().is_empty());
    }
}

//! The host's side of observe: the observers of the functions this worker
//! hosts, and the task that runs their hooks.
//!
//! Each registered function has an entry: its compiled set of active
//! observers (read with one atomic load on every call), its match and
//! deliver hooks, and a hook task that applies the engine's announcements
//! (`functionregistered`, `observeradded`, `observerremoved`) one at a time,
//! running the [`AddedHook`](crate::AddedHook)s and [`RemovedHook`]s set in
//! [`hooks`](crate::hooks).

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
    hooks::{AddedSlot, Call, DeliverHook, Level, Match, Matcher, RemovedHook},
    iii::{IIIClient, IIIInner},
    observer_set::ObserverSet,
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
    /// A rejection with a code of the hook's own (`HTTP_ROUTE_CONFLICT`,
    /// `INVALID_CRON_EXPRESSION`, ...) and a message.
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

/// Every [`Error`] is `OBSERVE_REJECTED`, a failed call's
/// ([`Error::Remote`]) included: its code and message are in the rejection's
/// message. Return an [`ObserveRejection`] to reject with another code.
impl From<Error> for ObserveRejection {
    fn from(error: Error) -> Self {
        Self::new(observe_codes::OBSERVE_REJECTED, error.to_string())
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
///
/// The engine may announce an observer again with the same args and other
/// filters, target, capture or `on`: it stays the same observer and no hook
/// runs. Handles given out earlier keep the view they were made with, and
/// [`IIIClient::observers`] returns the announced one.
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

/// What the engine announced for an observer. A replay that keeps its args
/// and changes anything else gets a new one, with the same [`ObserverState`].
struct ObserverInner {
    view: ObserverView,
    filter: Filter,
    mask: CaptureMask,
    function_id: String,
    namespace: String,
    state: Arc<ObserverState>,
}

/// Where an observer stands, whatever view of it a handle holds.
struct ObserverState {
    /// Its place among the function's observers, in the order they were
    /// added: a call matches by keys only the observers it started with.
    seq: u64,
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
        seq: u64,
        client: Weak<IIIInner>,
    ) -> Self {
        let (acceptance, _) = watch::channel(Acceptance::Pending);
        let state = Arc::new(ObserverState {
            seq,
            revision: AtomicU64::new(revision),
            active: AtomicBool::new(false),
            rejection: Mutex::new(None),
            acceptance,
            client,
        });
        Self::with_state(view, filter, function_id.to_string(), namespace, state)
    }

    fn with_state(
        view: ObserverView,
        filter: Filter,
        function_id: String,
        namespace: String,
        state: Arc<ObserverState>,
    ) -> Self {
        let mask = capture_mask(&view.capture);
        Self {
            inner: Arc::new(ObserverInner {
                view,
                filter,
                mask,
                function_id,
                namespace,
                state,
            }),
        }
    }

    /// This observer with the view a replay announced, sharing its state.
    fn with_view(&self, view: ObserverView, filter: Filter, namespace: &str) -> Self {
        Self::with_state(
            view,
            filter,
            self.inner.function_id.clone(),
            namespace.to_string(),
            self.inner.state.clone(),
        )
    }

    fn state(&self) -> &ObserverState {
        &self.inner.state
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
    /// iii's delivery and [`matches`](Self::matches) apply them.
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
        self.state().active.load(Ordering::Acquire)
    }

    /// Whether `call` reaches this observer: its `on` accepts the call's
    /// status and its filters match the call. A deliver hook that calls
    /// targets itself applies them with this.
    pub fn matches(&self, call: &Call) -> bool {
        let success = call.status == InvocationStatus::Success;
        if !self.accepts(success) {
            return false;
        }
        let filter = &self.inner.filter;
        if filter.is_match_all() {
            return true;
        }
        let parts = DocParts::of(filter.roots(), call.error.as_ref(), call.caller.as_ref());
        filter.matches(&parts.doc(
            Some(&call.input),
            call.output.as_ref().filter(|_| success),
            call.metadata.as_ref(),
        ))
    }

    /// Resolves once the SDK has acked the observer, which makes it active:
    /// a call made after this resolves reaches an active observer, since the
    /// engine handles a connection's frames in order. A provider sends an
    /// observer its initial state this way: a call of its own function with
    /// metadata `{ "for_observer": <the observer's id> }`, which the
    /// function's match hook gives to this observer alone, delivered like
    /// every call. Fails with the rejection's code when the observer is
    /// rejected first.
    ///
    /// ```rust,no_run
    /// # use iii_sdk::{CallAction, CallRequest, IIIClient, Match, Observer};
    /// # use serde_json::json;
    /// # fn demo(iii: &IIIClient) {
    /// let client = iii.clone();
    /// iii.on_observer_added_for("stock::on-changed", move |obs: Observer| {
    ///     let iii = client.clone();
    ///     async move {
    ///         tokio::spawn(async move {
    ///             if obs.accepted().await.is_ok() {
    ///                 let initial = CallRequest::new("stock::on-changed", json!({ "available": 12 }))
    ///                     .metadata(json!({ "for_observer": obs.id() }));
    ///                 let _ = iii.call(initial.action(CallAction::Void)).await;
    ///             }
    ///         });
    ///         Ok(())
    ///     }
    /// });
    /// // a call for one observer goes to it alone; every other call to every observer
    /// iii.on_observer_match_for(
    ///     "stock::on-changed",
    ///     Match::predicate(|obs, call| {
    ///         let for_observer = call.metadata.as_ref().and_then(|m| m["for_observer"].as_str());
    ///         for_observer.is_none_or(|id| id == obs.id())
    ///     }),
    /// );
    /// # }
    /// ```
    pub async fn accepted(&self) -> Result<(), Error> {
        let mut acceptance = self.state().acceptance.subscribe();
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
    /// what it needs). It stops receiving calls now, and its removed hooks
    /// run next on the function's hook task, connected or not; the engine
    /// moves it to `error` with this code, and its `observerremoved` that
    /// follows runs none again. The rejection holds across a reconnect: the
    /// SDK sends it again whenever the engine lists the observer, until a
    /// fresh `observeradded` brings it back. Called while the observer's
    /// added hooks run, it rejects the observer once they return, and the
    /// removed hook of each level whose added hook resolved runs.
    pub fn reject(&self, code: impl Into<String>, message: impl Into<String>) {
        let error = ErrorBody::new(code, message);
        let state = self.state();
        // Held to the end: ordered against an added hook finishing.
        let mut rejection = state.rejection.lock().unwrap_or_else(|e| e.into_inner());
        *rejection = Some(error.clone());
        if !state.active.swap(false, Ordering::AcqRel) {
            // Not active (yet): its pending ack, or the next replay, carries it.
            return;
        }
        let Some(iii) = state.client.upgrade().map(IIIClient::from_inner) else {
            return;
        };
        if let Some(entry) = iii.function_entry(&self.inner.function_id) {
            // Out of the set now; the hook task rebuilds it in order too, and
            // runs the removed hooks: the engine sends no `observerremoved`
            // for a rejection that answers an announcement it has not seen
            // acked (a replay after a reconnect).
            entry.drop_inactive(self);
            entry.enqueue(
                &iii,
                HookOp::Rejected {
                    id: self.id().to_string(),
                },
            );
        }
        let sent = iii.send_message(Message::ObserverAck {
            function_id: self.inner.function_id.clone(),
            namespace: Some(self.inner.namespace.clone()),
            id: self.id().to_string(),
            revision: state.revision.load(Ordering::Acquire),
            error: Some(error),
        });
        if let Err(error) = sent {
            // Sent again with the next `functionregistered` that lists it.
            tracing::warn!(observer_id = %self.id(), error = %error, "late rejection not sent");
        }
    }

    fn settle(&self, acceptance: Acceptance) {
        self.state().acceptance.send_replace(acceptance);
    }

    pub(crate) fn seq(&self) -> u64 {
        self.state().seq
    }

    /// Whether `other` is this observer, in any view of it.
    fn is(&self, other: &Observer) -> bool {
        Arc::ptr_eq(&self.inner.state, &other.inner.state)
    }

    /// An observer of `test::f` with id `id`, not held by any client.
    #[cfg(test)]
    pub(crate) fn test(id: &str, seq: u64) -> Self {
        let view: ObserverView =
            serde_json::from_value(serde_json::json!({ "id": id, "target": "t", "filters": {} }))
                .expect("a view");
        Self::new(
            view,
            Filter::match_all(),
            "test::f",
            "default".to_string(),
            0,
            seq,
            Weak::new(),
        )
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
/// hot path reads it with one load. A change makes a new version that shares
/// all but `O(log n)` nodes of its observers with the one before, which a
/// call that loaded it keeps.
#[derive(Clone, Default)]
pub(crate) struct CompiledSet {
    /// By id.
    pub observers: ObserverSet,
    /// The roots any observer's filter reads.
    pub roots: Roots,
    /// What any observer captures.
    pub capture: CaptureMask,
    /// The newest [`Observer::seq`] that joined the set: an active observer
    /// added after it is not in the set. Seqs are never reused, and an
    /// observer that left is not active, so it never needs to go back down.
    pub seq: u64,
    /// How many of `observers` read each root and capture each part:
    /// `roots` and `capture` follow from them as observers come and go.
    counts: PartCounts,
}

/// Counts per part, in `input`, `output`, `error`, `caller`, `metadata`
/// order.
#[derive(Debug, Clone, Copy, Default)]
struct PartCounts {
    roots: [usize; 5],
    capture: [usize; 5],
}

impl PartCounts {
    fn count(&mut self, obs: &Observer, joined: bool) {
        let roots = obs.filter().roots();
        let mask = obs.mask();
        for (counts, parts) in [
            (
                &mut self.roots,
                [
                    roots.input,
                    roots.output,
                    roots.error,
                    roots.caller,
                    roots.metadata,
                ],
            ),
            (
                &mut self.capture,
                [
                    mask.input,
                    mask.output,
                    mask.error,
                    mask.caller,
                    mask.metadata,
                ],
            ),
        ] {
            for (count, part) in counts.iter_mut().zip(parts) {
                if part {
                    if joined {
                        *count += 1;
                    } else {
                        *count -= 1;
                    }
                }
            }
        }
    }
}

impl CompiledSet {
    fn of(observers: ObserverSet, counts: PartCounts, seq: u64) -> Self {
        let [input, output, error, caller, metadata] = counts.roots.map(|count| count > 0);
        let roots = Roots {
            input,
            output,
            error,
            caller,
            metadata,
        };
        let [input, output, error, caller, metadata] = counts.capture.map(|count| count > 0);
        let capture = CaptureMask {
            input,
            output,
            error,
            caller,
            metadata,
        };
        Self {
            observers,
            roots,
            capture,
            seq,
            counts,
        }
    }

    /// This set with `obs`, in place of any other view of it.
    fn with(&self, obs: &Observer) -> Self {
        let mut counts = self.counts;
        if let Some(held) = self.observers.get(obs.id()) {
            counts.count(held, false);
        }
        counts.count(obs, true);
        Self::of(self.observers.with(obs), counts, self.seq.max(obs.seq()))
    }

    /// This set without `obs` (in any view of it); `None` when it does not
    /// hold it.
    fn without(&self, obs: &Observer) -> Option<Self> {
        let held = self.observers.get(obs.id()).filter(|held| held.is(obs))?;
        let mut counts = self.counts;
        counts.count(held, false);
        let observers = self.observers.without(obs.id())?;
        Some(Self::of(observers, counts, self.seq))
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
    /// Bumped by each [`release`](Self::release): a `functionregistered`
    /// queued before it must not turn local dispatch back on.
    revocation: AtomicU64,
    /// The engine does not host the function for this worker since a
    /// release (a conflict, a refused or withdrawn registration), until it
    /// confirms it again.
    revoked: AtomicBool,
    /// A conflict was logged since the engine last confirmed the function:
    /// a standby's later ones are logged at debug.
    conflicted: AtomicBool,
    compiled: ArcSwap<CompiledSet>,
    /// The number of active observers, shifted left by one, with the low bit
    /// set while a deliver hook is: what a call reads first, in one atomic
    /// load.
    delivery: AtomicUsize,
    /// The function's deliver hook, mirrored from the hook store.
    deliver: ArcSwapOption<Arc<dyn DeliverHook>>,
    /// The function's match hook, with its key index.
    matcher: ArcSwapOption<Matcher>,
    /// Held while an observer is activated or deactivated with its filing in
    /// the key index, and while a match hook builds its index from the
    /// active observers: the index holds exactly the active observers.
    indexing: Mutex<()>,
    /// The observers created so far: the next one's [`Observer::seq`].
    created: AtomicU64,
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
        matcher: Option<Matcher>,
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
            conflicted: AtomicBool::new(false),
            compiled: ArcSwap::from_pointee(CompiledSet::default()),
            delivery: AtomicUsize::new(delivery),
            deliver: ArcSwapOption::new(deliver.map(Arc::new)),
            matcher: ArcSwapOption::new(matcher.map(Arc::new)),
            indexing: Mutex::new(()),
            created: AtomicU64::new(0),
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

    /// Whether `call` would reach anyone: matched by keys, an observer is
    /// filed under one of its keys; otherwise any is active.
    pub(crate) fn is_observed_for(&self, call: &Call) -> bool {
        if !self.is_observed() {
            return false;
        }
        let matcher = self.matcher.load();
        matcher
            .as_deref()
            .and_then(|matcher| matcher.wants_any(&self.id, call))
            .unwrap_or(true)
    }

    pub(crate) fn matcher(&self) -> Option<Arc<Matcher>> {
        self.matcher.load_full()
    }

    /// Replaces the match hook with the one `rule` reads under the index
    /// lock, its index built from the active observers before it is
    /// published, so no call matches by an empty index. Returns the observers
    /// whose key function panicked, filed nowhere, with the panic: the caller
    /// rejects them.
    pub(crate) fn set_match(
        &self,
        rule: impl FnOnce() -> Option<Match>,
    ) -> Vec<(Observer, String)> {
        let _indexing = self.lock_index();
        let rule = rule();
        let unchanged = match (self.matcher.load().as_deref(), &rule) {
            (Some(current), Some(rule)) => current.rule().same(rule),
            (None, None) => true,
            _ => false,
        };
        if unchanged {
            return Vec::new();
        }
        let matcher = rule.map(Matcher::new);
        let panicked = matcher
            .as_ref()
            .map(|matcher| matcher.file_all(&self.observers()))
            .unwrap_or_default();
        self.matcher.store(matcher.map(Arc::new));
        panicked
    }

    fn lock_index(&self) -> std::sync::MutexGuard<'_, ()> {
        self.indexing.lock().unwrap_or_else(|e| e.into_inner())
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

    /// Needs an engine that advertised `observe.v2`: it declares
    /// `args_format`, `private` or `observable: false`, or has a deliver or
    /// match hook.
    pub(crate) fn needs_v2(&self) -> bool {
        self.declares_v2 || self.has_deliver() || self.matcher.load().is_some()
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

    /// Puts `obs` in the compiled set when it is active, in place of any
    /// other view of it, and takes it out otherwise: `O(log n)`.
    fn compile(&self, obs: &Observer) {
        let _serialized = self.publishing.lock().unwrap_or_else(|e| e.into_inner());
        let current = self.compiled.load();
        let next = if obs.is_active() {
            Some(current.with(obs))
        } else {
            current.without(obs)
        };
        if let Some(next) = next {
            self.compiled.store(Arc::new(next));
            self.refresh_delivery();
        }
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

    /// Turns local dispatch off and forgets the confirmation: the function
    /// was released or unregistered.
    pub(crate) fn revoke(&self) {
        self.local_dispatch.store(false, Ordering::Release);
        self.confirmed_epoch.store(0, Ordering::Release);
    }

    /// The function is not this worker's any more: another worker hosts it
    /// (`FUNCTION_NAMESPACE_CONFLICT`), the engine refused or withdrew its
    /// registration, or it was unregistered. Ordered against
    /// `functionregistered` messages already queued, and against added hooks
    /// still running.
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

    /// Records a `FUNCTION_NAMESPACE_CONFLICT`: `true` for the first since
    /// the engine last confirmed the function.
    pub(crate) fn first_conflict(&self) -> bool {
        !self.conflicted.swap(true, Ordering::AcqRel)
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

    /// Takes `obs`, deactivated, out of the compiled set.
    fn drop_inactive(&self, obs: &Observer) {
        self.compile(obs);
    }

    /// The compiled set follows `members` on the change of the member with
    /// id `id`: it holds that member's observer when it is active, and no
    /// observer with that id otherwise. `gone` is the observer of a member
    /// just removed.
    fn publish(&self, members: &BTreeMap<String, Member>, id: &str, gone: Option<&Observer>) {
        match (members.get(id), gone) {
            (Some(member), _) => self.compile(&member.observer),
            (None, Some(obs)) => self.compile(obs),
            (None, None) => {}
        }
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
            HookOp::Reset { .. } | HookOp::Rejected { .. } | HookOp::CatchUp(_)
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
    /// An observer was rejected late: rebuild the compiled set and run its
    /// removed hooks. It stays held with its rejection, which a replay that
    /// lists it gets again.
    Rejected {
        id: String,
    },
    /// Drop the whole set: the function was released (a conflict, a refused
    /// or withdrawn registration), or unregistered (`last`).
    Reset {
        last: bool,
    },
    /// An added hook of this level was set: it runs for each active observer
    /// that did not run it.
    CatchUp(Level),
}

/// An accepted observer, and the added hooks that ran for it, by the
/// generation of the hook at each level.
struct Member {
    observer: Observer,
    worker: Option<u64>,
    function: Option<u64>,
    /// Its removed hooks ran (a late rejection): its removal runs none.
    torn_down: bool,
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
                entry.conflicted.store(false, Ordering::Release);
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
                    if let Some(view) = ack_held(
                        &iii,
                        &entry,
                        &mut members,
                        view,
                        &namespace,
                        rev,
                        Replay::Listed,
                    ) {
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
                    // changed args: removed, then added as a new observer
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
                if entry.is_revoked() {
                    // Announced before the release: the function is not
                    // this worker's, and nothing undoes an add now.
                    tracing::debug!(function_id = %entry.id, observer_id = %observer.id, "observeradded for a released function ignored");
                    continue;
                }
                if let Some(observer) = ack_held(
                    &iii,
                    &entry,
                    &mut members,
                    observer,
                    &namespace,
                    rev,
                    Replay::Added,
                ) {
                    // New, with changed args, or rejected earlier: a fresh
                    // observer.
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
            HookOp::Rejected { id } => {
                entry.publish(&members, &id, None);
                tear_down(&iii, &entry, &mut members, &id).await;
            }
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
            HookOp::CatchUp(level) => catch_up(&iii, &entry, &mut members, level).await,
        }
    }
}

/// Why the engine announced an observer the SDK may already hold.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Replay {
    /// Listed by `functionregistered`: a late rejection is sent again.
    Listed,
    /// `observeradded`: an observer rejected late comes back as a new one.
    Added,
}

/// Acks again, at `revision` and without running any hook, an observer this
/// SDK holds that the engine announced again as `view`: one with the same
/// args, or (when `replay` is [`Replay::Listed`]) one rejected late, whose
/// rejection is sent again. A held observer whose filters, target, capture or
/// `on` changed is served with the new view from the ack on. Returns `view`
/// for an observer to add as a new one (after removing the one held).
fn ack_held(
    iii: &IIIClient,
    entry: &FunctionEntry,
    members: &mut BTreeMap<String, Member>,
    view: ObserverView,
    namespace: &str,
    revision: u64,
    replay: Replay,
) -> Option<ObserverView> {
    let Some(held) = members.get(&view.id).map(|member| member.observer.clone()) else {
        return Some(view);
    };
    let _indexing = entry.lock_index();
    // A changed view with the same args, prepared before the rejection lock
    // is taken (key functions are the worker's code). `Some(None)`: a view
    // the SDK cannot serve (a filter that does not compile, a key function
    // that panics), which makes it a new observer.
    let changed = (held.view().args == view.args && held.view() != &view)
        .then(|| updated(entry, &held, &view, namespace));
    // Under the rejection lock, ordered with `reject()`.
    let rejection = held
        .state()
        .rejection
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let kept = match (rejection.is_some(), replay) {
        (true, Replay::Listed) => true,
        (true, Replay::Added) => false,
        (false, _) => held.view().args == view.args,
    };
    if !kept {
        return Some(view);
    }
    match (rejection.is_some(), changed) {
        (false, Some(None)) => return Some(view),
        (false, Some(Some((observer, keys)))) => {
            if let (Some(matcher), Some(keys)) = (entry.matcher(), keys) {
                matcher.file(&observer, keys);
            }
            if let Some(member) = members.get_mut(&view.id) {
                member.observer = observer;
            }
            entry.publish(members, &view.id, None);
        }
        _ => {}
    }
    held.state().revision.store(revision, Ordering::Release);
    send_ack(iii, entry, namespace, &view.id, revision, rejection.clone());
    None
}

/// `held` with the changed view `view`, and its keys when the function
/// matches by keys; `None` when the SDK cannot serve the view.
fn updated(
    entry: &FunctionEntry,
    held: &Observer,
    view: &ObserverView,
    namespace: &str,
) -> Option<(Observer, Option<Vec<String>>)> {
    let filter = Filter::from_normalized(&view.filters).ok()?;
    let observer = held.with_view(view.clone(), filter, namespace);
    let keys = match entry
        .matcher()
        .and_then(|matcher| matcher.observer_keys(&observer))
    {
        Some(Err(_)) => return None,
        Some(Ok(keys)) => Some(keys),
        None => None,
    };
    Some((observer, keys))
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
        entry.created.fetch_add(1, Ordering::AcqRel) + 1,
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
    match admit(iii, entry, members, &observer, resolved, revocation) {
        Admission::Accepted => observer.settle(Acceptance::Accepted),
        Admission::Refused { error, ack } => {
            unwind(iii, entry, &observer, resolved).await;
            observer.settle(Acceptance::Refused(error.clone()));
            if ack {
                send_ack(iii, entry, namespace, &id, revision, Some(error));
            }
        }
    }
}

/// What became of an observer whose added hooks resolved.
enum Admission {
    /// Active, filed in the key index, acked and compiled in.
    Accepted,
    /// Not accepted: the levels that resolved are undone, then it fails with
    /// `error`, which its ack carries when `ack` (a released function's
    /// observers get none).
    Refused { error: ErrorBody, ack: bool },
}

/// Accepts an observer whose added hooks resolved, under the function's
/// index lock: an observer whose key function panics is rejected, and one
/// rejected (`reject()`) or released while the hooks ran is not accepted.
fn admit(
    iii: &IIIClient,
    entry: &FunctionEntry,
    members: &mut BTreeMap<String, Member>,
    observer: &Observer,
    resolved: Resolved,
    revocation: u64,
) -> Admission {
    let _indexing = entry.lock_index();
    // The keys it is filed under, computed once.
    let matcher = entry.matcher();
    let keys = match matcher
        .as_ref()
        .and_then(|matcher| matcher.observer_keys(observer))
    {
        Some(Err(crate::hooks::KeysPanicked(panic))) => {
            return Admission::Refused {
                error: ErrorBody::new(
                    observe_codes::OBSERVE_REJECTED,
                    format!("on_observer_match key function panicked: {panic}"),
                ),
                ack: true,
            };
        }
        Some(Ok(keys)) => Some(keys),
        None => None,
    };
    if entry.revocation() != revocation {
        // The function was released while the hooks ran: undo the add (no
        // ack, never active), as if it had been removed.
        tracing::debug!(function_id = %entry.id, observer_id = %observer.id(), "function released during the added hooks");
        return Admission::Refused {
            error: ErrorBody::new(
                observe_codes::OBSERVER_NOT_ACTIVE,
                format!("{} was released while its added hooks ran", entry.id),
            ),
            ack: false,
        };
    }
    // A `reject()` made while the hooks ran wins. Holding the rejection lock
    // through the ack keeps a concurrent `reject()` strictly after it (a late
    // rejection the engine handles), never between activation and ack.
    let rejection = observer
        .state()
        .rejection
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(error) = rejection.clone() {
        tracing::debug!(function_id = %entry.id, observer_id = %observer.id(), code = %error.code, "observer rejected during its added hooks");
        return Admission::Refused { error, ack: true };
    }
    // Filed, acked, then published, in one synchronous step: the ack is
    // queued ahead of any report the new observer matches, and the
    // connection loop (same thread) only sends it after this task yields, by
    // when the observer is compiled in.
    observer.state().active.store(true, Ordering::Release);
    if let (Some(matcher), Some(keys)) = (&matcher, keys) {
        matcher.file(observer, keys);
    }
    send_ack(
        iii,
        entry,
        observer.namespace(),
        observer.id(),
        observer.state().revision.load(Ordering::Acquire),
        None,
    );
    members.insert(
        observer.id().to_string(),
        Member {
            observer: observer.clone(),
            worker: resolved.worker,
            function: resolved.function,
            torn_down: false,
        },
    );
    entry.publish(members, observer.id(), None);
    Admission::Accepted
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
    {
        let _indexing = entry.lock_index();
        member
            .observer
            .state()
            .active
            .store(false, Ordering::Release);
        if let Some(matcher) = entry.matcher() {
            matcher.unfile(id);
        }
        entry.publish(members, id, Some(&member.observer));
    }
    if member.torn_down {
        return;
    }
    for hook in iii.hooks().removed(&entry.id).into_iter().flatten() {
        run_removed(iii, entry, &hook, &member.observer).await;
    }
}

/// Runs the removed hooks of a held observer rejected late, the function's
/// then the worker's, once: it stays held (inactive, out of the key index)
/// until the engine removes it or announces it afresh, and that removal runs
/// none again.
async fn tear_down(
    iii: &IIIClient,
    entry: &Arc<FunctionEntry>,
    members: &mut BTreeMap<String, Member>,
    id: &str,
) {
    let observer = {
        let Some(member) = members.get_mut(id) else {
            return;
        };
        if member.torn_down || member.observer.is_active() {
            return;
        }
        member.torn_down = true;
        member.observer.clone()
    };
    {
        let _indexing = entry.lock_index();
        if let Some(matcher) = entry.matcher() {
            matcher.unfile(id);
        }
    }
    for hook in iii.hooks().removed(&entry.id).into_iter().flatten() {
        run_removed(iii, entry, &hook, &observer).await;
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

    /// An `ObserveRejection` keeps its code; every `Error` an added hook
    /// returns with `?`, a failed call's included, is `OBSERVE_REJECTED`.
    #[test]
    fn errors_reject_with_observe_rejected() {
        let failed_call = Error::Remote {
            code: "FORBIDDEN".into(),
            message: "nope".into(),
            stacktrace: None,
        };
        let rejection = ObserveRejection::from(failed_call);
        assert_eq!(rejection.code, observe_codes::OBSERVE_REJECTED);
        assert!(
            rejection.message.contains("FORBIDDEN") && rejection.message.contains("nope"),
            "{}",
            rejection.message
        );
        assert_eq!(
            ObserveRejection::from(Error::Timeout).code,
            observe_codes::OBSERVE_REJECTED
        );
        assert_eq!(
            ObserveRejection::new("INVALID_CRON_EXPRESSION", "x").code,
            "INVALID_CRON_EXPRESSION"
        );
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
        iii.on_observer_deliver_for("o::hooked", answer_with_count);
        iii.register_function("o::hooked", echo());
        iii.on_observer_match_for("o::matched", kind_keys());
        iii.register_function("o::matched", echo());
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
                    // every reason is named, the match and deliver hooks among them
                    .filter(|l| l.contains("on_observer_match or on_observer_deliver"))
                    .count()
            };
            match (
                named("o::private"),
                named("o::hooked"),
                named("o::matched"),
                named("o::late"),
                named("o::plain"),
            ) {
                (1, 1, 1, 1, 0) => Ok(()),
                other => Err(format!("expected one line each, got {other:?}")),
            }
        };
        logs_assert(warned);

        // a plain function that gets a deliver hook is withdrawn
        iii.on_observer_deliver_for("o::plain", answer_with_count);
        let frames = sent(&mut rx);
        assert_eq!(frames.len(), 1);
        assert_eq!(
            frames[0],
            json!({ "type": "unregisterfunction", "id": "o::plain" })
        );
        // and registered again once its deliver hook is removed
        iii.clear_observer_deliver_for("o::plain");
        assert_eq!(registered(&sent(&mut rx)), vec!["o::plain"]);
        // a match hook withdraws it too, until it is removed
        iii.on_observer_match_for("o::plain", kind_keys());
        assert_eq!(
            sent(&mut rx),
            vec![json!({ "type": "unregisterfunction", "id": "o::plain" })]
        );
        iii.clear_observer_match_for("o::plain");
        assert_eq!(registered(&sent(&mut rx)), vec!["o::plain"]);
        iii.on_observer_deliver_for("o::plain", answer_with_count);
        assert_eq!(
            sent(&mut rx),
            vec![json!({ "type": "unregisterfunction", "id": "o::plain" })]
        );

        // an engine with observe.v2 registers them all
        feed(
            &iii,
            json!({ "type": "workerregistered", "worker_id": "w",
                    "features": ["observe.v1", "observe.v2", "local-dispatch.v1"] }),
        );
        let mut ids = registered(&sent(&mut rx));
        ids.sort();
        assert_eq!(
            ids,
            vec![
                "o::hooked",
                "o::late",
                "o::matched",
                "o::plain",
                "o::private"
            ]
        );
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

    /// A deliver hook that answers with the number of observers it got.
    async fn answer_with_count(_call: Call, observers: Vec<Observer>) -> Result<Value, Error> {
        Ok(json!(observers.len()))
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

    /// Registers `function_id` on a v2 connection with `observers` and waits
    /// until they are active.
    async fn hosted(
        iii: &IIIClient,
        rx: &mut UnboundedReceiver<Outbound>,
        function_id: &str,
        observers: Value,
    ) -> crate::runtime::FunctionRef {
        let function = iii.register_function(function_id, echo());
        let count = observers.as_array().map_or(0, Vec::len);
        feed(
            iii,
            json!({ "type": "functionregistered", "id": function_id, "revision": 1,
                    "local_dispatch": true, "observers": observers }),
        );
        for _ in 0..count {
            next_ack(rx).await;
        }
        while function.observers().len() < count {
            tokio::task::yield_now().await;
        }
        function
    }

    fn view_with_args(id: &str, args: Value) -> Value {
        json!({ "id": id, "args": args, "target": "t", "filters": {} })
    }

    /// A function with a deliver hook delivers its calls itself: a routed
    /// call's answer is the hook's, with no `observed` and no report, and the
    /// hook gets the observers its match hook says want the call.
    #[tokio::test]
    async fn a_deliver_hook_answers_and_iii_delivers_nothing() {
        let (iii, mut rx) = client();
        v2_hello(&iii);
        sent(&mut rx);
        iii.on_observer_match_for(
            "self::f",
            crate::Match::predicate(|obs, call| obs.args_value()["kind"] == call.input["kind"]),
        );
        iii.on_observer_deliver_for("self::f", answer_with_count);
        hosted(
            &iii,
            &mut rx,
            "self::f",
            json!([
                view_with_args("a1", json!({ "kind": "a" })),
                view_with_args("a2", json!({ "kind": "a" })),
                view_with_args("b", json!({ "kind": "b" }))
            ]),
        )
        .await;
        feed(
            &iii,
            json!({ "type": "invokefunction", "invocation_id": "11111111-2222-4333-8444-555555555555",
                    "function_id": "self::f", "data": { "kind": "a" } }),
        );
        let answer = next_frame(&mut rx, "invocationresult").await;
        assert_eq!(answer["result"], 2, "the two observers of kind a");
        assert!(answer.get("observed").is_none());
        iii.flush_reports();
        assert!(
            sent(&mut rx)
                .iter()
                .all(|m| m["type"] != "invocationsobserved"),
            "iii delivers nothing for it"
        );
    }

    /// A predicate that panics matches nothing, logged once per function.
    #[tokio::test]
    async fn a_panicking_predicate_matches_nothing_and_is_logged_once() {
        let warnings = Warnings::default();
        let _capture = tracing::subscriber::set_default(warnings.clone());
        let (iii, mut rx) = client();
        v2_hello(&iii);
        iii.on_observer_match_for(
            "pp::f",
            crate::Match::predicate(|obs, _call| {
                if obs.id() == "boom" {
                    panic!("predicate bug");
                }
                true
            }),
        );
        hosted(
            &iii,
            &mut rx,
            "pp::f",
            json!([
                view_with_args("boom", json!({})),
                view_with_args("fine", json!({}))
            ]),
        )
        .await;
        sent(&mut rx);
        for invocation in [
            "21111111-2222-4333-8444-555555555555",
            "31111111-2222-4333-8444-555555555555",
        ] {
            feed(
                &iii,
                json!({ "type": "invokefunction", "invocation_id": invocation,
                        "function_id": "pp::f", "data": {} }),
            );
            let answer = next_frame(&mut rx, "invocationresult").await;
            assert_eq!(answer["observed"], json!(["fine"]));
        }
        let lines = warnings.0.lock().unwrap().clone();
        let logged = lines
            .iter()
            .filter(|l| l.contains("predicate panicked"))
            .count();
        assert_eq!(logged, 1, "{lines:#?}");
    }

    /// An observer whose key function panics is rejected with
    /// `OBSERVE_REJECTED`, when it is added and when a match hook set late
    /// rebuilds the index.
    #[tokio::test]
    async fn a_panicking_key_function_rejects_the_observer() {
        let (iii, mut rx) = client();
        v2_hello(&iii);
        let keys = || {
            crate::Match::keys(
                |obs| {
                    if obs.id() == "boom" {
                        panic!("key bug");
                    }
                    obs.args_value()["kind"].as_str().into()
                },
                |call| call.input["kind"].as_str().into(),
            )
        };
        iii.on_observer_match_for("kp::f", keys());
        let function = iii.register_function("kp::f", echo());
        feed(
            &iii,
            json!({ "type": "functionregistered", "id": "kp::f", "revision": 1,
                    "local_dispatch": true,
                    "observers": [view_with_args("boom", json!({ "kind": "a" })),
                                  view_with_args("fine", json!({ "kind": "a" }))] }),
        );
        let first = next_ack(&mut rx).await;
        assert_eq!(first["id"], "boom");
        assert_eq!(first["error"]["code"], "OBSERVE_REJECTED");
        let second = next_ack(&mut rx).await;
        assert!(second.get("error").is_none());
        let call = Call {
            input: json!({ "kind": "a" }),
            ..Call::default()
        };
        while !function.is_observed_for(&call) {
            tokio::task::yield_now().await;
        }

        // a match hook set late files the active observers; one whose keys
        // panic is rejected late
        let late = hosted(
            &iii,
            &mut rx,
            "kl::f",
            json!([
                view_with_args("boom", json!({ "kind": "a" })),
                view_with_args("fine", json!({ "kind": "b" }))
            ]),
        )
        .await;
        late.on_observer_match(keys());
        let rejection = next_ack(&mut rx).await;
        assert_eq!(rejection["id"], "boom");
        assert_eq!(rejection["error"]["code"], "OBSERVE_REJECTED");
        let b = Call {
            input: json!({ "kind": "b" }),
            ..Call::default()
        };
        while !late.is_observed_for(&b) {
            tokio::task::yield_now().await;
        }
        assert!(!late.is_observed_for(&call), "boom is filed nowhere");
    }

    /// Keys from a string array at `value[field]`.
    fn keys_at<'a>(value: &'a Value, field: &str) -> crate::Keys<'a> {
        value[field]
            .as_array()
            .map(|keys| keys.iter().filter_map(Value::as_str).collect::<Vec<&str>>())
            .into()
    }

    /// The `observed` ids of the answer to a routed call of `function_id`
    /// with `input`.
    async fn routed_observed(
        iii: &IIIClient,
        rx: &mut UnboundedReceiver<Outbound>,
        function_id: &str,
        invocation_id: &str,
        input: Value,
    ) -> Value {
        feed(
            iii,
            json!({ "type": "invokefunction", "invocation_id": invocation_id,
                    "function_id": function_id, "data": input }),
        );
        let answer = next_frame(rx, "invocationresult").await;
        answer.get("observed").cloned().unwrap_or(json!([]))
    }

    /// A match hook set while observers are active matches them from the
    /// moment it is set, while the function's hook task is busy too.
    #[tokio::test]
    async fn a_match_hook_set_late_matches_at_once() {
        let (iii, mut rx) = client();
        v2_hello(&iii);
        let function = hosted(
            &iii,
            &mut rx,
            "ml::f",
            json!([
                view_with_args("a", json!({ "kind": "a" })),
                view_with_args("b", json!({ "kind": "b" }))
            ]),
        )
        .await;
        // the hook task runs a late added hook that never finishes
        function.on_observer_added(|_obs: Observer| async move {
            std::future::pending::<()>().await;
            Ok(())
        });
        function.on_observer_match(crate::Match::keys(
            |obs| obs.args_value()["kind"].as_str().into(),
            |call| call.input["kind"].as_str().into(),
        ));
        let call = |kind: &str| Call {
            input: json!({ "kind": kind }),
            ..Call::default()
        };
        assert!(function.is_observed_for(&call("a")));
        assert!(function.is_observed_for(&call("b")));
        assert!(!function.is_observed_for(&call("c")));
        sent(&mut rx);
        let observed = routed_observed(
            &iii,
            &mut rx,
            "ml::f",
            "51111111-2222-4333-8444-555555555555",
            json!({ "kind": "b" }),
        )
        .await;
        assert_eq!(observed, json!(["b"]));
    }

    /// Matched by keys, an observer filed under several keys of a call is
    /// matched once, and each observer under any of them is matched.
    #[tokio::test]
    async fn an_observer_under_several_keys_of_a_call_matches_once() {
        let (iii, mut rx) = client();
        v2_hello(&iii);
        iii.on_observer_match_for(
            "mk::f",
            crate::Match::keys(
                |obs| keys_at(obs.args_value(), "keys"),
                |call| keys_at(&call.input, "keys"),
            ),
        );
        hosted(
            &iii,
            &mut rx,
            "mk::f",
            json!([
                view_with_args("both", json!({ "keys": ["x", "*"] })),
                view_with_args("star", json!({ "keys": ["*"] })),
                view_with_args("y", json!({ "keys": ["y"] }))
            ]),
        )
        .await;
        sent(&mut rx);
        let sorted = |observed: Value| {
            let mut ids: Vec<String> = serde_json::from_value(observed).unwrap();
            ids.sort();
            ids
        };
        let observed = routed_observed(
            &iii,
            &mut rx,
            "mk::f",
            "61111111-2222-4333-8444-555555555555",
            json!({ "keys": ["x", "*", "x", "y"] }),
        )
        .await;
        assert_eq!(sorted(observed), vec!["both", "star", "y"]);
        let observed = routed_observed(
            &iii,
            &mut rx,
            "mk::f",
            "71111111-2222-4333-8444-555555555555",
            json!({ "keys": ["*"] }),
        )
        .await;
        assert_eq!(sorted(observed), vec!["both", "star"]);
    }

    fn kind_keys() -> crate::Match {
        crate::Match::keys(
            |obs| obs.args_value()["kind"].as_str().into(),
            |call| call.input["kind"].as_str().into(),
        )
    }

    fn kind(kind: &str) -> Call {
        Call {
            input: json!({ "kind": kind }),
            ..Call::default()
        }
    }

    /// A match hook set while an observer's removal runs builds its index
    /// from the active observers, each one's keys computed once: the removed
    /// observer is filed nowhere.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_match_hook_set_during_a_removal_files_only_active_observers() {
        let (iii, mut rx) = client();
        v2_hello(&iii);
        let removed = Arc::new(AtomicBool::new(false));
        iii.on_observer_removed_for("mr::f", {
            let removed = removed.clone();
            move |_obs: Observer| {
                let removed = removed.clone();
                async move { removed.store(true, Ordering::SeqCst) }
            }
        });
        let function = hosted(
            &iii,
            &mut rx,
            "mr::f",
            json!([
                view_with_args("a", json!({ "kind": "a" })),
                view_with_args("b", json!({ "kind": "b" }))
            ]),
        )
        .await;
        let computed = Arc::new(Mutex::new(Vec::<String>::new()));
        // a's key function announces a's removal and gives the hook task
        // 200 ms to run it before a is filed
        function.on_observer_match(crate::Match::keys(
            {
                let (iii, removed, computed) = (iii.clone(), removed.clone(), computed.clone());
                move |obs| {
                    computed.lock().unwrap().push(obs.id().to_string());
                    if obs.id() == "a" {
                        feed(
                            &iii,
                            json!({ "type": "observerremoved", "function_id": "mr::f",
                                    "revision": 2, "id": "a" }),
                        );
                        let deadline = std::time::Instant::now() + Duration::from_millis(200);
                        while !removed.load(Ordering::SeqCst)
                            && std::time::Instant::now() < deadline
                        {
                            std::thread::sleep(Duration::from_millis(5));
                        }
                    }
                    obs.args_value()["kind"].as_str().into()
                }
            },
            |call| call.input["kind"].as_str().into(),
        ));
        while !removed.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert!(function.is_observed_for(&kind("b")));
        assert!(!function.is_observed_for(&kind("a")), "a is filed nowhere");
        assert_eq!(*computed.lock().unwrap(), vec!["a", "b"], "keys once each");
    }

    /// Matched by keys, a call matches only the observers it started with:
    /// one accepted while the handler runs is neither delivered the call nor
    /// handed to the deliver hook.
    #[tokio::test]
    async fn an_observer_accepted_during_a_call_does_not_match_it() {
        let (iii, mut rx) = client();
        v2_hello(&iii);
        let entered = Arc::new(tokio::sync::Notify::new());
        let gate = Arc::new(tokio::sync::Notify::new());
        let gated = || {
            let (entered, gate) = (entered.clone(), gate.clone());
            RegisterFunction::new_async(move |input: Value| {
                let (entered, gate) = (entered.clone(), gate.clone());
                async move {
                    entered.notify_one();
                    gate.notified().await;
                    Ok::<_, Error>(input)
                }
            })
        };
        iii.on_observer_match_for("kd::f", kind_keys());
        iii.on_observer_match_for("kh::f", kind_keys());
        iii.on_observer_deliver_for(
            "kh::f",
            |_call: Call, observers: Vec<Observer>| async move {
                Ok(json!(
                    observers.iter().map(Observer::id).collect::<Vec<_>>()
                ))
            },
        );
        for (n, function_id) in ["kd::f", "kh::f"].into_iter().enumerate() {
            iii.register_function(function_id, gated());
            feed(
                &iii,
                json!({ "type": "functionregistered", "id": function_id, "revision": 1,
                        "local_dispatch": true,
                        "observers": [view_with_args("a", json!({ "kind": "x" }))] }),
            );
            next_ack(&mut rx).await;
            feed(
                &iii,
                json!({ "type": "invokefunction", "function_id": function_id,
                        "invocation_id": format!("8111111{n}-2222-4333-8444-555555555555"),
                        "data": { "kind": "x" } }),
            );
            entered.notified().await;
            feed(
                &iii,
                json!({ "type": "observeradded", "function_id": function_id, "revision": 2,
                        "observer": view_with_args("b", json!({ "kind": "x" })) }),
            );
            assert_eq!(next_ack(&mut rx).await["id"], "b");
            gate.notify_one();
            let answer = next_frame(&mut rx, "invocationresult").await;
            match function_id {
                "kd::f" => assert_eq!(answer["observed"], json!(["a"])),
                _ => assert_eq!(answer["result"], json!(["a"])),
            }
        }
    }

    /// An `observeradded` queued behind a slow added hook when the function's
    /// registration is withdrawn runs no hook and gets no ack.
    #[tokio::test]
    async fn an_observer_announced_before_a_withdrawal_is_not_added() {
        let (iii, mut rx) = client();
        feed(
            &iii,
            json!({ "type": "workerregistered", "worker_id": "w",
                    "features": ["observe.v1", "local-dispatch.v1"] }),
        );
        let gate = Arc::new(tokio::sync::Notify::new());
        let ran = Arc::new(Mutex::new(Vec::<String>::new()));
        iii.on_observer_added_for("wq::f", {
            let (gate, ran) = (gate.clone(), ran.clone());
            move |obs: Observer| {
                let (gate, ran) = (gate.clone(), ran.clone());
                async move {
                    ran.lock().unwrap().push(obs.id().to_string());
                    if obs.id() == "slow" {
                        gate.notified().await;
                    }
                    Ok(())
                }
            }
        });
        let function = iii.register_function("wq::f", echo());
        feed(
            &iii,
            json!({ "type": "functionregistered", "id": "wq::f", "revision": 1,
                    "local_dispatch": true, "observers": [view_with_args("slow", json!({}))] }),
        );
        while ran.lock().unwrap().is_empty() {
            tokio::task::yield_now().await;
        }
        feed(
            &iii,
            json!({ "type": "observeradded", "function_id": "wq::f", "revision": 2,
                    "observer": view_with_args("late", json!({})) }),
        );
        iii.on_observer_deliver_for("wq::f", answer_with_count);
        gate.notify_one();
        for _ in 0..100 {
            tokio::task::yield_now().await;
        }
        assert_eq!(*ran.lock().unwrap(), vec!["slow"], "late's hook never ran");
        let frames = sent(&mut rx);
        assert!(
            frames.iter().all(|m| m["type"] != "observerack"),
            "{frames:#?}"
        );
        assert!(
            frames
                .iter()
                .any(|m| m["type"] == "unregisterfunction" && m["id"] == "wq::f")
        );
        assert!(!function.is_observed());
    }

    /// No observer is acked before `register_function` returned: the hook
    /// task waits for the entry to open.
    #[tokio::test]
    async fn no_observer_is_acked_before_the_registration_returns() {
        let (iii, mut rx) = client();
        let entry = Arc::new(FunctionEntry::new(
            "b7::f".to_string(),
            false,
            true,
            true,
            false,
            None,
            None,
        ));
        let view: ObserverView =
            serde_json::from_value(json!({ "id": "o", "target": "t", "filters": {} })).unwrap();
        entry.enqueue(
            &iii,
            HookOp::Registered {
                revocation: entry.revocation(),
                epoch: 0,
                namespace: "default".to_string(),
                revision: 1,
                local_dispatch: true,
                observers: vec![view],
            },
        );
        for _ in 0..50 {
            tokio::task::yield_now().await;
        }
        assert!(sent(&mut rx).is_empty(), "nothing acked");
        assert!(!entry.is_observed());
        entry.open();
        assert_eq!(next_ack(&mut rx).await["id"], "o");
        while !entry.is_observed() {
            tokio::task::yield_now().await;
        }
    }

    /// Hooks set by id stay in place across `unregister()` and a later
    /// registration of the same id: the added, removed, match and deliver
    /// hooks all apply to the new registration.
    #[tokio::test]
    async fn hooks_by_id_survive_unregister_and_a_new_registration() {
        let (iii, mut rx) = client();
        v2_hello(&iii);
        let events = Arc::new(Mutex::new(Vec::<String>::new()));
        let record = |what: &'static str| {
            let events = events.clone();
            move |obs: Observer| {
                events.lock().unwrap().push(format!("{what} {}", obs.id()));
            }
        };
        let (on_added, on_removed) = (record("added"), record("removed"));
        iii.on_observer_added_for("rr::f", move |obs: Observer| {
            on_added(obs);
            async { Ok::<_, ObserveRejection>(()) }
        })
        .on_observer_removed_for("rr::f", move |obs: Observer| {
            on_removed(obs);
            async {}
        })
        .on_observer_match_for("rr::f", kind_keys())
        .on_observer_deliver_for("rr::f", answer_with_count);
        let events_now = || events.lock().unwrap().clone();

        let first = hosted(
            &iii,
            &mut rx,
            "rr::f",
            json!([view_with_args("a", json!({ "kind": "a" }))]),
        )
        .await;
        first.unregister();
        while events_now().len() < 2 {
            tokio::task::yield_now().await;
        }
        let again = hosted(
            &iii,
            &mut rx,
            "rr::f",
            json!([view_with_args("b", json!({ "kind": "b" }))]),
        )
        .await;
        assert_eq!(events_now(), vec!["added a", "removed a", "added b"]);
        assert!(again.is_observed_for(&kind("b")));
        assert!(!again.is_observed_for(&kind("a")), "matched by keys");
        sent(&mut rx);
        for (input, matched) in [("b", 1), ("a", 0)] {
            feed(
                &iii,
                json!({ "type": "invokefunction", "function_id": "rr::f",
                        "invocation_id": format!("9111111{matched}-2222-4333-8444-555555555555"),
                        "data": { "kind": input } }),
            );
            let answer = next_frame(&mut rx, "invocationresult").await;
            assert_eq!(answer["result"], matched, "the deliver hook answers");
        }
        again.unregister();
        while events_now().len() < 4 {
            tokio::task::yield_now().await;
        }
        assert_eq!(events_now()[3], "removed b");
    }

    /// A standby warns once per wait for the host to leave: its later
    /// conflicts are logged at debug until the engine confirms it.
    #[tokio::test]
    #[tracing_test::traced_test]
    async fn a_standby_warns_once_per_wait() {
        let (iii, _rx) = client();
        let function = iii.register_function("sb::f", echo().standby());
        let conflict = json!({ "type": "registrationrejected", "code": "FUNCTION_NAMESPACE_CONFLICT",
                               "namespace": "default", "function_id": "sb::f",
                               "owner_worker_id": "w2" });
        for _ in 0..3 {
            feed(&iii, conflict.clone());
        }
        feed(
            &iii,
            json!({ "type": "functionregistered", "id": "sb::f", "revision": 1,
                    "local_dispatch": true, "observers": [] }),
        );
        while !function.local_dispatch_active() {
            tokio::task::yield_now().await;
        }
        feed(&iii, conflict);
        logs_assert(|lines: &[&str]| {
            let count = |level: &str, text: &str| {
                lines
                    .iter()
                    .filter(|l| l.contains(level) && l.contains(text))
                    .count()
            };
            match (
                count("WARN", "the standby registers it again until it takes over"),
                count(
                    "DEBUG",
                    "standby: another worker still hosts this function id",
                ),
            ) {
                (2, 2) => Ok(()),
                other => Err(format!("expected (warn, debug) = (2, 2), got {other:?}")),
            }
        });
    }

    /// A peer that stops reading: flushed frames wait in the outbound channel,
    /// and their bytes keep counting against the 8 MiB bound.
    #[tokio::test]
    async fn frames_a_peer_does_not_read_stay_within_the_bound() {
        let (iii, _unread) = client();
        iii.set_connection_state(IIIConnectionState::Connected);
        iii.inner.connection_epoch.store(1, Ordering::SeqCst);
        let function =
            iii.register_function("b::f", RegisterFunction::new(|v: Value| Ok::<_, Error>(v)));
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

    /// Records the observers a hook runs for, as `"<what> <id>"`.
    #[derive(Clone, Default)]
    struct Ran(Arc<Mutex<Vec<String>>>);

    impl Ran {
        fn added(&self, what: &'static str) -> impl crate::AddedHook {
            let ran = self.clone();
            move |obs: Observer| {
                ran.0.lock().unwrap().push(format!("{what} {}", obs.id()));
                async { Ok::<_, ObserveRejection>(()) }
            }
        }

        fn removed(&self, what: &'static str) -> impl RemovedHook {
            let ran = self.clone();
            move |obs: Observer| {
                ran.0.lock().unwrap().push(format!("{what} {}", obs.id()));
                async {}
            }
        }

        fn now(&self) -> Vec<String> {
            self.0.lock().unwrap().clone()
        }

        async fn until(&self, n: usize) {
            while self.0.lock().unwrap().len() < n {
                tokio::task::yield_now().await;
            }
        }
    }

    /// A removed hook that panics is logged once; the observer is removed
    /// either way, the worker's removed hook still runs for it, and later
    /// removals run the hooks again.
    #[tokio::test]
    async fn a_panicking_removed_hook_is_logged_and_the_observer_removed() {
        let warnings = Warnings::default();
        let _capture = tracing::subscriber::set_default(warnings.clone());
        let (iii, mut rx) = client();
        v2_hello(&iii);
        let ran = Ran::default();
        iii.on_observer_removed_for("rp::f", {
            let ran = ran.clone();
            move |obs: Observer| {
                let ran = ran.clone();
                async move {
                    if obs.id() == "boom" {
                        panic!("teardown bug");
                    }
                    ran.0.lock().unwrap().push(format!("function {}", obs.id()));
                }
            }
        })
        .on_observer_removed(ran.removed("worker"));
        let function = hosted(
            &iii,
            &mut rx,
            "rp::f",
            json!([view_with_args("boom", json!({}))]),
        )
        .await;
        feed(
            &iii,
            json!({ "type": "observerremoved", "function_id": "rp::f", "revision": 2, "id": "boom" }),
        );
        ran.until(1).await;
        assert_eq!(ran.now(), vec!["worker boom"]);
        assert!(!function.is_observed());
        assert!(function.observers().is_empty());

        feed(
            &iii,
            json!({ "type": "observeradded", "function_id": "rp::f", "revision": 3,
                    "observer": view_with_args("later", json!({})) }),
        );
        assert_eq!(next_ack(&mut rx).await["id"], "later");
        feed(
            &iii,
            json!({ "type": "observerremoved", "function_id": "rp::f", "revision": 4, "id": "later" }),
        );
        ran.until(3).await;
        assert_eq!(
            ran.now(),
            vec!["worker boom", "function later", "worker later"]
        );
        let lines = warnings.0.lock().unwrap().clone();
        let logged: Vec<&String> = lines
            .iter()
            .filter(|l| l.contains("on_observer_removed panicked"))
            .collect();
        assert_eq!(logged.len(), 1, "{lines:#?}");
        assert!(logged[0].contains("teardown bug"), "{}", logged[0]);
    }

    /// A worker's added hook set once functions have active observers runs
    /// for each of them, with one warning per function naming how many.
    #[tokio::test]
    async fn a_late_worker_hook_is_caught_up_for_every_function() {
        let warnings = Warnings::default();
        let _capture = tracing::subscriber::set_default(warnings.clone());
        let (iii, mut rx) = client();
        hosted(
            &iii,
            &mut rx,
            "wa::f",
            json!([
                view_with_args("a1", json!({})),
                view_with_args("a2", json!({}))
            ]),
        )
        .await;
        hosted(
            &iii,
            &mut rx,
            "wb::f",
            json!([view_with_args("b1", json!({}))]),
        )
        .await;
        let ran = Ran::default();
        iii.on_observer_added(ran.added("worker"));
        ran.until(3).await;
        let mut caught_up = ran.now();
        caught_up.sort();
        assert_eq!(caught_up, vec!["worker a1", "worker a2", "worker b1"]);
        let lines = warnings.0.lock().unwrap().clone();
        let warned: Vec<&String> = lines
            .iter()
            .filter(|l| l.contains("were active before its on_observer_added hook was set"))
            .collect();
        assert_eq!(warned.len(), 2, "{lines:#?}");
        for named in ["2 observer(s) of wa::f", "1 observer(s) of wb::f"] {
            assert!(
                warned.iter().any(|l| l.contains(named)),
                "{named}: {warned:#?}"
            );
        }
    }

    /// Each `clear_*` removes its hook, at both levels: an observer added or
    /// removed afterwards runs none, iii delivers once the deliver hook is
    /// cleared, and a call reaches every observer once the match hook is.
    #[tokio::test]
    async fn cleared_hooks_stop_running() {
        let (iii, mut rx) = client();
        v2_hello(&iii);
        let ran = Ran::default();
        iii.on_observer_added(ran.added("worker added"))
            .on_observer_added_for("cl::f", ran.added("function added"))
            .on_observer_removed(ran.removed("worker removed"))
            .on_observer_removed_for("cl::f", ran.removed("function removed"))
            .on_observer_match_for("cl::f", kind_keys())
            .on_observer_deliver_for("cl::f", answer_with_count);
        hosted(
            &iii,
            &mut rx,
            "cl::f",
            json!([
                view_with_args("a", json!({ "kind": "a" })),
                view_with_args("b", json!({ "kind": "b" }))
            ]),
        )
        .await;
        let with_hooks = vec![
            "worker added a",
            "function added a",
            "worker added b",
            "function added b",
        ];
        assert_eq!(ran.now(), with_hooks);
        let routed = |n: u32, input: Value| {
            feed(
                &iii,
                json!({ "type": "invokefunction", "function_id": "cl::f",
                        "invocation_id": format!("c111111{n}-2222-4333-8444-555555555555"),
                        "data": input }),
            );
        };
        sent(&mut rx);
        routed(0, json!({ "kind": "a" }));
        let answer = next_frame(&mut rx, "invocationresult").await;
        assert_eq!(
            answer["result"], 1,
            "the deliver hook got the one of kind a"
        );
        assert!(answer.get("observed").is_none());

        iii.clear_observer_added()
            .clear_observer_added_for("cl::f")
            .clear_observer_removed()
            .clear_observer_removed_for("cl::f");
        feed(
            &iii,
            json!({ "type": "observeradded", "function_id": "cl::f", "revision": 2,
                    "observer": view_with_args("c", json!({ "kind": "b" })) }),
        );
        assert_eq!(next_ack(&mut rx).await["id"], "c");
        feed(
            &iii,
            json!({ "type": "observerremoved", "function_id": "cl::f", "revision": 3, "id": "a" }),
        );
        // processed in order: once d is acked, a's removal has run
        feed(
            &iii,
            json!({ "type": "observeradded", "function_id": "cl::f", "revision": 4,
                    "observer": view_with_args("d", json!({ "kind": "d" })) }),
        );
        assert_eq!(next_ack(&mut rx).await["id"], "d");
        assert_eq!(ran.now(), with_hooks, "no hook ran once cleared");

        // iii delivers: the match hook still picks the observers of kind b
        iii.clear_observer_deliver_for("cl::f");
        sent(&mut rx);
        routed(1, json!({ "kind": "b" }));
        let answer = next_frame(&mut rx, "invocationresult").await;
        assert_eq!(answer["result"], json!({ "kind": "b" }), "the handler's");
        let mut observed: Vec<String> = serde_json::from_value(answer["observed"].clone()).unwrap();
        observed.sort();
        assert_eq!(observed, vec!["b", "c"]);

        // every observer wants every call
        iii.clear_observer_match_for("cl::f");
        routed(2, json!({ "kind": "b" }));
        let answer = next_frame(&mut rx, "invocationresult").await;
        let mut observed: Vec<String> = serde_json::from_value(answer["observed"].clone()).unwrap();
        observed.sort();
        assert_eq!(observed, vec!["b", "c", "d"]);
    }

    /// Matched by keys, a call's keys are computed once per call, routed or
    /// local.
    #[tokio::test]
    async fn call_keys_are_computed_once_per_call() {
        let (iii, mut rx) = client();
        v2_hello(&iii);
        let computed = Arc::new(AtomicUsize::new(0));
        iii.on_observer_match_for(
            "ck::f",
            crate::Match::keys(|obs| obs.args_value()["kind"].as_str().into(), {
                let computed = computed.clone();
                move |call| {
                    computed.fetch_add(1, Ordering::SeqCst);
                    call.input["kind"].as_str().into()
                }
            }),
        );
        let function = hosted(
            &iii,
            &mut rx,
            "ck::f",
            json!([
                view_with_args("a", json!({ "kind": "a" })),
                view_with_args("b", json!({ "kind": "b" }))
            ]),
        )
        .await;
        assert_eq!(computed.load(Ordering::SeqCst), 0);
        sent(&mut rx);
        let observed = routed_observed(
            &iii,
            &mut rx,
            "ck::f",
            "d1111111-2222-4333-8444-555555555555",
            json!({ "kind": "a" }),
        )
        .await;
        assert_eq!(observed, json!(["a"]));
        assert_eq!(
            computed.load(Ordering::SeqCst),
            1,
            "once for the routed call"
        );
        while !function.local_dispatch_active() {
            tokio::task::yield_now().await;
        }
        iii.call(CallRequest::new("ck::f", json!({ "kind": "b" })))
            .await
            .unwrap();
        assert_eq!(
            computed.load(Ordering::SeqCst),
            2,
            "once for the local call"
        );
    }

    /// A standby registers again 1 s ±20 % after each conflict, and not once
    /// it is unregistered.
    #[tokio::test(start_paused = true)]
    async fn a_standby_retries_every_second_until_unregistered() {
        let (iii, mut rx) = client();
        let function = iii.register_function("sb::retry", echo().standby());
        sent(&mut rx);
        let conflict = json!({ "type": "registrationrejected", "code": "FUNCTION_NAMESPACE_CONFLICT",
                               "namespace": "default", "function_id": "sb::retry",
                               "owner_worker_id": "w2" });
        let registrations = |frames: Vec<Value>| {
            frames
                .iter()
                .filter(|m| m["type"] == "registerfunction" && m["id"] == "sb::retry")
                .count()
        };
        const STEP: Duration = Duration::from_millis(5);
        let mut delays = Vec::new();
        for _ in 0..40 {
            feed(&iii, conflict.clone());
            let started = tokio::time::Instant::now();
            loop {
                tokio::time::sleep(STEP).await;
                match registrations(sent(&mut rx)) {
                    0 => assert!(started.elapsed() < Duration::from_secs(2), "no retry"),
                    1 => break,
                    n => panic!("{n} registrations for one conflict"),
                }
            }
            delays.push(started.elapsed());
        }
        let (fewest, most) = (*delays.iter().min().unwrap(), *delays.iter().max().unwrap());
        assert!(fewest >= Duration::from_millis(800), "{delays:?}");
        assert!(most <= Duration::from_millis(1_200) + STEP, "{delays:?}");
        assert!(
            fewest < Duration::from_secs(1) && most > Duration::from_secs(1),
            "jittered both ways: {delays:?}"
        );

        // unregistered while it waits: no retry
        feed(&iii, conflict);
        function.unregister();
        tokio::time::sleep(Duration::from_secs(5)).await;
        assert_eq!(registrations(sent(&mut rx)), 0);
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

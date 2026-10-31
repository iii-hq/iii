//! Observing: `iii.observe(source, target)`.
//!
//! When the source function runs, the target runs. The first argument is the
//! left side, what is observed: a function, the `args` the worker that hosts
//! it reads and the `filters` that pick which of its calls reach the target.
//! The second is the right side: a function registered by any worker, called
//! once per matching call. The optional third ([`ObserverOptions`]) is the
//! observer's policy and metadata.
//!
//! ```rust,no_run
//! use iii_sdk::{Capture, IIIClient, ObserverOptions, Source};
//! use serde_json::json;
//!
//! # async fn demo(iii: &IIIClient) -> Result<(), iii_sdk::Error> {
//! // every successful call of orders::create runs alerts::new-order
//! iii.observe("orders::create", "alerts::new-order");
//!
//! // large orders only
//! let big_orders = iii.observe(
//!     Source::function("orders::create")
//!         .filters(json!({ "input.total": { "$gte": 500 } })),
//!     "alerts::big-order",
//! );
//! big_orders.ready().await?;
//!
//! // a source with args for the worker that hosts it
//! iii.observe(
//!     Source::function("cron::on-tick").args(json!({ "expression": "0 0 9 * * * *" })),
//!     "reports::daily-summary",
//! );
//!
//! // failed charges above 500, with a policy
//! iii.observe_with(
//!     Source::function("payments::charge")
//!         .on(iii_sdk::On::Error)
//!         .filters(json!({ "input.amount": { "$gte": 500 } }))
//!         .capture([Capture::Input, Capture::Error, Capture::Caller]),
//!     "alerts::payment-failed",
//!     ObserverOptions::default().max_in_flight(4),
//! );
//! # Ok(()) }
//! ```

use std::{
    fmt,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::watch;

use crate::{
    error::Error,
    iii::IIIClient,
    observable::coded_error,
    protocol::{
        Caller, Capture, Deliver, ErrorBody, InvocationStatus, Message, ObserveSource,
        ObserveTarget, ObserverState, On, observe_codes,
    },
};

/// How long [`ObserverHandle::dispose`] waits for the engine to confirm.
const DISPOSE_WAIT: Duration = Duration::from_secs(5);

/// The left side of an observer: what is observed.
#[derive(Debug, Clone, PartialEq)]
pub struct Source {
    function_id: String,
    namespace: Option<String>,
    args: Option<Value>,
    filters: Option<Value>,
    capture: Option<Vec<Capture>>,
    on: Option<On>,
    deliver: Option<Deliver>,
}

impl Source {
    /// Observe `function_id`. Every function is observable.
    pub fn function(function_id: impl Into<String>) -> Self {
        Self {
            function_id: function_id.into(),
            namespace: None,
            args: None,
            filters: None,
            capture: None,
            on: None,
            deliver: None,
        }
    }

    /// The namespace the function is in. Unset, it resolves like a
    /// [`call`](crate::IIIClient::call): this worker's namespace (`default`
    /// for `engine::*`).
    pub fn namespace(mut self, namespace: impl Into<String>) -> Self {
        self.namespace = Some(namespace.into());
        self
    }

    /// Args for the worker that hosts the function: what to set up for this
    /// observer and which calls it wants, validated against the function's
    /// `args_format` (`INVALID_ARGS`). The function's description says what
    /// they mean.
    pub fn args(mut self, args: Value) -> Self {
        self.args = Some(args);
        self
    }

    /// Which calls reach the target: a filter over `input`, `output`,
    /// `error`, `caller` and `metadata`, with explicit operators
    /// (`{ "input.amount": { "$gte": 500 } }`). Unset matches every call.
    pub fn filters(mut self, filters: Value) -> Self {
        self.filters = Some(filters);
        self
    }

    /// What travels to the target. Default `[input, output, caller]`.
    pub fn capture(mut self, capture: impl IntoIterator<Item = Capture>) -> Self {
        self.capture = Some(capture.into_iter().collect());
        self
    }

    /// Which outcomes reach the target. Default [`On::Success`].
    pub fn on(mut self, on: On) -> Self {
        self.on = Some(on);
        self
    }

    /// What the target receives when iii delivers: the [`Observed`] envelope
    /// (default), or the observed input or output alone. A function whose
    /// deliver hook calls targets itself documents what they receive.
    pub fn deliver(mut self, deliver: Deliver) -> Self {
        self.deliver = Some(deliver);
        self
    }

    /// The observed function id.
    pub fn function_id(&self) -> &str {
        &self.function_id
    }
}

/// `iii.observe("orders::create", target)`: every successful call of the
/// function, with the defaults.
impl From<&str> for Source {
    fn from(function_id: &str) -> Self {
        Source::function(function_id)
    }
}

impl From<String> for Source {
    fn from(function_id: String) -> Self {
        Source::function(function_id)
    }
}

impl From<&String> for Source {
    fn from(function_id: &String) -> Self {
        Source::function(function_id.clone())
    }
}

/// The right side of an observer: a registered function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    function_id: String,
    namespace: Option<String>,
}

impl Target {
    pub fn function(function_id: impl Into<String>) -> Self {
        Self {
            function_id: function_id.into(),
            namespace: None,
        }
    }

    /// The target's namespace. Unset, it resolves like a
    /// [`call`](crate::IIIClient::call).
    pub fn namespace(mut self, namespace: impl Into<String>) -> Self {
        self.namespace = Some(namespace.into());
        self
    }
}

impl From<&str> for Target {
    fn from(function_id: &str) -> Self {
        Target::function(function_id)
    }
}

impl From<String> for Target {
    fn from(function_id: String) -> Self {
        Target::function(function_id)
    }
}

impl From<&String> for Target {
    fn from(function_id: &String) -> Self {
        Target::function(function_id.clone())
    }
}

/// The observer itself: its id, policy and metadata sidecar.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ObserverOptions {
    id: Option<String>,
    max_fires: Option<u64>,
    expires_in_ms: Option<u64>,
    max_in_flight: Option<u32>,
    metadata: Option<Value>,
}

impl ObserverOptions {
    /// The observer's id. Default: a random UUID v4, kept across replays.
    ///
    /// The handle [`observe_with`](IIIClient::observe_with) returns holds
    /// the id until its [`dispose`](ObserverHandle::dispose) is called or the
    /// engine disposes the observer, and the id is free again at once: a
    /// queue consumer observes again under its stable id as soon as it
    /// disposes the observer before. While a handle of this client holds the
    /// id, in any state, another `observe_with` under it is refused with
    /// nothing sent (see [`observe_with`](IIIClient::observe_with)).
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Dispose of the observer after this many activations.
    pub fn max_fires(mut self, max_fires: u64) -> Self {
        self.max_fires = Some(max_fires);
        self
    }

    /// Dispose of the observer after this long.
    pub fn expires_in(mut self, expires_in: Duration) -> Self {
        self.expires_in_ms = Some(expires_in.as_millis() as u64);
        self
    }

    /// At most this many activations running at once; more queue in the
    /// engine, and a call through the observer fails with `OVERFLOW`.
    pub fn max_in_flight(mut self, max_in_flight: u32) -> Self {
        self.max_in_flight = Some(max_in_flight);
        self
    }

    /// Delivered as the metadata sidecar of every activation. The worker that
    /// hosts the function never sees it.
    pub fn metadata(mut self, metadata: Value) -> Self {
        self.metadata = Some(metadata);
        self
    }
}

/// Why an observer is in its state: `{ code, message }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObserverReason {
    pub code: String,
    pub message: String,
}

impl From<ErrorBody> for ObserverReason {
    fn from(body: ErrorBody) -> Self {
        Self {
            code: body.code,
            message: body.message,
        }
    }
}

/// An observer's state, why, and how often it fired.
#[derive(Debug, Clone, PartialEq)]
pub struct ObserverStatus {
    pub state: ObserverState,
    pub reason: Option<ObserverReason>,
    pub fires: u64,
}

type Listener = Arc<dyn Fn(&ObserverState, Option<&ObserverReason>) + Send + Sync>;

pub(crate) type Delivery = Box<dyn FnOnce() + Send>;

/// Runs state listeners on a thread of their own, in order: never on the
/// connection's read loop, which a listener calling the client would stall.
pub(crate) fn deliver(job: Delivery) {
    static QUEUE: OnceLock<Mutex<std::sync::mpsc::Sender<Delivery>>> = OnceLock::new();
    let queue = QUEUE.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<Delivery>();
        std::thread::Builder::new()
            .name("iii-observer-events".into())
            .spawn(move || {
                for job in rx {
                    // each job catches its listeners' panics
                    job();
                }
            })
            .expect("failed to spawn the observer events thread");
        Mutex::new(tx)
    });
    let _ = queue.lock().unwrap_or_else(|e| e.into_inner()).send(job);
}

/// The observing worker's record of one observer, replayed on reconnect.
pub(crate) struct ObserverEntry {
    pub(crate) id: String,
    /// When it was created, among this process's observers: replays send
    /// observers in this order, so the engine announces them to their
    /// functions' hosts in the order the worker created them.
    pub(crate) seq: u64,
    /// The `observe` message, sent as is on every replay.
    pub(crate) message: Message,
    status: watch::Sender<ObserverStatus>,
    warnings: Mutex<Vec<String>>,
    listeners: Mutex<Vec<(u64, Listener)>>,
    next_listener: AtomicU64,
    /// Sent to an engine at least once: `dispose` must tell it.
    pub(crate) sent: AtomicBool,
    /// The connection epoch its `observe` last went out on
    /// ([`NOT_SENT`] before that): sent once per connection.
    pub(crate) sent_epoch: AtomicU64,
    /// `dispose` was called: never replayed again.
    pub(crate) disposing: AtomicBool,
    /// The engine refused its `observe` (an `observeresult` in `error`):
    /// that is its final answer. The engine keeps nothing for it, so it is
    /// never sent again and `dispose` sends no `unobserve`. Set under the
    /// listeners lock, with the transition to `error`.
    refused: AtomicBool,
    /// Orders a replayed `observe` against `dispose`'s `unobserve`: whichever
    /// takes it first, the other sees its outcome.
    pub(crate) send_lock: Mutex<()>,
}

/// The next [`ObserverEntry::seq`].
static NEXT_OBSERVER_SEQ: AtomicU64 = AtomicU64::new(0);

/// [`ObserverEntry::sent_epoch`] of an observer not sent yet.
pub(crate) const NOT_SENT: u64 = u64::MAX;

impl ObserverEntry {
    pub(crate) fn new(id: String, message: Message) -> Self {
        let (status, _) = watch::channel(ObserverStatus {
            state: ObserverState::Pending,
            reason: None,
            fires: 0,
        });
        Self {
            id,
            seq: NEXT_OBSERVER_SEQ.fetch_add(1, Ordering::Relaxed),
            message,
            status,
            warnings: Mutex::new(Vec::new()),
            listeners: Mutex::new(Vec::new()),
            next_listener: AtomicU64::new(0),
            sent: AtomicBool::new(false),
            sent_epoch: AtomicU64::new(NOT_SENT),
            disposing: AtomicBool::new(false),
            refused: AtomicBool::new(false),
            send_lock: Mutex::new(()),
        }
    }

    pub(crate) fn state(&self) -> ObserverState {
        self.status.borrow().state.clone()
    }

    /// The engine refused its `observe` ([`refuse`](Self::refuse)).
    pub(crate) fn is_refused(&self) -> bool {
        self.refused.load(Ordering::Acquire)
    }

    /// Records a transition and tells the listeners. `disposed` is final: a
    /// later transition to another state changes nothing.
    pub(crate) fn set(
        &self,
        state: ObserverState,
        reason: Option<ObserverReason>,
        fires: Option<u64>,
    ) {
        self.transition(state, reason, fires, false);
    }

    /// The engine refused its `observe`: `error` with the engine's reason,
    /// the handle's final answer.
    pub(crate) fn refuse(&self, reason: Option<ObserverReason>) {
        self.transition(ObserverState::Error, reason, None, true);
    }

    fn transition(
        &self,
        state: ObserverState,
        reason: Option<ObserverReason>,
        fires: Option<u64>,
        refused: bool,
    ) {
        // Held across the status write and the enqueue: two transitions from
        // different threads reach the listeners in the order they took effect.
        let listeners = self.listeners.lock().unwrap_or_else(|e| e.into_inner());
        if refused {
            self.refused.store(true, Ordering::Release);
        }
        let mut changed = false;
        self.status.send_modify(|status| {
            if status.state == ObserverState::Disposed && state != ObserverState::Disposed {
                return;
            }
            changed = status.state != state || status.reason != reason;
            status.state = state.clone();
            status.reason = reason.clone();
            if let Some(fires) = fires {
                status.fires = fires;
            }
        });
        if !changed || listeners.is_empty() {
            return;
        }
        let listeners: Vec<Listener> = listeners.iter().map(|(_, l)| l.clone()).collect();
        deliver(Box::new(move || {
            for listener in listeners {
                // one listener's panic does not cost the others the transition
                let call = std::panic::AssertUnwindSafe(|| listener(&state, reason.as_ref()));
                if let Err(panic) = std::panic::catch_unwind(call) {
                    tracing::warn!(
                        "observer state listener panicked: {}",
                        crate::observable::panic_message(&panic)
                    );
                }
            }
        }));
    }

    pub(crate) fn set_warnings(&self, warnings: Vec<String>) {
        if warnings.is_empty() {
            return;
        }
        for warning in &warnings {
            tracing::warn!(observer_id = %self.id, "observe: {warning}");
        }
        *self.warnings.lock().unwrap_or_else(|e| e.into_inner()) = warnings;
    }
}

/// Handle to an observer created by [`IIIClient::observe`]. Cheap to clone.
///
/// The observer exists until [`dispose`](Self::dispose) (or its policy runs
/// out); dropping the handle does not remove it. It works before the
/// connection is up and is replayed with the same id after a reconnect.
#[derive(Clone)]
pub struct ObserverHandle {
    pub(crate) entry: Arc<ObserverEntry>,
    pub(crate) iii: IIIClient,
}

impl fmt::Debug for ObserverHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ObserverHandle")
            .field("id", &self.entry.id)
            .field("status", &*self.entry.status.borrow())
            .finish()
    }
}

/// A listener added with [`ObserverHandle::on_state_change`].
#[derive(Debug)]
pub struct StateListener {
    entry: std::sync::Weak<ObserverEntry>,
    id: u64,
}

impl StateListener {
    /// Stops calling the listener.
    pub fn unsubscribe(self) {
        if let Some(entry) = self.entry.upgrade() {
            entry
                .listeners
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .retain(|(id, _)| *id != self.id);
        }
    }
}

impl ObserverHandle {
    /// The observer's id.
    pub fn id(&self) -> &str {
        &self.entry.id
    }

    /// The current state.
    pub fn state(&self) -> ObserverState {
        self.entry.state()
    }

    /// Why it is in `error` (or `suspended`, `disposed`): an engine code such
    /// as `INVALID_ARGS`, `FORBIDDEN`, `UNSUPPORTED`, or the rejection code of
    /// an added hook of the worker that hosts the function.
    pub fn reason(&self) -> Option<ObserverReason> {
        self.entry.status.borrow().reason.clone()
    }

    /// Activations so far, as last reported by the engine.
    pub fn fires(&self) -> u64 {
        self.entry.status.borrow().fires
    }

    /// The engine's warnings about the filters (paths it could not verify
    /// against an open schema).
    pub fn warnings(&self) -> Vec<String> {
        self.entry
            .warnings
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Resolves once the observer is `active`. Fails with the reason's code
    /// (`Error::Remote`) when it ends in `error` or `disposed`; waits through
    /// `pending` and `suspended`.
    pub async fn ready(&self) -> Result<(), Error> {
        let mut rx = self.entry.status.subscribe();
        loop {
            let status = rx.borrow_and_update().clone();
            match status.state {
                ObserverState::Active => return Ok(()),
                ObserverState::Error | ObserverState::Disposed => {
                    let reason = status.reason.unwrap_or_else(|| ObserverReason {
                        code: if status.state == ObserverState::Error {
                            "OBSERVER_ERROR".to_string()
                        } else {
                            observe_codes::UNOBSERVED.to_string()
                        },
                        message: format!("observer {} is {}", self.entry.id, status.state),
                    });
                    return Err(coded_error(&reason.code, reason.message));
                }
                _ => {}
            }
            if rx.changed().await.is_err() {
                return Err(Error::NotConnected);
            }
        }
    }

    /// Calls `listener` on every state change. Listeners run on one
    /// process-wide thread, one call at a time and in the order the changes
    /// happened, never on the connection's read loop: a listener may call the
    /// client. A listener that blocks delays every client's listeners in the
    /// process; hand slow work to a task of its own.
    pub fn on_state_change<F>(&self, listener: F) -> StateListener
    where
        F: Fn(&ObserverState, Option<&ObserverReason>) + Send + Sync + 'static,
    {
        let id = self.entry.next_listener.fetch_add(1, Ordering::Relaxed);
        self.entry
            .listeners
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((id, Arc::new(listener)));
        StateListener {
            entry: Arc::downgrade(&self.entry),
            id,
        }
    }

    /// Removes the observer, and frees its id at once: an
    /// [`observe_with`](IIIClient::observe_with) under the same id makes a
    /// new observer while this one is still being disposed, and each handle
    /// hears the engine's answers to its own messages only. Once it resolves
    /// (the engine confirmed it, or after 5 s without an answer), no new
    /// activation starts and a call through it fails with
    /// `OBSERVER_NOT_ACTIVE`. A handle `observe_with` refused (state
    /// `error`, with no observer of its own) sends nothing and leaves the
    /// handle that holds the id alone. The engine's refusal of an `observe`
    /// (its answer in state `error`) is the handle's final answer: the
    /// engine keeps no observer for it and the handle is not sent again, so
    /// `dispose()` sends nothing and resolves at once, or, called before
    /// the refusal arrives, when it arrives.
    pub async fn dispose(&self) -> Result<(), Error> {
        let mut rx = self.entry.status.subscribe();
        let answered_here = {
            let _order = self
                .entry
                .send_lock
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if self.entry.state() == ObserverState::Disposed
                || self.entry.disposing.swap(true, Ordering::AcqRel)
            {
                return Ok(());
            }
            // Only an observer some engine holds needs an `unobserve`: one
            // never sent stays unsent (`send_observe` checks `disposing`),
            // and the engine keeps nothing for an `observe` it refused.
            let unobserved = self.entry.sent.load(Ordering::Acquire)
                && !self.entry.is_refused()
                && match self.iii.send_message(Message::Unobserve {
                    id: self.entry.id.clone(),
                }) {
                    Ok(()) => true,
                    Err(error) => {
                        tracing::debug!(observer_id = %self.entry.id, error = %error, "unobserve not sent");
                        false
                    }
                };
            // The id is free once the `unobserve` is queued: a new observer's
            // `observe` goes out after it, and the engine reads them in order.
            self.iii.forget_observer(&self.entry);
            // The engine's `disposed` reaches this handle when its `observe`
            // went out on this connection; an earlier connection's is not
            // routed to it.
            unobserved
                && self.iii.engine_observes()
                && self.entry.sent_epoch.load(Ordering::Acquire) == self.iii.connection_epoch()
        };
        if answered_here {
            // Its `disposed`, or the refusal of its `observe`: the engine
            // ignores the `unobserve` of an observer it refused.
            let _ = tokio::time::timeout(DISPOSE_WAIT, async {
                loop {
                    if rx.borrow_and_update().state == ObserverState::Disposed
                        || self.entry.is_refused()
                    {
                        return;
                    }
                    if rx.changed().await.is_err() {
                        return;
                    }
                }
            })
            .await;
        }
        if self.entry.state() != ObserverState::Disposed {
            self.entry.set(
                ObserverState::Disposed,
                Some(ObserverReason {
                    code: observe_codes::UNOBSERVED.to_string(),
                    message: "disposed".to_string(),
                }),
                None,
            );
        }
        Ok(())
    }
}

/// What an observer's target receives with the default `deliver: envelope`:
/// the observed call, per the observer's `capture`.
///
/// ```rust,no_run
/// # use iii_sdk::{Error, IIIClient, Observed, RegisterFunction};
/// # use serde_json::Value;
/// # fn demo(iii: &IIIClient) {
/// iii.register_function(
///     "alerts::payment-failed",
///     RegisterFunction::new(|event: Observed<Value>| {
///         println!("{} failed: {:?}", event.function_id, event.error);
///         Ok::<_, Error>(())
///     }),
/// );
/// # }
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Observed<I = Value, O = Value> {
    pub observer_id: String,
    pub activation_id: String,
    pub function_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    pub invocation_id: String,
    pub status: InvocationStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<I>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<O>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ObservedError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caller: Option<Caller>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
    pub started_at_ms: u64,
    pub duration_ms: f64,
}

/// What a call failed with: [`Observed::error`], [`Call::error`](crate::Call::error).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ObservedError {
    pub code: String,
    pub message: String,
}

impl IIIClient {
    /// Observes `source` and calls `target` once per matching call.
    ///
    /// `source` is a [`Source`], or a function id for every successful call
    /// with the defaults. Returns at once, before the engine answered: the
    /// handle starts `pending`; [`ObserverHandle::ready`] resolves on
    /// `active` and fails on `error` (`INVALID_ARGS`, `INVALID_FILTER`,
    /// `FORBIDDEN`, `NOT_OBSERVABLE`, an added hook's rejection,
    /// `UNSUPPORTED` against an engine without observe). There is no inline
    /// handler: `target` is a registered function, and the observer stays
    /// `pending` until it is registered.
    pub fn observe(&self, source: impl Into<Source>, target: impl Into<Target>) -> ObserverHandle {
        self.observe_with(source, target, ObserverOptions::default())
    }

    /// [`observe`](Self::observe) with the observer's id, policy and metadata.
    ///
    /// An id ([`ObserverOptions::id`]) that a handle of this client holds,
    /// in any state, is refused at the call: nothing is sent, the holding
    /// handle and its observer are untouched, and the handle returned is in
    /// state `error` with the reason `OBSERVER_ID_CONFLICT`,
    /// `observer id already in use: <id>`. Its [`ready`](ObserverHandle::ready)
    /// fails with that code, and its [`dispose`](ObserverHandle::dispose)
    /// sends nothing.
    ///
    /// The id is free again as soon as `dispose()` is called on the handle
    /// that held it, before the engine confirms the disposal. The engine answers a connection's
    /// observes in the order it reads them, so each handle of the id gets the
    /// answers to its own `observe` and `unobserve`: a late `disposed` for
    /// the earlier observer never disposes the new one.
    pub fn observe_with(
        &self,
        source: impl Into<Source>,
        target: impl Into<Target>,
        options: ObserverOptions,
    ) -> ObserverHandle {
        let source = source.into();
        let target = target.into();
        let id = options
            .id
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

        let built = self.build_observe(&id, source, target, options);
        let (message, early_error) = match built {
            Ok(message) => (message, None),
            Err(reason) => (Message::Unobserve { id: id.clone() }, Some(reason)),
        };
        let entry = Arc::new(ObserverEntry::new(id, message));
        let handle = ObserverHandle {
            entry: entry.clone(),
            iii: self.clone(),
        };
        if let Some(reason) = early_error {
            entry.set(ObserverState::Error, Some(reason), None);
            return handle;
        }
        if !self.track_observer(entry.clone()) {
            let reason = ObserverReason {
                code: observe_codes::OBSERVER_ID_CONFLICT.to_string(),
                message: format!("observer id already in use: {}", entry.id),
            };
            entry.set(ObserverState::Error, Some(reason), None);
        }
        handle
    }

    fn build_observe(
        &self,
        id: &str,
        source: Source,
        target: Target,
        options: ObserverOptions,
    ) -> Result<Message, ObserverReason> {
        let invalid = |code: &str, message: String| ObserverReason {
            code: code.to_string(),
            message,
        };
        if let Some(filters) = &source.filters {
            if let Err(error) = iii_filter::Filter::parse(filters) {
                return Err(invalid(error.code.as_str(), error.message));
            }
        }
        let source_namespace = crate::iii::invocation_namespace(
            source.namespace,
            self.namespace(),
            &source.function_id,
            "Source.namespace",
        )
        .map_err(|e| invalid(observe_codes::INVALID_ARGS, e.to_string()))?;
        let target_namespace = crate::iii::invocation_namespace(
            target.namespace,
            self.namespace(),
            &target.function_id,
            "Target.namespace",
        )
        .map_err(|e| invalid(observe_codes::INVALID_ARGS, e.to_string()))?;
        Ok(Message::Observe {
            id: id.to_string(),
            source: Box::new(ObserveSource {
                function_id: source.function_id,
                namespace: source_namespace,
                args: source.args,
                filters: source.filters,
                capture: source.capture,
                on: source.on,
                deliver: source.deliver,
            }),
            target: ObserveTarget {
                function_id: target.function_id,
                namespace: target_namespace,
            },
            metadata: options.metadata,
            max_fires: options.max_fires,
            expires_in_ms: options.expires_in_ms,
            max_in_flight: options.max_in_flight,
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn source_builds_the_fixture_observe() {
        let iii = IIIClient::new("ws://127.0.0.1:0");
        let message = iii
            .build_observe(
                "5b0c3e1a-8f2d-4c6b-9a7e-1d2f3a4b5c6d",
                Source::function("cron::on-tick")
                    .namespace("default")
                    .args(json!({ "expression": "0 0 9 * * Mon-Fri" }))
                    .filters(json!({ "input.scheduled_time": { "$gte": "2026-11-01T00:00:00Z" } }))
                    .capture([Capture::Input])
                    .on(On::Success)
                    .deliver(Deliver::Envelope),
                Target::function("local::cron-handler").namespace("default"),
                ObserverOptions::default()
                    .metadata(json!({ "__binding": "weekday-digest" }))
                    .max_fires(1)
                    .expires_in(Duration::from_secs(600))
                    .max_in_flight(64),
            )
            .unwrap();
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../../fixtures/protocol/observe/observe.json"
        ))
        .unwrap();
        assert_eq!(serde_json::to_value(&message).unwrap(), fixture);
    }

    #[test]
    fn a_function_id_is_the_minimal_source() {
        let iii = IIIClient::new("ws://127.0.0.1:0");
        let message = iii
            .build_observe(
                "d94f3a5b-6c7d-4e8f-a01b-2c3d4e5f6a7b",
                "orders::archive".into(),
                "search::reindex-order".into(),
                ObserverOptions::default(),
            )
            .unwrap();
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../../fixtures/protocol/observe/observe-minimal.json"
        ))
        .unwrap();
        assert_eq!(serde_json::to_value(&message).unwrap(), fixture);
        assert_eq!(Source::from(String::from("a::b")), Source::function("a::b"));
    }

    #[test]
    fn an_invalid_filter_fails_without_a_round_trip() {
        let iii = IIIClient::new("ws://127.0.0.1:0");
        let handle = iii.observe(
            Source::function("a::b").filters(json!({ "input.op": "insert" })),
            "c::d",
        );
        assert_eq!(handle.state(), ObserverState::Error);
        assert_eq!(handle.reason().unwrap().code, "INVALID_FILTER");
    }

    #[test]
    fn a_panicking_listener_does_not_cost_the_others_the_transition() {
        let entry = Arc::new(ObserverEntry::new(
            "o".into(),
            Message::Unobserve { id: "o".into() },
        ));
        let handle = ObserverHandle {
            entry: entry.clone(),
            iii: IIIClient::new("ws://127.0.0.1:0"),
        };
        let (tx, rx) = std::sync::mpsc::channel();
        let _boom = handle.on_state_change(|_, _| panic!("listener bug"));
        let _next = handle.on_state_change(move |state, _| {
            let _ = tx.send(state.clone());
        });
        entry.set(ObserverState::Active, None, None);
        let got = rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("the second listener still ran");
        assert_eq!(got, ObserverState::Active);
    }

    #[test]
    fn a_filter_nested_too_deep_is_invalid_at_observe() {
        let iii = IIIClient::new("ws://127.0.0.1:0");
        let mut literal = json!(1);
        for _ in 0..130 {
            literal = json!([literal]);
        }
        let handle = iii.observe(
            Source::function("a::b").filters(json!({ "input.x": { "$eq": literal } })),
            "c::d",
        );
        assert_eq!(handle.state(), ObserverState::Error);
        assert_eq!(handle.reason().unwrap().code, "INVALID_FILTER");
    }

    #[tokio::test]
    async fn ready_follows_the_state() {
        let entry = Arc::new(ObserverEntry::new(
            "o".into(),
            Message::Unobserve { id: "o".into() },
        ));
        let handle = ObserverHandle {
            entry: entry.clone(),
            iii: IIIClient::new("ws://127.0.0.1:0"),
        };
        let seen = Arc::new(Mutex::new(Vec::new()));
        let listener = {
            let seen = seen.clone();
            handle.on_state_change(move |state, _| {
                seen.lock()
                    .unwrap()
                    .push((state.clone(), std::thread::current().id()))
            })
        };
        let waiter = tokio::spawn({
            let handle = handle.clone();
            async move { handle.ready().await }
        });
        tokio::task::yield_now().await;
        entry.set(ObserverState::Suspended, None, None);
        entry.set(ObserverState::Active, None, Some(3));
        let ready = tokio::time::timeout(std::time::Duration::from_secs(5), waiter)
            .await
            .expect("ready() resolves on active");
        assert!(ready.unwrap().is_ok());
        assert_eq!(handle.fires(), 3);
        listener.unsubscribe();
        entry.set(
            ObserverState::Error,
            Some(ObserverReason {
                code: "HTTP_ROUTE_CONFLICT".into(),
                message: "taken".into(),
            }),
            None,
        );
        let error = tokio::time::timeout(std::time::Duration::from_secs(5), handle.ready())
            .await
            .expect("ready() fails on error")
            .unwrap_err();
        assert_eq!(
            error.invocation_error().unwrap().code,
            "HTTP_ROUTE_CONFLICT"
        );
        // delivered off this thread, in order; the third came after unsubscribe
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while seen.lock().unwrap().len() < 2 && std::time::Instant::now() < deadline {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
        let seen = seen.lock().unwrap().clone();
        let states: Vec<ObserverState> = seen.iter().map(|(s, _)| s.clone()).collect();
        assert_eq!(
            states,
            vec![ObserverState::Suspended, ObserverState::Active]
        );
        assert!(
            seen.iter()
                .all(|(_, thread)| *thread != std::thread::current().id()),
            "listeners run off the caller's thread"
        );
    }
}

#[cfg(test)]
mod envelope_tests {
    use super::*;

    #[test]
    fn the_activation_fixture_decodes_as_observed() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../../fixtures/protocol/observe/invokefunction-activation.json"
        ))
        .unwrap();
        let envelope: Observed = serde_json::from_value(fixture["data"].clone()).unwrap();
        assert_eq!(envelope.function_id, "payments::charge");
        assert_eq!(envelope.status, InvocationStatus::Error);
        assert_eq!(envelope.error.unwrap().code, "CARD_DECLINED");
        assert!(envelope.output.is_none());
        assert_eq!(
            envelope.caller.unwrap().worker_name.as_deref(),
            Some("checkout")
        );
        assert_eq!(
            serde_json::to_value(&envelope.input).unwrap()["amount"],
            1250
        );

        // `capture: []` (D14): ids, status and timings only
        let bare: Observed = serde_json::from_value(serde_json::json!({
            "observer_id": "o", "activation_id": "a", "function_id": "f",
            "invocation_id": "i", "status": "success", "started_at_ms": 1, "duration_ms": 0.5
        }))
        .unwrap();
        assert!(bare.input.is_none() && bare.caller.is_none());
    }
}

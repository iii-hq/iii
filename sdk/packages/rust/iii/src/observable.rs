//! Observable functions: the owner's side of observe.
//!
//! A function registered with [`RegisterFunction::observe`](crate::RegisterFunction::observe)
//! learns who observes it through an [`ObserveHandler`], and, when it is
//! owner-delivered, hands each event to the observers it belongs to with
//! [`Observer::emit`] or [`Observer::call`].
//!
//! ```rust,no_run
//! use iii_sdk::{
//!     Error, IIIClient, Observe, ObserveHandler, ObserveRejection, Observer, RegisterFunction,
//! };
//! use schemars::JsonSchema;
//! use serde::{Deserialize, Serialize};
//! use std::{collections::HashMap, sync::{Arc, Mutex}};
//!
//! #[derive(Deserialize, JsonSchema)]
//! #[serde(deny_unknown_fields)]
//! struct CronArgs { expression: String }
//!
//! #[derive(Serialize, Deserialize, JsonSchema)]
//! struct CronTick { expression: String, scheduled_time: String }
//!
//! /// expression → observers
//! #[derive(Clone, Default)]
//! struct Schedules(Arc<Mutex<HashMap<String, Vec<Observer>>>>);
//!
//! #[async_trait::async_trait]
//! impl ObserveHandler for Schedules {
//!     async fn on_observer_added(&self, obs: &Observer) -> Result<(), ObserveRejection> {
//!         let CronArgs { expression } = obs.args()?; // validated by the engine
//!         self.0.lock().unwrap().entry(expression).or_default().push(obs.clone());
//!         Ok(())
//!     }
//!     async fn on_observer_removed(&self, obs: &Observer) {
//!         for list in self.0.lock().unwrap().values_mut() {
//!             list.retain(|o| o.id() != obs.id());
//!         }
//!     }
//! }
//!
//! # fn demo(iii: &IIIClient) {
//! let schedules = Schedules::default();
//! let on_tick = iii.register_function(
//!     "scheduler::on-tick",
//!     RegisterFunction::new({
//!         let schedules = schedules.clone();
//!         move |tick: CronTick| {
//!             let observers = schedules.0.lock().unwrap().get(&tick.expression).cloned();
//!             for obs in observers.unwrap_or_default() {
//!                 obs.emit(&tick)?; // the observer's filters are checked here
//!             }
//!             Ok::<_, Error>(())
//!         }
//!     })
//!     .observe(Observe::owner().args_format::<CronArgs>().handler(schedules)),
//! );
//! assert!(!on_tick.is_observed());
//! # }
//! ```

use std::{
    collections::{BTreeMap, HashSet},
    fmt,
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    time::Duration,
};

use arc_swap::{ArcSwap, ArcSwapOption};
use futures_util::FutureExt;
use iii_filter::{Doc, Filter};
use schemars::JsonSchema;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, value::to_raw_value};
use tokio::sync::mpsc;

use crate::{
    context::with_current,
    error::Error,
    iii::{IIIClient, IIIInner, RemoteCall},
    protocol::{
        Activation, Caller, Capture, Delivery, ErrorBody, InvocationStatus, LegacyTrigger, Message,
        ObserveDescriptor, ObserverView, On, observe_codes,
    },
    reporter::{CaptureMask, PendingItem},
};

/// How long `on_observer_added` may run before the observer is rejected with
/// `OWNER_TIMEOUT` (the engine enforces the same timeout).
pub(crate) const HOOK_TIMEOUT: Duration = Duration::from_secs(30);

/// The observe descriptor of a function, for
/// [`RegisterFunction::observe`](crate::RegisterFunction::observe).
///
/// - [`Observe::auto`]: the function's invocations are the events; every
///   observer is offered every invocation and the SDK reports the matching
///   ones. It takes no args.
/// - [`Observe::owner`]: the function's owner reads each observer's args, sets
///   up what they ask for and hands each event to the observers it belongs to
///   with [`Observer::emit`] or [`Observer::call`].
///
/// A function registered without one is still observable, as `auto` without
/// hooks.
#[derive(Clone)]
pub struct Observe {
    pub(crate) descriptor: ObserveDescriptor,
    pub(crate) handler: Option<Arc<dyn ObserveHandler>>,
}

impl fmt::Debug for Observe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Observe")
            .field("descriptor", &self.descriptor)
            .field("handler", &self.handler.is_some())
            .finish()
    }
}

impl Observe {
    fn with_delivery(delivery: Delivery) -> Self {
        Self {
            descriptor: ObserveDescriptor {
                delivery: Some(delivery),
                ..ObserveDescriptor::default()
            },
            handler: None,
        }
    }

    /// Owner delivery: this worker routes events to observers by their args.
    pub fn owner() -> Self {
        Self::with_delivery(Delivery::Owner)
    }

    /// Auto delivery (the default): every invocation is an event.
    pub fn auto() -> Self {
        Self::with_delivery(Delivery::Auto)
    }

    /// The JSON Schema of the observers' args, generated from `T`. The engine
    /// validates every `observe` against it (`INVALID_ARGS`) before
    /// [`ObserveHandler::on_observer_added`] runs. `#[serde(deny_unknown_fields)]`
    /// on `T` rejects argument typos.
    ///
    /// Owner delivery only: registering an auto function with an
    /// `args_format` panics with `INVALID_OBSERVE_DESCRIPTOR`.
    pub fn args_format<T: JsonSchema>(self) -> Self {
        let schema = crate::iii::json_schema_for::<T>();
        self.args_format_value(schema.unwrap_or(Value::Null))
    }

    /// [`args_format`](Self::args_format) from a JSON Schema value.
    pub fn args_format_value(mut self, schema: Value) -> Self {
        self.descriptor.args_format = Some(schema);
        self
    }

    /// Accept the old `register_trigger({ trigger_type })` for this trigger
    /// type: when no live provider of it exists, the engine turns such a
    /// trigger into an observer of this function, with
    /// [`Observer::legacy`] set.
    pub fn legacy_trigger_type(mut self, trigger_type: impl Into<String>) -> Self {
        self.descriptor.legacy_trigger_type = Some(trigger_type.into());
        self
    }

    /// `false` refuses every observer (`NOT_OBSERVABLE`). Default `true`.
    pub fn allow(mut self, allow: bool) -> Self {
        self.descriptor.allow = Some(allow);
        self
    }

    /// The callbacks that run when an observer is added to or removed from
    /// this function.
    pub fn handler<H: ObserveHandler>(mut self, handler: H) -> Self {
        self.handler = Some(Arc::new(handler));
        self
    }

    pub(crate) fn delivery(&self) -> Delivery {
        self.descriptor.delivery.clone().unwrap_or_default()
    }
}

/// Callbacks of an observable function, run once per observer added or
/// removed, one at a time per function and in the engine's order.
///
/// `on_observer_added` runs before the SDK acknowledges the observer, so what
/// it puts in the owner's index is in place before the engine marks the
/// observer `active`. Returning an error rejects the observer: an
/// [`ObserveRejection`] sets the code, a plain error is `OBSERVE_REJECTED`,
/// and a callback still running after 30 s is `OWNER_TIMEOUT`.
///
/// A callback past the timeout is cancelled (its future is dropped at its
/// next `.await`) so the function's next callback can run; anything it
/// spawned keeps running and may overlap that next callback. Callbacks run on
/// the connection's runtime: do not block in them.
// async_trait injects #[must_use] on boxed Future-returning trait methods.
#[allow(
    clippy::double_must_use,
    reason = "async_trait generates redundant must_use on boxed futures"
)]
pub trait ObserveHandler: Send + Sync + 'static {
    /// An observer was attached to the function. Index it by its args here.
    fn on_observer_added<'a>(
        &'a self,
        obs: &Observer,
    ) -> Pin<Box<dyn Future<Output = Result<(), ObserveRejection>> + Send + 'a>> {
        let _ = obs;
        Box::pin(async { Ok(()) })
    }

    /// An observer was detached (unobserve, disposal, its worker left for
    /// good, a reconnect that no longer lists it). Cannot fail.
    fn on_observer_removed<'a>(
        &'a self,
        obs: &Observer,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        let _ = obs;
        Box::pin(async {})
    }
}

impl<T: ObserveHandler + ?Sized> ObserveHandler for Arc<T> {
    fn on_observer_added<'a>(
        &'a self,
        obs: &Observer,
    ) -> Pin<Box<dyn Future<Output = Result<(), ObserveRejection>> + Send + 'a>> {
        let obs = obs.clone();

        Box::pin(async move { (**self).on_observer_added(&obs).await })
    }

    fn on_observer_removed<'a>(
        &'a self,
        obs: &Observer,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        let obs = obs.clone();

        Box::pin(async move { (**self).on_observer_removed(&obs).await })
    }
}

/// An owner's refusal of an observer, with the code its `ready()` fails with.
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

/// One observer of a function this worker hosts, as its owner sees it: its
/// id, args, filters, target, capture and `on`. Never its metadata, policy or
/// observing worker.
///
/// Cheap to clone (an `Arc`), so owners keep it in their own index. Two
/// `Observer`s are equal when their ids are.
#[derive(Clone)]
pub struct Observer {
    inner: Arc<ObserverInner>,
}

struct ObserverInner {
    view: ObserverView,
    filter: Filter,
    mask: CaptureMask,
    function_id: String,
    namespace: String,
    delivery: Delivery,
    /// The revision the engine last announced it in (`observeradded`, or a
    /// `functionregistered` that listed it): what an ack must echo, or the
    /// engine ignores it as stale.
    revision: AtomicU64,
    /// In the function's compiled set: acked and neither removed nor rejected.
    active: AtomicBool,
    /// Set by [`Observer::reject`]: re-sent whenever the engine lists the
    /// observer again, until it is removed.
    rejection: Mutex<Option<ErrorBody>>,
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

/// Who an event (emit) or an owner call is from, by the rules of sdk.md §1.
pub(crate) struct EventOrigin {
    /// The caller filters see: inside the observed function's own handler,
    /// that invocation's caller; inside another handler, that handler; outside
    /// any handler, this worker.
    pub caller: Caller,
    /// Inside the function's own handler: that invocation's id.
    pub invocation_id: Option<String>,
    pub observe_depth: u32,
    pub metadata: Option<Value>,
    pub traceparent: Option<String>,
    pub baggage: Option<String>,
}

impl EventOrigin {
    pub(crate) fn current(function_id: &str, iii: &IIIClient) -> Self {
        with_current(|current| match current {
            Some(cur) if cur.function_id == function_id => EventOrigin {
                caller: cur.caller.clone().unwrap_or_else(|| iii.self_caller()),
                invocation_id: Some(cur.invocation_id.clone()),
                observe_depth: cur.observe_depth,
                metadata: cur.metadata.clone(),
                traceparent: cur.traceparent.clone(),
                baggage: cur.baggage.clone(),
            },
            Some(cur) => EventOrigin {
                caller: Caller {
                    function_id: Some(cur.function_id.clone()),
                    invocation_id: Some(cur.invocation_id.clone()),
                    parent: cur.caller.as_ref().and_then(crate::iii::parent_of),
                    ..iii.self_caller()
                },
                invocation_id: None,
                observe_depth: cur.observe_depth,
                metadata: cur.metadata.clone(),
                traceparent: cur.traceparent.clone(),
                baggage: cur.baggage.clone(),
            },
            None => {
                let (traceparent, baggage) = crate::iii::inject_trace_headers();
                EventOrigin {
                    caller: iii.self_caller(),
                    invocation_id: None,
                    observe_depth: 0,
                    metadata: None,
                    traceparent,
                    baggage,
                }
            }
        })
    }
}

impl Observer {
    fn new(
        view: ObserverView,
        filter: Filter,
        entry: &FunctionEntry,
        namespace: String,
        revision: u64,
        client: Weak<IIIInner>,
    ) -> Self {
        let mask = capture_mask(&view.capture);
        Self {
            inner: Arc::new(ObserverInner {
                view,
                filter,
                mask,
                function_id: entry.id.clone(),
                namespace,
                delivery: entry.delivery.clone(),
                revision: AtomicU64::new(revision),
                active: AtomicBool::new(false),
                rejection: Mutex::new(None),
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

    /// The observer's filters in normal form (`{}` matches every event).
    /// Applied by the SDK on every [`emit`](Self::emit) and
    /// [`call`](Self::call); the owner never changes them.
    pub fn filters(&self) -> &Value {
        &self.inner.view.filters
    }

    /// The function each activation invokes.
    pub fn target(&self) -> &str {
        &self.inner.view.target
    }

    /// What activations ship to the target.
    pub fn capture(&self) -> &[Capture] {
        &self.inner.view.capture
    }

    /// Which outcomes activate it (auto functions only).
    pub fn on(&self) -> &On {
        &self.inner.view.on
    }

    /// The old trigger this observer stands for, when the engine's legacy shim
    /// created it from a `register_trigger`. Read `config` as its args.
    pub fn legacy(&self) -> Option<&LegacyTrigger> {
        self.inner.view.legacy.as_ref()
    }

    /// Whether it is still delivered to: acked, and neither removed nor
    /// rejected since.
    pub fn is_active(&self) -> bool {
        self.inner.active.load(Ordering::Acquire)
    }

    /// Evaluates the observer's filters against `doc`, exactly as given.
    /// Lets an owner answer before calling (the HTTP router's 422).
    pub fn matches(&self, doc: &Doc<'_>) -> bool {
        self.inner.filter.matches(doc)
    }

    fn owner_delivered(&self) -> Result<(), Error> {
        if self.inner.delivery == Delivery::Owner {
            return Ok(());
        }
        Err(coded_error(
            observe_codes::NOT_OWNER_DELIVERED,
            format!(
                "{} is not owner-delivered: its invocations are its events",
                self.inner.function_id
            ),
        ))
    }

    fn client(&self) -> Option<IIIClient> {
        self.inner.client.upgrade().map(IIIClient::from_inner)
    }

    /// Hands `event` to this observer, fire-and-forget. Owner functions only
    /// (`NOT_OWNER_DELIVERED` otherwise).
    ///
    /// The observer's filters see `{ input: event, caller, metadata }`, where
    /// `caller` is the invocation this runs in: inside the observed function's
    /// own handler, that invocation's caller; inside another handler, that
    /// handler; outside any handler, this worker. Returns `Ok(false)` and
    /// sends nothing when they reject it, or when the observer is no longer
    /// active (removed or rejected meanwhile: an owner's fan-out loop goes
    /// on). Otherwise the event joins the next batched report and the engine
    /// activates the target with the [`Observed`](crate::Observed) envelope,
    /// in the trace of the invocation it was emitted from.
    ///
    /// Delivery is at-most-once. Reports wait in a bounded buffer while the
    /// connection is down and go out once the engine confirms the function
    /// again; a report frame whose write failed when the connection dropped
    /// is not resent (it may have arrived) and counts as dropped. Every drop
    /// is counted and reported in `localinvocationstats`.
    pub fn emit<T: Serialize + ?Sized>(&self, event: &T) -> Result<bool, Error> {
        self.owner_delivered()?;
        if !self.is_active() {
            return Ok(false);
        }
        let Some(iii) = self.client() else {
            return Ok(false);
        };
        let input = serde_json::to_value(event)?;
        let origin = EventOrigin::current(&self.inner.function_id, &iii);
        if !self.filter_accepts(&input, &origin) {
            return Ok(false);
        }

        let item = PendingItem {
            function_id: self.inner.function_id.clone(),
            namespace: Some(self.inner.namespace.clone()),
            invocation_id: origin
                .invocation_id
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            matched: vec![self.id().to_string()],
            status: InvocationStatus::Success,
            input: Some(to_raw_value(&input)?),
            output: None,
            error: None,
            caller: Some(origin.caller),
            metadata: origin.metadata.as_ref().map(to_raw_value).transpose()?,
            started_at_ms: crate::iii::now_ms(),
            duration_ms: 0.0,
            observe_depth: origin.observe_depth,
            traceparent: origin.traceparent,
            baggage: origin.baggage,
            mask: CaptureMask {
                output: false,
                error: false,
                ..self.inner.mask
            },
            emitted: true,
            seq: 0,
        };
        iii.reporter().push(item);
        Ok(true)
    }

    fn filter_accepts(&self, input: &Value, origin: &EventOrigin) -> bool {
        let filter = &self.inner.filter;
        if filter.is_match_all() {
            return true;
        }
        let caller = if filter.roots().caller {
            serde_json::to_value(&origin.caller).ok()
        } else {
            None
        };
        filter.matches(&Doc {
            input: Some(input),
            caller: caller.as_ref(),
            metadata: origin.metadata.as_ref(),
            ..Doc::default()
        })
    }

    /// Calls the observer's target with `payload` through the engine and
    /// returns its result: the HTTP response, the queue ack. Owner functions
    /// only. Fails with `FILTERS_NOT_MATCHED`, sending nothing, when the
    /// observer's filters reject `{ input: payload, caller, metadata }`;
    /// with `OBSERVER_NOT_ACTIVE` once it was removed; with the engine's code
    /// (`LOOP_GUARD`, `OVERFLOW`, ...) or the target's error otherwise.
    pub async fn call<R: DeserializeOwned>(&self, payload: impl Serialize) -> Result<R, Error> {
        self.call_with_timeout(
            payload,
            Duration::from_millis(crate::iii::DEFAULT_TIMEOUT_MS),
        )
        .await
    }

    /// [`call`](Self::call) with its own timeout.
    pub async fn call_with_timeout<R: DeserializeOwned>(
        &self,
        payload: impl Serialize,
        timeout: Duration,
    ) -> Result<R, Error> {
        self.owner_delivered()?;
        if !self.is_active() {
            return Err(coded_error(
                observe_codes::OBSERVER_NOT_ACTIVE,
                format!("observer {} is not active", self.id()),
            ));
        }
        let iii = self.client().ok_or(Error::NotConnected)?;
        let input = serde_json::to_value(payload)?;
        let origin = EventOrigin::current(&self.inner.function_id, &iii);
        if !self.filter_accepts(&input, &origin) {
            return Err(coded_error(
                observe_codes::FILTERS_NOT_MATCHED,
                format!("observer {}'s filters reject this payload", self.id()),
            ));
        }
        let (caller, observe_depth) = iii.outgoing_caller();
        let result = iii
            .invoke_remote(RemoteCall {
                function_id: self.inner.view.target.clone(),
                payload: input,
                action: None,
                timeout: Some(timeout),
                metadata: None,
                // The engine routes an owner delivery to the observer's own
                // target, whatever namespace a request names.
                namespace: None,
                caller,
                observe_depth,
                activation: Some(Activation {
                    observer_id: self.id().to_string(),
                    activation_id: None,
                }),
            })
            .await?;
        Ok(serde_json::from_value(result)?)
    }

    /// Rejects this observer after it was accepted (a config reload removed
    /// what it needs). It stops receiving events now; the engine moves it to
    /// `error` with this code and then removes it, which runs
    /// [`ObserveHandler::on_observer_removed`]. The rejection holds across a
    /// reconnect: the SDK sends it again whenever the engine lists the
    /// observer, until a fresh `observeradded` brings it back.
    pub fn reject(&self, code: impl Into<String>, message: impl Into<String>) {
        let error = ErrorBody::new(code, message);
        // Held to the end: ordered against an `on_observer_added` finishing.
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
        let Some(iii) = self.client() else {
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

    pub(crate) fn mask(&self) -> CaptureMask {
        self.inner.mask
    }

    pub(crate) fn filter(&self) -> &Filter {
        &self.inner.filter
    }

    pub(crate) fn view(&self) -> &ObserverView {
        &self.inner.view
    }

    /// Whether an auto invocation that ended with `status` activates it.
    pub(crate) fn accepts(&self, success: bool) -> bool {
        match self.inner.view.on {
            On::Success => success,
            On::Error => !success,
            On::Complete => true,
            On::Unknown(_) => false,
        }
    }
}

/// What a local call needs from the documents of its observers.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Needs {
    pub input: bool,
    pub caller: bool,
}

/// The active observers of a function, swapped whole on every change so the
/// hot path reads it with one load.
#[derive(Default)]
pub(crate) struct CompiledSet {
    pub observers: Vec<Observer>,
    pub needs: Needs,
}

impl CompiledSet {
    fn new(observers: Vec<Observer>) -> Self {
        let mut needs = Needs::default();
        for obs in &observers {
            let roots = obs.filter().roots();
            needs.input |= roots.input || obs.mask().input;
            needs.caller |= roots.caller || obs.mask().caller;
        }
        Self { observers, needs }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.observers.is_empty()
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
    pub(crate) delivery: Delivery,
    hooks: Option<Arc<dyn ObserveHandler>>,
    /// Hooks or owner delivery: inactive against an engine without observe.
    pub(crate) needs_observe: bool,
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
    /// Another worker owns the function since a conflict, until the engine
    /// confirms it again.
    revoked: AtomicBool,
    compiled: ArcSwap<CompiledSet>,
    observed: AtomicUsize,
    /// Serializes the rebuilds of `compiled` (the hook task's, `reject`'s).
    publishing: Mutex<()>,
    pub(crate) stats: LocalStats,
    ops: Mutex<Option<mpsc::UnboundedSender<HookOp>>>,
}

impl FunctionEntry {
    pub(crate) fn new(
        id: String,
        observe: Option<&Observe>,
        has_handler: bool,
        local_option: bool,
    ) -> Self {
        let delivery = observe.map(Observe::delivery).unwrap_or_default();
        let hooks = observe.and_then(|o| o.handler.clone());
        Self {
            needs_observe: delivery == Delivery::Owner || hooks.is_some(),
            id,
            delivery,
            hooks,
            wants_local: has_handler && local_option,
            namespace: ArcSwapOption::empty(),
            local_dispatch: AtomicBool::new(false),
            confirmed_epoch: AtomicU64::new(0),
            revocation: AtomicU64::new(0),
            revoked: AtomicBool::new(false),
            compiled: ArcSwap::from_pointee(CompiledSet::default()),
            observed: AtomicUsize::new(0),
            publishing: Mutex::new(()),
            stats: LocalStats::default(),
            ops: Mutex::new(None),
        }
    }

    /// One atomic load.
    #[inline]
    pub(crate) fn is_observed(&self) -> bool {
        self.observed.load(Ordering::Acquire) != 0
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

    /// The function is not this worker's any more: another worker owns it
    /// (`FUNCTION_NAMESPACE_CONFLICT`) or it was unregistered. Ordered
    /// against `functionregistered` messages already queued, and against
    /// `on_observer_added` callbacks still running.
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
        let count = observers.len();
        self.compiled.store(Arc::new(CompiledSet::new(observers)));
        self.observed.store(count, Ordering::Release);
    }

    fn publish(&self, members: &BTreeMap<String, Observer>) {
        let _serialized = self.publishing.lock().unwrap_or_else(|e| e.into_inner());
        let observers: Vec<Observer> = members
            .values()
            .filter(|o| o.is_active())
            .cloned()
            .collect();
        let count = observers.len();
        self.compiled.store(Arc::new(CompiledSet::new(observers)));
        self.observed.store(count, Ordering::Release);
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
        if matches!(op, HookOp::Reset { .. } | HookOp::Republish) {
            // Nothing was ever added: there is nothing to reset.
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
    /// An observer was rejected late: rebuild the compiled set.
    Republish,
    /// Drop the whole set: a `FUNCTION_NAMESPACE_CONFLICT`, or the function
    /// was unregistered (`last`).
    Reset {
        last: bool,
    },
}

async fn run_hooks(
    entry: Arc<FunctionEntry>,
    client: Weak<IIIInner>,
    mut ops: mpsc::UnboundedReceiver<HookOp>,
) {
    let mut members: BTreeMap<String, Observer> = BTreeMap::new();
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
                        held.ack_again(&iii, &entry, &namespace, rev, |rejected| {
                            rejected || held.view() == &view
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
                    remove_member(&entry, &mut members, &id).await;
                }
                for view in fresh {
                    // a changed view: removed, then added as a new observer
                    if members.contains_key(&view.id) {
                        remove_member(&entry, &mut members, &view.id).await;
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
                    held.ack_again(&iii, &entry, &namespace, rev, |rejected| {
                        !rejected && held.view() == &observer
                    })
                });
                if !acked {
                    // New, changed, or rejected earlier: a fresh observer.
                    if members.contains_key(&observer.id) {
                        remove_member(&entry, &mut members, &observer.id).await;
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
                remove_member(&entry, &mut members, &id).await;
                revision = rev;
            }
            HookOp::Republish => entry.publish(&members),
            HookOp::Reset { last } => {
                entry.revoke();
                let ids: Vec<String> = members.keys().cloned().collect();
                for id in ids {
                    remove_member(&entry, &mut members, &id).await;
                }
                revision = 0;
                if last {
                    return;
                }
            }
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

/// Runs `on_observer_added`, puts the observer in the compiled set, then acks
/// it: the first event after the engine marks it active is evaluated.
async fn add_member(
    iii: &IIIClient,
    entry: &Arc<FunctionEntry>,
    members: &mut BTreeMap<String, Observer>,
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
        entry,
        namespace.to_string(),
        revision,
        Arc::downgrade(&iii.inner),
    );
    if let Some(hooks) = &entry.hooks {
        let revocation = entry.revocation();
        let added = std::panic::AssertUnwindSafe(hooks.on_observer_added(&observer)).catch_unwind();
        let rejection = match tokio::time::timeout(iii.hook_timeout(), added).await {
            Ok(Ok(Ok(()))) => None,
            Ok(Ok(Err(rejection))) => Some(rejection),
            Ok(Err(panic)) => Some(ObserveRejection::new(
                observe_codes::OBSERVE_REJECTED,
                format!("on_observer_added panicked: {}", panic_message(&panic)),
            )),
            Err(_) => Some(ObserveRejection::new(
                observe_codes::OWNER_TIMEOUT,
                format!(
                    "on_observer_added for observer {id} did not finish within {:?}",
                    iii.hook_timeout()
                ),
            )),
        };
        if let Some(rejection) = rejection {
            tracing::debug!(function_id = %entry.id, observer_id = %id, code = %rejection.code, "observer rejected by its owner");
            let body = ErrorBody::new(rejection.code, rejection.message);
            return send_ack(iii, entry, namespace, &id, revision, Some(body));
        }
        if entry.revocation() != revocation {
            // The function was released while the callback ran: undo the add
            // (no ack, never active), as if it had been removed.
            tracing::debug!(function_id = %entry.id, observer_id = %id, "function released during on_observer_added");
            run_removed_hook(entry, hooks, &observer).await;
            return;
        }
    }
    // A `reject()` made while the hook ran wins. Holding the rejection lock
    // through the ack keeps a concurrent `reject()` strictly after it (a late
    // rejection the engine handles), never between activation and ack.
    let rejection = observer
        .inner
        .rejection
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(error) = rejection.clone() {
        drop(rejection);
        tracing::debug!(function_id = %entry.id, observer_id = %id, code = %error.code, "observer rejected during on_observer_added");
        return send_ack(iii, entry, namespace, &id, revision, Some(error));
    }
    // Ack, then publish, in one synchronous step: the ack is queued ahead of
    // any report the new observer matches, and the connection loop (same
    // thread) only sends it after this task yields, by when the observer is
    // compiled in.
    observer.inner.active.store(true, Ordering::Release);
    send_ack(iii, entry, namespace, &id, revision, None);
    members.insert(id.clone(), observer.clone());
    entry.publish(members);
    drop(rejection);
}

async fn remove_member(
    entry: &Arc<FunctionEntry>,
    members: &mut BTreeMap<String, Observer>,
    id: &str,
) {
    let Some(observer) = members.remove(id) else {
        return;
    };
    observer.inner.active.store(false, Ordering::Release);
    entry.publish(members);
    if let Some(hooks) = &entry.hooks {
        run_removed_hook(entry, hooks, &observer).await;
    }
}

async fn run_removed_hook(
    entry: &FunctionEntry,
    hooks: &Arc<dyn ObserveHandler>,
    observer: &Observer,
) {
    let removed = std::panic::AssertUnwindSafe(hooks.on_observer_removed(observer)).catch_unwind();
    match tokio::time::timeout(HOOK_TIMEOUT, removed).await {
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
            "on_observer_removed did not finish within {HOOK_TIMEOUT:?}"
        ),
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
        RegisterFunction,
        iii::{IIIConnectionState, MutexExt, Outbound},
        protocol::TriggerRequest,
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

    struct Hooks;

    #[async_trait]
    impl ObserveHandler for Hooks {
        async fn on_observer_added(&self, obs: &Observer) -> Result<(), ObserveRejection> {
            match obs.id() {
                "hangs" => {
                    std::future::pending::<()>().await;
                    Ok(())
                }
                "panics" => panic!("hook blew up"),
                _ => Ok(()),
            }
        }
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
        iii.register_function(
            "h::f",
            RegisterFunction::new(|v: Value| Ok::<_, Error>(v))
                .observe(Observe::owner().handler(Hooks)),
        );
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

    #[tokio::test]
    #[tracing_test::traced_test]
    async fn an_engine_without_observe_is_logged_once_for_functions_with_hooks() {
        let (iii, _rx) = client();
        iii.register_function(
            "o::hooked",
            RegisterFunction::new(|v: Value| Ok::<_, Error>(v)).observe(Observe::owner()),
        );
        iii.register_function(
            "o::plain",
            RegisterFunction::new(|v: Value| Ok::<_, Error>(v)),
        );
        let hello = json!({ "type": "workerregistered", "worker_id": "w" });
        feed(&iii, hello.clone());
        feed(&iii, hello.clone());
        // registered once the engine said it has no observe: warned too
        iii.register_function(
            "o::late",
            RegisterFunction::new(|v: Value| Ok::<_, Error>(v)).observe(Observe::owner()),
        );
        iii.register_function(
            "o::late-plain",
            RegisterFunction::new(|v: Value| Ok::<_, Error>(v)),
        );
        let warned = |lines: &[&str]| {
            let hits: Vec<&&str> = lines
                .iter()
                .filter(|l| l.contains("does not support observe"))
                .collect();
            match hits.as_slice() {
                [first, late]
                    if first.contains("o::hooked")
                        && !first.contains("o::plain")
                        && late.contains("o::late")
                        && !late.contains("o::late-plain")
                        && !late.contains("o::hooked") =>
                {
                    Ok(())
                }
                other => Err(format!(
                    "expected a line naming o::hooked, then one naming o::late, got {other:?}"
                )),
            }
        };
        logs_assert(warned);
        // a reconnect to the same engine says nothing new
        feed(&iii, hello);
        logs_assert(warned);
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
            iii.trigger(TriggerRequest::new("b::f", payload.clone()))
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

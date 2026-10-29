//! Observer hooks: what the worker that hosts a function does when it is
//! observed.
//!
//! Every function is observable, and iii delivers each of its calls to the
//! observers whose `on` and filters match. Four hooks override that default:
//!
//! | Hook | The question it answers | What iii does by default |
//! |---|---|---|
//! | [`AddedHook`] | What do I set up for this observer (a database trigger, a cron job, a route)? Return an [`ObserveRejection`] to reject it. | accepts the observer |
//! | [`RemovedHook`] | What do I tear down? | forgets the observer |
//! | [`Match`] | Does this observer want this call? | every observer wants every call |
//! | [`DeliverHook`] | I deliver this call myself: which observers, and what does the caller get back? | delivers the call to every matching observer whose filters match it, and gives the caller the handler's return value |
//!
//! The added and removed hooks can be set for one function and for every
//! function the worker hosts, and both levels run: when an observer is
//! added, the worker's hook and then the function's; when it is removed, the
//! function's and then the worker's. The match and deliver hooks are set per
//! function; with both set, the deliver hook receives the observers that
//! matched. Async closures implement the hook traits, and a worker with
//! shared state implements them on its own type.
//!
//! ```rust,no_run
//! use iii_sdk::{Error, IIIClient, Match, ObserveRejection, Observer, RegisterFunction};
//! use schemars::JsonSchema;
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Deserialize, JsonSchema)]
//! #[serde(deny_unknown_fields)]
//! struct CronArgs { expression: String }
//!
//! #[derive(Serialize, Deserialize, JsonSchema)]
//! struct CronTick { expression: String, scheduled_time: String }
//!
//! # fn demo(iii: &IIIClient) {
//! // by id, before registering: the hooks see every observer
//! iii.on_observer_added_for("cron::on-tick", |obs: Observer| async move {
//!     let CronArgs { expression } = obs.args()?;
//!     if expression.split_whitespace().count() != 7 {
//!         return Err(ObserveRejection::new("INVALID_CRON_EXPRESSION", expression));
//!     }
//!     Ok(()) // start the expression's job here
//! });
//! // an observer of expression X wants the ticks of expression X
//! iii.on_observer_match_for(
//!     "cron::on-tick",
//!     Match::keys(
//!         |obs| obs.args_value()["expression"].as_str().into(),
//!         |call| call.input["expression"].as_str().into(),
//!     ),
//! );
//!
//! // the handler returns the tick it is called with: observers observe it
//! iii.register_function(
//!     "cron::on-tick",
//!     RegisterFunction::new(|tick: CronTick| Ok::<_, Error>(tick))
//!         .args_format::<CronArgs>()
//!         .private(),
//! );
//! # }
//! ```

use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    future::Future,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, Ordering},
    },
};

use async_trait::async_trait;
use serde_json::Value;

use crate::{
    error::Error,
    iii::{IIIClient, MutexExt},
    observable::{CatchUp, FunctionEntry, HookOp, ObserveRejection, Observer},
    observe::ObservedError,
    protocol::{Caller, InvocationStatus},
};

/// Runs when an observer is added to a function this worker hosts, before
/// the SDK accepts it: what it sets up is in place before the observer's
/// first call. Returning an error rejects the observer: an
/// [`ObserveRejection`] sets the code, and a hook still running after 30 s is
/// `OWNER_TIMEOUT`.
///
/// Implemented for async closures taking the [`Observer`] by value.
// async_trait injects #[must_use] on boxed Future-returning trait methods.
#[allow(
    clippy::double_must_use,
    reason = "async_trait generates redundant must_use on boxed futures"
)]
#[async_trait]
pub trait AddedHook: Send + Sync + 'static {
    async fn on_added(&self, obs: Observer) -> Result<(), ObserveRejection>;
}

/// Runs once for every accepted observer when it is removed (unobserved,
/// disposed, rejected late), to tear down what [`AddedHook`] set up. It never
/// runs for an observer that was not accepted. A panic is logged; the
/// observer is removed either way.
///
/// Implemented for async closures taking the [`Observer`] by value.
#[allow(
    clippy::double_must_use,
    reason = "async_trait generates redundant must_use on boxed futures"
)]
#[async_trait]
pub trait RemovedHook: Send + Sync + 'static {
    async fn on_removed(&self, obs: Observer);
}

/// Delivers each call of one function itself, in place of iii's delivery:
/// what it returns (or fails with) is the call's result, and iii delivers
/// nothing for the function. For a function whose observers' answers matter
/// (an HTTP route's response, a queue subscriber's ack) or whose calls each go
/// to one observer, called through with
/// [`CallRequest::observer`](crate::CallRequest::observer).
///
/// It runs after the handler, inside the function's invocation context and
/// span, so calls it makes have the function's invocation as their caller.
/// It receives the call and the observers that matched it ([`Match`], or all
/// of them), a snapshot taken when the call started. It applies their filters
/// with [`Observer::matches`].
///
/// Implemented for async closures taking `(Call, Vec<Observer>)`.
#[allow(
    clippy::double_must_use,
    reason = "async_trait generates redundant must_use on boxed futures"
)]
#[async_trait]
pub trait DeliverHook: Send + Sync + 'static {
    async fn on_deliver(&self, call: Call, observers: Vec<Observer>) -> Result<Value, Error>;
}

#[async_trait]
impl<F, Fut> AddedHook for F
where
    F: Fn(Observer) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<(), ObserveRejection>> + Send + 'static,
{
    async fn on_added(&self, obs: Observer) -> Result<(), ObserveRejection> {
        self(obs).await
    }
}

#[async_trait]
impl<F, Fut> RemovedHook for F
where
    F: Fn(Observer) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    async fn on_removed(&self, obs: Observer) {
        self(obs).await
    }
}

#[async_trait]
impl<F, Fut> DeliverHook for F
where
    F: Fn(Call, Vec<Observer>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Value, Error>> + Send + 'static,
{
    async fn on_deliver(&self, call: Call, observers: Vec<Observer>) -> Result<Value, Error> {
        self(call, observers).await
    }
}

/// One call of a function this worker hosts, as its observers observe it.
///
/// `output` is the handler's return value: what observers observe. A source
/// that asks [`FunctionRef::is_observed_for`](crate::runtime::FunctionRef::is_observed_for)
/// before calling fills in only what its [`Match::keys`] read:
///
/// ```rust
/// # use iii_sdk::Call;
/// # use serde_json::json;
/// let call = Call { input: json!({ "expression": "0 * * * * * *" }), ..Call::default() };
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    pub function_id: String,
    pub invocation_id: String,
    pub status: InvocationStatus,
    pub input: Value,
    /// The handler's return value; `None` when it failed.
    pub output: Option<Value>,
    /// What the handler failed with.
    pub error: Option<ObservedError>,
    /// Who made the call; `None` for a call made outside any handler of a
    /// worker, or from an engine older than 0.25.
    pub caller: Option<Caller>,
    /// The call's metadata sidecar.
    pub metadata: Option<Value>,
}

impl Default for Call {
    /// A successful call with `null` input and nothing else set.
    fn default() -> Self {
        Self {
            function_id: String::new(),
            invocation_id: String::new(),
            status: InvocationStatus::Success,
            input: Value::Null,
            output: None,
            error: None,
            caller: None,
            metadata: None,
        }
    }
}

/// Whether `a` and `b` serialize to the same bytes: the same structure, and
/// numbers with the same representation (`0.0` and `-0.0` differ).
pub(crate) fn same_json(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => match (x.as_f64(), y.as_f64()) {
            (Some(fx), Some(fy)) if x.is_f64() && y.is_f64() => fx.to_bits() == fy.to_bits(),
            _ => x == y,
        },
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(a, b)| same_json(a, b))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .zip(y)
                    .all(|((ka, va), (kb, vb))| ka == kb && same_json(va, vb))
        }
        _ => a == b,
    }
}

/// The keys an observer is filed under, or a call matches, for
/// [`Match::keys`]: none, one or several. Converts from `&str`, `String`,
/// their `Option`s and `Vec`s. Keys borrowed from the observer or the call
/// cost no allocation.
#[derive(Debug, Clone, Default)]
pub struct Keys<'a>(KeysInner<'a>);

#[derive(Debug, Clone, Default)]
enum KeysInner<'a> {
    #[default]
    None,
    One(Cow<'a, str>),
    Many(Vec<Cow<'a, str>>),
}

impl<'a> Keys<'a> {
    /// No key: an observer filed nowhere, a call that matches nobody.
    pub fn none() -> Self {
        Self(KeysInner::None)
    }

    fn for_each(&self, mut f: impl FnMut(&str) -> bool) -> bool {
        match &self.0 {
            KeysInner::None => false,
            KeysInner::One(key) => f(key),
            KeysInner::Many(keys) => keys.iter().any(|key| f(key)),
        }
    }
}

impl<'a> From<&'a str> for Keys<'a> {
    fn from(key: &'a str) -> Self {
        Self(KeysInner::One(Cow::Borrowed(key)))
    }
}

impl<'a> From<&'a String> for Keys<'a> {
    fn from(key: &'a String) -> Self {
        Self(KeysInner::One(Cow::Borrowed(key)))
    }
}

impl From<String> for Keys<'_> {
    fn from(key: String) -> Self {
        Self(KeysInner::One(Cow::Owned(key)))
    }
}

impl<'a> From<Cow<'a, str>> for Keys<'a> {
    fn from(key: Cow<'a, str>) -> Self {
        Self(KeysInner::One(key))
    }
}

impl<'a, K: Into<Keys<'a>>> From<Option<K>> for Keys<'a> {
    fn from(key: Option<K>) -> Self {
        key.map_or_else(Keys::none, Into::into)
    }
}

impl<'a> From<Vec<&'a str>> for Keys<'a> {
    fn from(keys: Vec<&'a str>) -> Self {
        Self(KeysInner::Many(
            keys.into_iter().map(Cow::Borrowed).collect(),
        ))
    }
}

impl From<Vec<String>> for Keys<'_> {
    fn from(keys: Vec<String>) -> Self {
        Self(KeysInner::Many(keys.into_iter().map(Cow::Owned).collect()))
    }
}

impl<'a> From<Vec<Cow<'a, str>>> for Keys<'a> {
    fn from(keys: Vec<Cow<'a, str>>) -> Self {
        Self(KeysInner::Many(keys))
    }
}

type Predicate = dyn Fn(&Observer, &Call) -> bool + Send + Sync;
type ObserverKeys = dyn for<'a> Fn(&'a Observer) -> Keys<'a> + Send + Sync;
type CallKeys = dyn for<'a> Fn(&'a Call) -> Keys<'a> + Send + Sync;

/// Which calls of a function each observer wants: the function's match hook
/// ([`IIIClient::on_observer_match_for`]). A worker sets it when its
/// observers' args say which calls they want; iii does everything else as
/// usual: the observers' filters, `on`, policy and `deliver` still apply.
#[derive(Clone)]
pub enum Match {
    /// Asked once per active observer per call, before its filters. A
    /// predicate that panics counts as no match, logged once per function.
    Predicate(Arc<Predicate>),
    /// An observer wants a call when they share a key. The SDK computes an
    /// observer's keys once, when it is accepted, and files it under each
    /// (an observer whose key function panics is rejected with
    /// `OBSERVE_REJECTED`); a call's keys once per call. A call matches the
    /// observers filed under any of its keys, each once. Keys express
    /// wildcards by listing them: an observer of every table filed under
    /// `"primary|*|insert"`, and a change matching `"primary|orders|insert"`,
    /// `"primary|orders|*"`, `"primary|*|insert"` and `"primary|*|*"`: four
    /// lookups, whatever the number of observers.
    Keys {
        observer: Arc<ObserverKeys>,
        call: Arc<CallKeys>,
    },
}

impl Match {
    /// Does `obs` want `call`?
    ///
    /// ```rust,no_run
    /// # use iii_sdk::{IIIClient, Match};
    /// # fn demo(iii: &IIIClient) {
    /// iii.on_observer_match_for(
    ///     "cron::on-tick",
    ///     Match::predicate(|obs, call| obs.args_value()["expression"] == call.input["expression"]),
    /// );
    /// # }
    /// ```
    pub fn predicate(f: impl Fn(&Observer, &Call) -> bool + Send + Sync + 'static) -> Self {
        Self::Predicate(Arc::new(f))
    }

    /// The keys an observer is filed under, and the keys a call matches.
    /// With them, [`FunctionRef::is_observed_for`](crate::runtime::FunctionRef::is_observed_for)
    /// answers with one hash lookup per call key.
    ///
    /// ```rust,no_run
    /// # use iii_sdk::{IIIClient, Keys, Match};
    /// # fn demo(iii: &IIIClient) {
    /// let key = |db: &str, table: &str, op: &str| format!("{db}|{table}|{op}");
    /// iii.on_observer_match_for(
    ///     "database::on-row-changed",
    ///     Match::keys(
    ///         move |obs| {
    ///             let args = obs.args_value();
    ///             let (db, table) = (args["db"].as_str().unwrap_or(""), args["table"].as_str().unwrap_or("*"));
    ///             match args["ops"].as_array() {
    ///                 Some(ops) => ops.iter().filter_map(|op| op.as_str()).map(|op| key(db, table, op)).collect::<Vec<_>>().into(),
    ///                 None => key(db, table, "*").into(),
    ///             }
    ///         },
    ///         move |call| {
    ///             let input = &call.input;
    ///             let (db, table, op) = (
    ///                 input["db"].as_str().unwrap_or(""),
    ///                 input["table"].as_str().unwrap_or(""),
    ///                 input["op"].as_str().unwrap_or(""),
    ///             );
    ///             Keys::from(vec![key(db, table, op), key(db, table, "*"), key(db, "*", op), key(db, "*", "*")])
    ///         },
    ///     ),
    /// );
    /// # }
    /// ```
    pub fn keys<O, C>(observer: O, call: C) -> Self
    where
        O: for<'a> Fn(&'a Observer) -> Keys<'a> + Send + Sync + 'static,
        C: for<'a> Fn(&'a Call) -> Keys<'a> + Send + Sync + 'static,
    {
        Self::Keys {
            observer: Arc::new(observer),
            call: Arc::new(call),
        }
    }
}

/// A function's [`Match`] at work: for keys, the index of its observers.
pub(crate) struct Matcher {
    rule: Match,
    index: RwLock<KeyIndex>,
    /// A predicate or a call key function panicked: logged once.
    warned: AtomicBool,
}

#[derive(Default)]
struct KeyIndex {
    by_key: HashMap<String, Vec<Observer>>,
    /// The keys each observer is filed under, by observer id.
    keys_of: HashMap<String, Vec<String>>,
}

/// An observer's keys could not be computed: its key function panicked.
pub(crate) struct KeysPanicked(pub String);

impl Matcher {
    pub(crate) fn new(rule: Match) -> Self {
        Self {
            rule,
            index: RwLock::new(KeyIndex::default()),
            warned: AtomicBool::new(false),
        }
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, KeyIndex> {
        self.index.read().unwrap_or_else(|e| e.into_inner())
    }

    fn write(&self) -> std::sync::RwLockWriteGuard<'_, KeyIndex> {
        self.index.write().unwrap_or_else(|e| e.into_inner())
    }

    /// Logs, once, that a match hook of `function_id` panicked.
    fn warn_once(&self, function_id: &str, what: &str, panic: Box<dyn std::any::Any + Send>) {
        if !self.warned.swap(true, Ordering::Relaxed) {
            tracing::warn!(
                function_id = %function_id,
                "on_observer_match {what} panicked; it matches nothing: {}",
                crate::observable::panic_message(&panic)
            );
        }
    }

    /// The keys `obs` is filed under, each once; `None` without keys.
    pub(crate) fn observer_keys(
        &self,
        obs: &Observer,
    ) -> Option<Result<Vec<String>, KeysPanicked>> {
        let Match::Keys { observer, .. } = &self.rule else {
            return None;
        };
        let keys = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut owned: Vec<String> = Vec::new();
            observer(obs).for_each(|key| {
                if !owned.iter().any(|k| k == key) {
                    owned.push(key.to_string());
                }
                false
            });
            owned
        }));
        Some(keys.map_err(|panic| KeysPanicked(crate::observable::panic_message(&panic))))
    }

    /// Files `obs` under `keys` (again, if it was filed already).
    pub(crate) fn file(&self, obs: &Observer, keys: Vec<String>) {
        let mut index = self.write();
        Self::unfile_locked(&mut index, obs.id());
        for key in &keys {
            index
                .by_key
                .entry(key.clone())
                .or_default()
                .push(obs.clone());
        }
        index.keys_of.insert(obs.id().to_string(), keys);
    }

    /// Takes the observer with id `id` out of the index.
    pub(crate) fn unfile(&self, id: &str) {
        Self::unfile_locked(&mut self.write(), id);
    }

    fn unfile_locked(index: &mut KeyIndex, id: &str) {
        let Some(keys) = index.keys_of.remove(id) else {
            return;
        };
        for key in keys {
            if let Some(list) = index.by_key.get_mut(&key) {
                list.retain(|obs| obs.id() != id);
                if list.is_empty() {
                    index.by_key.remove(&key);
                }
            }
        }
    }

    /// The keys of `call`; a panic matches nothing.
    fn keys_of_call<'a>(&self, call: &'a Call, keys: &CallKeys, function_id: &str) -> Keys<'a> {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| keys(call))).unwrap_or_else(
            |panic| {
                self.warn_once(function_id, "call key function", panic);
                Keys::none()
            },
        )
    }

    /// The observers of `function_id` among `active` (keys: in the index)
    /// that want `call`, each once.
    pub(crate) fn matching(
        &self,
        function_id: &str,
        active: &[Observer],
        call: &Call,
    ) -> Vec<Observer> {
        match &self.rule {
            Match::Predicate(wants) => active
                .iter()
                .filter(|obs| {
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| wants(obs, call)))
                        .unwrap_or_else(|panic| {
                            self.warn_once(function_id, "predicate", panic);
                            false
                        })
                })
                .cloned()
                .collect(),
            Match::Keys {
                call: call_keys, ..
            } => {
                let keys = self.keys_of_call(call, call_keys.as_ref(), function_id);
                let index = self.read();
                let mut found: Vec<Observer> = Vec::new();
                keys.for_each(|key| {
                    for obs in index.by_key.get(key).into_iter().flatten() {
                        if !found.contains(obs) {
                            found.push(obs.clone());
                        }
                    }
                    false
                });
                found
            }
        }
    }

    /// With keys: whether an observer is filed under any key of `call`, in
    /// one hash lookup per key and no allocation for keys borrowed from the
    /// call. `None` for a predicate, which says nothing without an observer.
    pub(crate) fn wants_any(&self, function_id: &str, call: &Call) -> Option<bool> {
        let Match::Keys {
            call: call_keys, ..
        } = &self.rule
        else {
            return None;
        };
        let keys = self.keys_of_call(call, call_keys.as_ref(), function_id);
        let index = self.read();
        Some(keys.for_each(|key| index.by_key.contains_key(key)))
    }
}

/// The level an added or removed hook is set at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Level {
    /// Every function this worker hosts.
    Worker,
    /// One function, by id.
    Function,
}

/// An added hook and the generation it was set at: a hook set again is a new
/// hook, caught up for the observers already active.
#[derive(Clone)]
pub(crate) struct AddedSlot {
    pub hook: Arc<dyn AddedHook>,
    pub generation: u64,
}

#[derive(Clone, Default)]
struct LevelHooks {
    added: Option<AddedSlot>,
    removed: Option<Arc<dyn RemovedHook>>,
}

#[derive(Clone, Default)]
struct FunctionHooks {
    level: LevelHooks,
    matching: Option<Match>,
    deliver: Option<Arc<dyn DeliverHook>>,
}

impl FunctionHooks {
    fn is_empty(&self) -> bool {
        self.level.added.is_none()
            && self.level.removed.is_none()
            && self.matching.is_none()
            && self.deliver.is_none()
    }
}

#[derive(Default)]
struct HookState {
    generation: u64,
    worker: LevelHooks,
    by_id: HashMap<String, FunctionHooks>,
    /// Hook ids already named in the unregistered-id warning.
    warned: HashSet<String>,
}

/// The hooks of this worker, by function id and for all its functions. They
/// are stored apart from the registrations: they stay in place across
/// `unregister()` and a later registration of the same id, and the hook task
/// of a function looks them up each time it runs one.
#[derive(Default)]
pub(crate) struct HookStore {
    state: Mutex<HookState>,
}

/// The store, locked: a registration reads the match and deliver hooks of
/// its id and files its entry under this lock, so a hook set meanwhile
/// reaches it.
pub(crate) struct HookStoreGuard<'a>(std::sync::MutexGuard<'a, HookState>);

impl HookStoreGuard<'_> {
    pub(crate) fn deliver(&self, function_id: &str) -> Option<Arc<dyn DeliverHook>> {
        self.0
            .by_id
            .get(function_id)
            .and_then(|hooks| hooks.deliver.clone())
    }

    pub(crate) fn matching(&self, function_id: &str) -> Option<Match> {
        self.0
            .by_id
            .get(function_id)
            .and_then(|hooks| hooks.matching.clone())
    }
}

impl HookStore {
    pub(crate) fn lock(&self) -> HookStoreGuard<'_> {
        HookStoreGuard(self.state.lock_or_recover())
    }

    /// The added hooks that apply to `function_id`: the worker's, then the
    /// function's.
    pub(crate) fn added(&self, function_id: &str) -> [Option<AddedSlot>; 2] {
        let state = self.state.lock_or_recover();
        [
            state.worker.added.clone(),
            state
                .by_id
                .get(function_id)
                .and_then(|hooks| hooks.level.added.clone()),
        ]
    }

    /// The added hook of one level.
    pub(crate) fn added_at(&self, level: Level, function_id: &str) -> Option<AddedSlot> {
        let [worker, function] = self.added(function_id);
        match level {
            Level::Worker => worker,
            Level::Function => function,
        }
    }

    /// The removed hooks that apply to `function_id`: the function's, then
    /// the worker's.
    pub(crate) fn removed(&self, function_id: &str) -> [Option<Arc<dyn RemovedHook>>; 2] {
        let state = self.state.lock_or_recover();
        [
            state
                .by_id
                .get(function_id)
                .and_then(|hooks| hooks.level.removed.clone()),
            state.worker.removed.clone(),
        ]
    }

    /// Hook ids not yet named in the unregistered-id warning for which
    /// `registered` says no; they are not named again.
    pub(crate) fn take_unregistered(&self, registered: impl Fn(&str) -> bool) -> Vec<String> {
        let mut state = self.state.lock_or_recover();
        let HookState { by_id, warned, .. } = &mut *state;
        let mut ids: Vec<String> = by_id
            .iter()
            .filter(|(id, hooks)| !hooks.is_empty() && !registered(id) && !warned.contains(*id))
            .map(|(id, _)| id.clone())
            .collect();
        ids.sort();
        warned.extend(ids.iter().cloned());
        ids
    }

    fn set_added(&self, function_id: Option<&str>, hook: Option<Arc<dyn AddedHook>>) -> bool {
        let mut state = self.state.lock_or_recover();
        state.generation += 1;
        let generation = state.generation;
        let slot = hook.map(|hook| AddedSlot { hook, generation });
        let set = slot.is_some();
        match function_id {
            None => state.worker.added = slot,
            Some(id) => state.by_id.entry(id.to_string()).or_default().level.added = slot,
        }
        set
    }

    fn set_removed(&self, function_id: Option<&str>, hook: Option<Arc<dyn RemovedHook>>) {
        let mut state = self.state.lock_or_recover();
        match function_id {
            None => state.worker.removed = hook,
            Some(id) => state.by_id.entry(id.to_string()).or_default().level.removed = hook,
        }
    }
}

impl IIIClient {
    /// Sets the added hook of `function_id`, or of every function when it is
    /// `None`, and catches it up for the observers already active.
    fn set_added_hook(&self, function_id: Option<&str>, hook: Option<Arc<dyn AddedHook>>) {
        if !self.inner.hooks.set_added(function_id, hook) {
            return;
        }
        let level = if function_id.is_some() {
            Level::Function
        } else {
            Level::Worker
        };
        for entry in self.hosted_entries(function_id) {
            entry.enqueue(self, HookOp::CatchUp(CatchUp::Added(level)));
        }
    }

    fn set_deliver_hook(&self, function_id: &str, hook: Option<Arc<dyn DeliverHook>>) {
        let entry = {
            // Under the store lock, as a registration reads it: the entry of
            // a function registered meanwhile gets the hook either way.
            let mut store = self.inner.hooks.lock();
            store
                .0
                .by_id
                .entry(function_id.to_string())
                .or_default()
                .deliver = hook.clone();
            let entry = self.function_entry(function_id);
            if let Some(entry) = &entry {
                entry.set_deliver(hook);
            }
            entry
        };
        if let Some(entry) = entry {
            self.reconcile_registration(&entry);
        }
    }

    fn set_match_hook(&self, function_id: &str, rule: Option<Match>) {
        let entry = {
            let mut store = self.inner.hooks.lock();
            store
                .0
                .by_id
                .entry(function_id.to_string())
                .or_default()
                .matching = rule.clone();
            let entry = self.function_entry(function_id);
            if let Some(entry) = &entry {
                entry.set_matcher(rule.map(Matcher::new));
            }
            entry
        };
        // a key index is rebuilt from the observers already active
        if let Some(entry) = entry {
            entry.enqueue(self, HookOp::CatchUp(CatchUp::Index));
        }
    }

    /// The function entries a hook set for `function_id` (every function
    /// when `None`) applies to.
    fn hosted_entries(&self, function_id: Option<&str>) -> Vec<Arc<FunctionEntry>> {
        let functions = self.functions();
        match function_id {
            Some(id) => functions
                .get(id)
                .map(|data| data.observe.clone())
                .into_iter()
                .collect(),
            None => functions
                .values()
                .map(|data| data.observe.clone())
                .collect(),
        }
    }

    /// Runs `hook` when an observer is added to any function this worker
    /// hosts, before the function's own added hook. Returns the client, for
    /// chaining.
    ///
    /// ```rust,no_run
    /// # use iii_sdk::{IIIClient, Observer};
    /// # use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
    /// # fn demo(iii: &IIIClient) {
    /// // keep the event stream open while anything is observed
    /// let watched = Arc::new(AtomicUsize::new(0));
    /// iii.on_observer_added({
    ///     let watched = watched.clone();
    ///     move |_obs: Observer| {
    ///         let watched = watched.clone();
    ///         async move {
    ///             watched.fetch_add(1, Ordering::SeqCst);
    ///             Ok(())
    ///         }
    ///     }
    /// })
    /// .on_observer_removed(move |_obs: Observer| {
    ///     let watched = watched.clone();
    ///     async move {
    ///         watched.fetch_sub(1, Ordering::SeqCst);
    ///     }
    /// });
    /// # }
    /// ```
    ///
    /// A hook set once functions already have active observers runs for each
    /// of them at once, and the SDK logs one warning per function.
    pub fn on_observer_added(&self, hook: impl AddedHook) -> &Self {
        self.set_added_hook(None, Some(Arc::new(hook)));
        self
    }

    /// Runs `hook` when an observer is added to `function_id`, after the
    /// worker's added hook. Set it before registering the function: then it
    /// runs for every observer.
    pub fn on_observer_added_for(&self, function_id: &str, hook: impl AddedHook) -> &Self {
        self.set_added_hook(Some(function_id), Some(Arc::new(hook)));
        self
    }

    /// Removes the added hook set with [`on_observer_added`](Self::on_observer_added).
    pub fn clear_observer_added(&self) -> &Self {
        self.set_added_hook(None, None);
        self
    }

    /// Removes the added hook of `function_id`.
    pub fn clear_observer_added_for(&self, function_id: &str) -> &Self {
        self.set_added_hook(Some(function_id), None);
        self
    }

    /// Runs `hook` when an observer of any function this worker hosts is
    /// removed, after the function's own removed hook.
    pub fn on_observer_removed(&self, hook: impl RemovedHook) -> &Self {
        self.inner.hooks.set_removed(None, Some(Arc::new(hook)));
        self
    }

    /// Runs `hook` when an observer of `function_id` is removed, before the
    /// worker's removed hook.
    pub fn on_observer_removed_for(&self, function_id: &str, hook: impl RemovedHook) -> &Self {
        self.inner
            .hooks
            .set_removed(Some(function_id), Some(Arc::new(hook)));
        self
    }

    /// Removes the removed hook set with [`on_observer_removed`](Self::on_observer_removed).
    pub fn clear_observer_removed(&self) -> &Self {
        self.inner.hooks.set_removed(None, None);
        self
    }

    /// Removes the removed hook of `function_id`.
    pub fn clear_observer_removed_for(&self, function_id: &str) -> &Self {
        self.inner.hooks.set_removed(Some(function_id), None);
        self
    }

    /// Says which calls of `function_id` each observer wants; see [`Match`].
    /// Set while observers are active, a key index is rebuilt from them.
    pub fn on_observer_match_for(&self, function_id: &str, rule: Match) -> &Self {
        self.set_match_hook(function_id, Some(rule));
        self
    }

    /// Removes the match hook of `function_id`: every observer wants every
    /// call.
    pub fn clear_observer_match_for(&self, function_id: &str) -> &Self {
        self.set_match_hook(function_id, None);
        self
    }

    /// Delivers each call of `function_id` with `hook`, which decides which
    /// observers get it and what its caller gets back; iii delivers nothing
    /// for the function. See [`DeliverHook`].
    ///
    /// A function with a deliver hook needs an engine that advertised
    /// `observe.v2`: against an older one it stays unregistered, and the SDK
    /// logs `UNSUPPORTED` once.
    ///
    /// ```rust,no_run
    /// # use iii_sdk::{Call, CallRequest, IIIClient, Observer};
    /// # use serde_json::json;
    /// # fn demo(iii: &IIIClient) {
    /// // each request goes to its route; the route's result is the response
    /// iii.on_observer_deliver_for("http::on-request", {
    ///     let iii = iii.clone();
    ///     move |call: Call, observers: Vec<Observer>| {
    ///         let iii = iii.clone();
    ///         async move {
    ///             let path = call.input["path"].clone();
    ///             let Some(route) = observers.iter().find(|obs| obs.args_value()["path"] == path)
    ///             else {
    ///                 return Ok(json!({ "status_code": 404 }));
    ///             };
    ///             if !route.matches(&call) {
    ///                 return Ok(json!({ "status_code": 422 }));
    ///             }
    ///             iii.call(CallRequest::observer(route, call.input.clone())).await
    ///         }
    ///     }
    /// });
    /// # }
    /// ```
    pub fn on_observer_deliver_for(&self, function_id: &str, hook: impl DeliverHook) -> &Self {
        self.set_deliver_hook(function_id, Some(Arc::new(hook)));
        self
    }

    /// Removes the deliver hook of `function_id`: iii delivers its calls.
    pub fn clear_observer_deliver_for(&self, function_id: &str) -> &Self {
        self.set_deliver_hook(function_id, None);
        self
    }

    /// The active observers of `function_id`, a function this worker hosts.
    pub fn observers(&self, function_id: &str) -> Vec<Observer> {
        self.function_entry(function_id)
            .map(|entry| entry.observers())
            .unwrap_or_default()
    }

    /// Whether `function_id`, a function this worker hosts, has any active
    /// observer.
    pub fn is_observed(&self, function_id: &str) -> bool {
        self.function_entry(function_id)
            .is_some_and(|entry| entry.is_observed())
    }

    /// Whether `call` of `function_id` would reach any observer; see
    /// [`FunctionRef::is_observed_for`](crate::runtime::FunctionRef::is_observed_for).
    pub fn is_observed_for(&self, function_id: &str, call: &Call) -> bool {
        self.function_entry(function_id)
            .is_some_and(|entry| entry.is_observed_for(call))
    }
}

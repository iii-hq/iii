// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! `observe` / `unobserve`, and the observing connection's lifecycle
//! (disconnect, grace, `Reattach`).

use std::{
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};

use iii_filter::Filter;
use serde_json::{Map, Value};
use uuid::Uuid;

use super::{
    Departed, FnKey, Held, ObserverRegistry,
    lifecycle::{
        CaptureSet, Flags, NewObserver, ObserveSpec, Observer, Observing, Policy, Tombstone,
        Transition,
    },
    owner::check_options,
};
use crate::{
    engine::Engine,
    protocol::{
        DEFAULT_NAMESPACE, ErrorBody, INVALID_NAMESPACE, Message, ObserveSource, ObserveTarget,
        ObserverState, default_capture, is_blank_namespace, observe_codes,
    },
    worker_connections::WorkerConnection,
};

/// Longest observer id accepted. Ids are client-generated UUIDs (CONTRACT
/// D3); the bound only keeps a misbehaving client from filling memory.
const MAX_OBSERVER_ID_LEN: usize = 128;

/// A decoded `observe` message.
#[derive(Debug, Clone)]
pub struct ObserveRequest {
    pub id: String,
    pub source: ObserveSource,
    pub target: ObserveTarget,
    pub metadata: Option<Value>,
    pub max_fires: Option<u64>,
    pub expires_in_ms: Option<u64>,
    pub max_in_flight: Option<u32>,
}

impl ObserveRequest {
    /// The spec a replay is compared on (see [`ObserveSpec`]).
    fn spec(&self, default_max_in_flight: u32) -> ObserveSpec {
        let namespace =
            |ns: &Option<String>| ns.clone().unwrap_or_else(|| DEFAULT_NAMESPACE.to_string());
        let filters = match &self.source.filters {
            None => Filter::match_all().normalized().clone(),
            Some(src) => Filter::parse(src)
                .map(|filter| filter.normalized().clone())
                .unwrap_or_else(|_| src.clone()),
        };
        ObserveSpec {
            function: (
                namespace(&self.source.namespace),
                self.source.function_id.clone(),
            ),
            target: (
                namespace(&self.target.namespace),
                self.target.function_id.clone(),
            ),
            args: match &self.source.args {
                None | Some(Value::Null) => Value::Object(Map::new()),
                Some(args) => args.clone(),
            },
            filters,
            capture: CaptureSet::from_list(&self.source.capture()).to_list(),
            on: self.source.on(),
            deliver: self.source.deliver(),
            metadata: self.metadata.clone(),
            max_fires: self.max_fires,
            expires_in_ms: self.expires_in_ms,
            max_in_flight: self.max_in_flight.unwrap_or(default_max_in_flight),
        }
    }
}

fn reject(code: &str, message: impl Into<String>) -> ErrorBody {
    ErrorBody::new(code, message)
}

/// The input of `rbac.on_observer_registration_function_id`: the
/// registration as the observing worker asked for it, and its session's
/// context.
fn registration_hook_input(
    id: &str,
    function: (&str, &str),
    target: (&str, &str),
    args: &Value,
    filters: Option<&Value>,
    metadata: Option<&Value>,
    context: &Value,
) -> Value {
    serde_json::json!({
        "observer_id": id,
        "function_id": function.1,
        "namespace": function.0,
        "args": args,
        "filters": filters,
        "target_function_id": target.1,
        "target_namespace": target.0,
        "metadata": metadata,
        "context": context,
    })
}

/// [`registration_hook_input`] for an observer that exists: the
/// registration its observing worker asked for (before a hook or a function
/// prefix rewrote it), its filters in normal form, absent when they match
/// everything.
fn requested_registration_hook_input(observer: &Observer, context: &Value) -> Value {
    let (function, target, args, filters, metadata) = match observer.requested_spec() {
        Some(spec) => (
            &spec.function,
            &spec.target,
            &spec.args,
            &spec.filters,
            spec.metadata.as_ref(),
        ),
        None => (
            &observer.function,
            &observer.target,
            &observer.args,
            observer.filter.normalized(),
            observer.metadata.as_ref(),
        ),
    };
    let filters = (!filters.as_object().is_some_and(Map::is_empty)).then_some(filters);
    registration_hook_input(
        &observer.id,
        (&function.0, &function.1),
        (&target.0, &target.1),
        args,
        filters,
        metadata,
        context,
    )
}

/// Calls the registration hook `hook` for observer `id` of connection
/// `worker`: its object answer, which may rewrite the registration, or
/// `None` when it denies it (any other answer, or a failed call).
async fn registration_hook(
    engine: &Engine,
    hook: &str,
    worker: Uuid,
    id: &str,
    input: Value,
) -> Option<Map<String, Value>> {
    match engine
        .call_value_with_metadata_ns(DEFAULT_NAMESPACE, hook, input, None)
        .await
    {
        Ok(Some(Value::Object(answer))) => Some(answer),
        other => {
            tracing::warn!(
                worker_id = %worker,
                observer_id = %id,
                result = ?other,
                "observer registration denied by hook"
            );
            None
        }
    }
}

fn owned_by_another_connection(id: &str) -> ErrorBody {
    reject(
        observe_codes::OBSERVER_ID_CONFLICT,
        format!("observer `{id}` is owned by another connection"),
    )
}

/// `suspended` because its observing worker is away (decision 47): not
/// disposed, not in `error`, its connection closed within the grace period.
fn left_behind(flags: &Flags) -> bool {
    flags.disposed.is_none() && flags.failed.is_none() && !flags.observing_connected
}

impl ObserverRegistry {
    /// Handles `observe` from `worker`: creates the observer, or answers a
    /// replay, and replies with `observeresult`. Every reply and every later
    /// `observerstatechanged` reach the observing worker in order.
    pub async fn observe(
        self: &Arc<Self>,
        engine: &Engine,
        worker: &WorkerConnection,
        request: ObserveRequest,
    ) {
        self.remember_runtime();
        let id = request.id.clone();
        if let Err(error) = self.create(engine, worker, request).await {
            tracing::debug!(
                worker_id = %worker.id,
                observer_id = %id,
                code = %error.code,
                message = %error.message,
                "observe rejected"
            );
            self.notify(
                worker.id,
                Message::ObserveResult {
                    id,
                    state: ObserverState::Error,
                    error: Some(error),
                    warnings: Vec::new(),
                },
            );
        }
    }

    /// Validates and stores the observer. On success the `observeresult` has
    /// already been queued; an `Err` is a rejection the caller reports.
    async fn create(
        self: &Arc<Self>,
        engine: &Engine,
        worker: &WorkerConnection,
        request: ObserveRequest,
    ) -> Result<(), ErrorBody> {
        if request.id.trim().is_empty() || request.id.len() > MAX_OBSERVER_ID_LEN {
            return Err(reject(
                observe_codes::INVALID_ARGS,
                format!(
                    "an observer id is a non-empty string of at most {MAX_OBSERVER_ID_LEN} bytes"
                ),
            ));
        }

        // A replay: the same spec from the observing connection is a no-op
        // that answers with the current state.
        let spec = request.spec(self.config().default_max_in_flight);
        let existing = self.get(&request.id);
        if let Some(existing) = &existing
            && existing.observing_connection() == worker.id
        {
            if !existing.spec_matches(&spec) {
                return Err(reject(
                    observe_codes::OBSERVER_ID_CONFLICT,
                    format!(
                        "observer `{}` already exists with a different spec",
                        request.id
                    ),
                ));
            }
            #[cfg(test)]
            super::interleave::point();
            // An observer in `error` answers its replay with that error, a
            // refusal (decision 56): it is disposed as it answers, so the
            // engine holds nothing under the id either.
            return self.answer_replay(existing, worker.id);
        }
        // A replay of an observer disposed meanwhile (max_fires, expiry, an
        // admin's dispose, a new connection of its worker replacing it) stays
        // disposed, whichever observer holds the id. A connection that heard
        // the disposal replays nothing more of it: the id is free for it.
        if let Some(tombstone) = self.tombstones.get(request.id.as_str())
            && tombstone.answers(worker.id, Instant::now())
        {
            let reason = ErrorBody::clone(&tombstone.reason);
            drop(tombstone);
            // Its replay answered, the connection has heard the disposal.
            if let Some(mut tombstone) = self.tombstones.get_mut(request.id.as_str())
                && tombstone.connection == worker.id
            {
                tombstone.heard = true;
            }
            self.notify(
                worker.id,
                Message::ObserveResult {
                    id: request.id,
                    state: ObserverState::Disposed,
                    error: Some(reason),
                    warnings: Vec::new(),
                },
            );
            return Ok(());
        }
        // Another connection's observer holds the id. Its own worker, back
        // on a new connection while it is suspended, replaces it (decision
        // 51) when the new observer is created: every check below runs
        // first, and the suspended observer keeps the id meanwhile. Anyone
        // else waits for the grace period to end.
        let replacing = match existing {
            Some(existing) if self.left_behind_by(&existing, worker) => Some(existing),
            Some(_) => return Err(owned_by_another_connection(&request.id)),
            None => None,
        };

        for (field, function_id) in [
            ("source.function_id", &request.source.function_id),
            ("target.function_id", &request.target.function_id),
        ] {
            if function_id.trim().is_empty() {
                return Err(reject(
                    observe_codes::INVALID_ARGS,
                    format!("`{field}` is empty"),
                ));
            }
        }

        let ObserveRequest {
            id,
            source,
            target,
            metadata,
            max_fires,
            expires_in_ms,
            max_in_flight,
        } = request;

        for (field, value) in [
            ("source.namespace", &source.namespace),
            ("target.namespace", &target.namespace),
        ] {
            if is_blank_namespace(value) {
                return Err(reject(
                    INVALID_NAMESPACE,
                    format!(
                        "`{field}` is empty: name a namespace, or omit it to use `{DEFAULT_NAMESPACE}`"
                    ),
                ));
            }
        }

        // Values a newer peer may send and this engine cannot honour
        // (CONTRACT D14). Never defaulted: the observer would silently do
        // something other than what was asked.
        if let Some(on) = &source.on
            && !on.is_known()
        {
            return Err(unsupported("source.on", on.as_str()));
        }
        if let Some(deliver) = &source.deliver
            && !deliver.is_known()
        {
            return Err(unsupported("source.deliver", deliver.as_str()));
        }
        if let Some(unknown) = source
            .capture
            .iter()
            .flatten()
            .find(|capture| !capture.is_known())
        {
            return Err(unsupported("source.capture", unknown.as_str()));
        }

        for (field, value) in [
            ("max_fires", max_fires),
            ("expires_in_ms", expires_in_ms),
            ("max_in_flight", max_in_flight.map(u64::from)),
        ] {
            if value == Some(0) {
                return Err(reject(
                    observe_codes::INVALID_ARGS,
                    format!("`{field}` must be at least 1, or omitted"),
                ));
            }
        }

        let mut function_id = source.function_id.clone();
        let function_namespace = source
            .namespace
            .clone()
            .unwrap_or_else(|| DEFAULT_NAMESPACE.to_string());
        let mut target_id = target.function_id.clone();
        let target_namespace = target
            .namespace
            .clone()
            .unwrap_or_else(|| DEFAULT_NAMESPACE.to_string());
        // `null` is no args, stored (and shown to the host) as `{}`.
        let mut args = match &source.args {
            None | Some(Value::Null) => Value::Object(Map::new()),
            Some(args) => args.clone(),
        };
        let mut filters = source.filters.clone();
        let mut activation_metadata = metadata;

        if let Some(session) = &worker.session {
            // An object answer may rewrite the registration, anything else
            // denies.
            if let Some(hook) = session
                .config
                .rbac
                .as_ref()
                .and_then(|rbac| rbac.on_observer_registration_function_id.as_ref())
            {
                let hook_input = registration_hook_input(
                    &id,
                    (&function_namespace, &function_id),
                    (&target_namespace, &target_id),
                    &args,
                    filters.as_ref(),
                    activation_metadata.as_ref(),
                    &session.context,
                );
                match registration_hook(engine, hook, worker.id, &id, hook_input).await {
                    Some(answer) => {
                        if let Some(s) = answer.get("function_id").and_then(Value::as_str) {
                            function_id = s.to_string();
                        }
                        if let Some(s) = answer.get("target_function_id").and_then(Value::as_str) {
                            target_id = s.to_string();
                        }
                        if let Some(v) = answer.get("args") {
                            args = if v.is_null() {
                                Value::Object(Map::new())
                            } else {
                                v.clone()
                            };
                        }
                        if let Some(v) = answer.get("filters") {
                            filters = (!v.is_null()).then(|| v.clone());
                        }
                        if let Some(v) = answer.get("metadata") {
                            activation_metadata = (!v.is_null()).then(|| v.clone());
                        }
                    }
                    None => {
                        return Err(reject(
                            observe_codes::FORBIDDEN,
                            "observer registration denied by the registration hook",
                        ));
                    }
                }
            }
            // A prefixed session's own functions live under `prefix::id`.
            if let Some(prefix) = &session.function_registration_prefix {
                target_id = format!("{prefix}::{target_id}");
            }
            for (namespace, function) in [
                (&function_namespace, &function_id),
                (&target_namespace, &target_id),
            ] {
                if !engine.is_function_allowed(session, Some(namespace), function) {
                    return Err(reject(
                        observe_codes::FORBIDDEN,
                        format!("function '{function}' not allowed in namespace '{namespace}'"),
                    ));
                }
            }
        }

        let function: FnKey = (function_namespace, function_id);
        let target_key: FnKey = (target_namespace, target_id);
        if function == target_key {
            return Err(reject(
                observe_codes::SELF_TARGET,
                format!("`{}` cannot be the target of its own observer", function.1),
            ));
        }
        self.check_target_invocable(&target_key)?;

        // `null` is absent, and absent matches every event.
        let filter = match &filters {
            Some(src) => {
                Filter::parse(src).map_err(|err| reject(err.code.as_str(), err.message))?
            }
            None => Filter::match_all(),
        };

        // What depends on the function's registration is checked now when it
        // is registered, and when it registers otherwise.
        let mut warnings = Vec::new();
        let formats = self.function_formats(&function);
        match &formats {
            Some((request_format, response_format)) => {
                let options = self.function_entry(&function).and_then(|e| e.options());
                check_options(options.as_ref(), &function, &args)?;
                warnings = filter
                    .check_schema_paths(request_format.as_ref(), response_format.as_ref())
                    .map_err(|err| reject(err.code.as_str(), err.message))?;
            }
            None => {
                warnings.push(format!(
                    "`{}` is not registered yet: its args and filter paths are checked when it \
                     registers",
                    function.1
                ));
            }
        }

        let config = self.config();
        let owned = self
            .by_connection
            .get(&worker.id)
            .map(|ids| ids.len())
            .unwrap_or(0);
        if owned >= config.max_per_connection {
            return Err(reject(
                observe_codes::LIMIT_EXCEEDED,
                format!(
                    "this connection already owns {owned} observers (observers.max_per_connection)"
                ),
            ));
        }
        // The observer this one replaces leaves the function as it joins.
        let replaced_here = usize::from(replacing.as_ref().is_some_and(|replaced| {
            replaced.function == function && replaced.with_flags(left_behind)
        }));
        let on_function = self
            .function_entry(&function)
            .map(|e| e.limited_observer_count())
            .unwrap_or(0)
            .saturating_sub(replaced_here);
        if on_function >= config.max_per_function {
            return Err(reject(
                observe_codes::LIMIT_EXCEEDED,
                format!(
                    "`{}` already has {on_function} observers (observers.max_per_function)",
                    function.1
                ),
            ));
        }

        let capture = source.capture.clone().unwrap_or_else(default_capture);
        let target_available = self.is_registered(&target_key);
        let observer = Arc::new(Observer::new(NewObserver {
            id,
            target: target_key,
            spec: Some(spec),
            args,
            filter,
            capture: CaptureSet::from_list(&capture),
            on: source.on(),
            deliver: source.deliver(),
            metadata: activation_metadata,
            policy: Policy {
                max_fires,
                expires_in_ms,
                expires_at: expires_in_ms.map(|ms| Instant::now() + Duration::from_millis(ms)),
                max_in_flight: max_in_flight.unwrap_or(config.default_max_in_flight).max(1),
                max_queue: config.max_queue_per_observer,
            },
            warnings: warnings.clone(),
            observing: {
                let (worker_name, namespace) = self.worker_identity(worker.id);
                Observing {
                    connection: worker.id,
                    worker_name,
                    namespace,
                    session: worker.session.clone(),
                    // Its creation's answer is queued under its status lock,
                    // before any transition is reported (see `publish`).
                    answered: true,
                }
            },
            flags: Flags {
                observing_connected: true,
                function_available: formats.is_some(),
                target_available,
                owner_acked: self.acked_by_default(&function),
                announced: None,
                rejected_by_owner: false,
                activated_once: false,
                failed: None,
                disposed: None,
                tombstone: true,
            },
            function,
        }));

        let inserted = self.publish(&observer, worker.id, warnings, replacing.as_ref());
        if !inserted {
            // The suspended observer's connection came back meanwhile, or
            // another `observe` created the id first.
            return Err(match replacing {
                Some(_) => owned_by_another_connection(&observer.id),
                None => reject(
                    observe_codes::OBSERVER_ID_CONFLICT,
                    format!("observer `{}` was created concurrently", observer.id),
                ),
            });
        }

        // The registry may have changed between the checks above and the
        // insert; the listener only sees observers it can find.
        let function_available = self.is_registered(&observer.function);
        let target_available = self.is_registered(&observer.target);
        self.update(&observer, |flags| {
            flags.function_available = function_available;
            flags.target_available = target_available;
        });
        self.arm_expiry(&observer);
        // The connection went away while this `observe` was being handled:
        // its teardown may already have suspended its other observers, so
        // this one joins them (suspended, grace timer) instead of staying
        // active with nobody to report to.
        if worker
            .cleanup_claimed
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            self.connection_closed(worker.id, worker.reattach_token);
        }
        Ok(())
    }

    /// Makes `observer` visible to every index, queues its `observeresult`
    /// and calls `engine::on-observers-changed` with the state it was created
    /// in, before any transition can be reported.
    ///
    /// With `replacing`, a suspended observer of the same worker under the
    /// same id (decision 51), the id passes from that observer to this one
    /// in one step: it is disposed here, unless its connection came back or
    /// it went meanwhile, and keeps the id until this one takes it, so no
    /// other `observe` finds the id free in between. Returns whether
    /// `observer` holds the id.
    pub(super) fn publish(
        self: &Arc<Self>,
        observer: &Arc<Observer>,
        connection: Uuid,
        warnings: Vec<String>,
        replacing: Option<&Arc<Observer>>,
    ) -> bool {
        let replaced = replacing.and_then(|previous| {
            self.dispose_left_behind(previous, connection)
                .map(|transition| (previous, transition))
        });
        // Under the status lock, which a transition takes: it is reported
        // after these, and a disposal, which finds the observer by its id,
        // finds its index entries too.
        let published = observer.with_state(|state, reason| {
            let took_id = match self.by_id.entry(observer.id.clone()) {
                dashmap::mapref::entry::Entry::Occupied(mut held) => {
                    // The observer just disposed stays in `by_id` until it
                    // is settled below.
                    let took = replaced
                        .as_ref()
                        .is_some_and(|(previous, _)| Arc::ptr_eq(held.get(), previous));
                    if took {
                        held.insert(observer.clone());
                    }
                    took
                }
                dashmap::mapref::entry::Entry::Vacant(vacant) => {
                    vacant.insert(observer.clone());
                    true
                }
            };
            if !took_id {
                return None;
            }
            self.index(observer, connection);
            self.state_counts.enter(state);
            // The engine hears nothing about the observers it owns.
            if !observer.is_engine_owned() {
                self.notify(
                    connection,
                    Message::ObserveResult {
                        id: observer.id.clone(),
                        state: state.clone(),
                        error: None,
                        warnings,
                    },
                );
            }
            Some(self.observer_changed(observer, state, reason))
        });
        if let Some((previous, transition)) = replaced {
            self.settle(previous, transition);
        }
        let Some(unqueued) = published else {
            return false;
        };
        if let Some(event) = unqueued {
            event.evaluate();
        }
        #[cfg(test)]
        super::interleave::point();
        self.attach_to_function(observer);
        // Whatever state it reached since its creation: a transition made
        // before it joined its function found it in no active set.
        self.refresh_observer(observer);
        true
    }

    /// Disposes an observer `expires_in_ms` after its creation.
    pub(super) fn arm_expiry(self: &Arc<Self>, observer: &Arc<Observer>) {
        let (Some(ms), Some(runtime)) = (observer.policy.expires_in_ms, self.runtime()) else {
            return;
        };
        let registry = Arc::downgrade(self);
        let weak_observer = Arc::downgrade(observer);
        let handle = runtime.spawn(async move {
            tokio::time::sleep(Duration::from_millis(ms)).await;
            if let (Some(registry), Some(observer)) = (registry.upgrade(), weak_observer.upgrade())
            {
                registry.dispose(
                    &observer,
                    observe_codes::EXPIRED,
                    "the observer's expires_in_ms elapsed",
                );
            }
        });
        observer.set_expiry_timer(handle.abort_handle());
    }

    /// The worker name and namespace `connection` registered with
    /// (`engine::workers::register`), which identify its worker across
    /// connections. No name until it registered one.
    fn worker_identity(&self, connection: Uuid) -> (Option<Arc<str>>, Arc<str>) {
        match self.deps.get() {
            Some(deps) => (
                deps.workers.get_worker_name(&connection).map(Arc::from),
                Arc::from(deps.workers.get_namespace(&connection)),
            ),
            None => (None, Arc::from(DEFAULT_NAMESPACE)),
        }
    }

    /// Whether `worker`, observing under the id of `existing` (another
    /// connection's observer), is that observer's worker back on a new
    /// connection while the observer is suspended: the same worker name in
    /// the same namespace, and `existing`'s observing connection away within
    /// its grace period (decision 51). Such an `observe` replaces `existing`
    /// when its observer is created ([`Self::publish`]). An observer
    /// disposed meanwhile frees the id anyway.
    ///
    /// A worker name is whatever a connection declares, so under RBAC the
    /// two connections must also be the same principal: both on a port
    /// without RBAC, or both with sessions whose auth context is equal.
    fn left_behind_by(&self, existing: &Arc<Observer>, worker: &WorkerConnection) -> bool {
        if existing.is_engine_owned() {
            return false;
        }
        let observing = existing.observing();
        let (worker_name, namespace) = self.worker_identity(worker.id);
        let same_principal = match (&observing.session, &worker.session) {
            (None, None) => true,
            (Some(before), Some(now)) => before.context == now.context,
            _ => false,
        };
        if worker_name.is_none()
            || observing.worker_name != worker_name
            || observing.namespace != namespace
            || !same_principal
        {
            return false;
        }
        existing.with_flags(|flags| flags.disposed.is_some() || left_behind(flags))
    }

    /// Disposes `previous`, the suspended observer a new connection of its
    /// worker replaces, when it is still suspended: under its status lock,
    /// which a `Reattach` takes too, so an observer whose connection came
    /// back first stays. The host let it go when the worker left, so nothing
    /// more is sent to it; the disposal leaves a tombstone, so the earlier
    /// connection, reattaching later, finds the observer disposed. Returns
    /// the transition, which the caller settles after the new observer has
    /// taken the id, or `None` when `previous` was not suspended.
    fn dispose_left_behind(
        &self,
        previous: &Arc<Observer>,
        connection: Uuid,
    ) -> Option<Transition> {
        let mut disposed = false;
        let transition = self.transition(previous, |flags| {
            if left_behind(flags) {
                flags.disposed = Some(Arc::new(ErrorBody::new(
                    observe_codes::OBSERVER_DISCONNECTED,
                    "replaced by an observe under its id from a new connection of its worker",
                )));
                disposed = true;
            }
        });
        if disposed {
            tracing::info!(
                observer_id = %previous.id,
                previous = %previous.observing().connection,
                connection = %connection,
                "a new connection of the observing worker replaces its observer"
            );
        }
        transition
    }

    /// Handles `unobserve` from `connection`: only the observing connection may
    /// dispose its observer this way.
    pub fn unobserve(&self, connection: Uuid, id: &str) {
        let Some(observer) = self.get(id) else {
            tracing::debug!(observer_id = %id, "unobserve for an unknown observer; ignoring");
            // Its handle is disposed: a tombstone of this connection (an
            // observer the engine disposed while the worker was away)
            // answers no later `observe` of the id.
            self.tombstones
                .remove_if(id, |_, tombstone| tombstone.connection == connection);
            return;
        };
        if observer.observing_connection() != connection {
            tracing::debug!(
                observer_id = %id,
                connection = %connection,
                "unobserve from a connection that does not own the observer; ignoring"
            );
            self.tombstones
                .remove_if(id, |_, tombstone| tombstone.connection == connection);
            return;
        }
        // A tombstone answers a replay of an observer disposed while its worker
        // was not looking. This worker asked for the disposal and replays no
        // handle it disposed, so the id is free for a new observer of its own
        // at once: a queue subscriber observing again under its stable id
        // finds its backlog.
        self.dispose_unobserved(&observer);
        self.tombstones
            .remove_if(id, |_, tombstone| tombstone.connection == connection);
    }

    /// The observing connection is gone: its observers are `suspended` for
    /// `observers.observer_grace_ms`, waiting for a `Reattach` that presents
    /// `reattach_token`, and disposed after that. The observers the engine
    /// disposed within `observers.observer_grace_ms` before the close wait
    /// with them, even with no observer left: their `disposed` may have been
    /// lost with the connection, so the `Reattach` (or, after the grace
    /// period, the connection's expired record) answers their replays with
    /// the disposal.
    pub fn connection_closed(self: &Arc<Self>, connection: Uuid, reattach_token: Uuid) {
        // Nothing more is queued for this connection.
        self.notifiers.remove(&connection);
        let observers: Vec<Arc<Observer>> = self
            .observing_for(connection)
            .into_iter()
            .filter(|o| o.observing_connection() == connection)
            .collect();
        // After the index read: an observer `forget` took out of it filed
        // its tombstone first.
        let disposed = self.tombstones_in_effect(connection);
        if observers.is_empty() && disposed.is_empty() {
            return;
        }
        // Suspended one by one, and taken out of their functions' active
        // sets together: one change of each function's set. The connection
        // that would hear about it is gone.
        let mut left = Vec::with_capacity(observers.len());
        let mut gone = Vec::new();
        for observer in &observers {
            if let Some(transition) =
                self.transition_telling(observer, |flags| flags.observing_connected = false, false)
            {
                let settled = self.settle_deferring_refresh(observer, transition);
                if settled.refresh {
                    left.push(observer.clone());
                }
                if settled.disposed {
                    gone.push(observer);
                }
            }
        }
        self.refresh_observers(&left);
        for observer in gone {
            self.forget(observer);
        }
        // Their functions' owners let them go until the worker reattaches.
        self.withdraw_all_while_away(&observers);
        let Some(runtime) = self.runtime() else {
            for observer in &observers {
                self.dispose(
                    observer,
                    observe_codes::OBSERVER_DISCONNECTED,
                    "the observing worker disconnected",
                );
            }
            return;
        };
        let now = tokio::time::Instant::now();
        // A grace period past what an `Instant` holds ends in 30 years, as
        // `tokio::time::sleep` takes it.
        let deadline = now
            .checked_add(Duration::from_millis(self.config().observer_grace_ms))
            .unwrap_or_else(|| now + Duration::from_secs(86_400 * 365 * 30));
        if let Some(previous) = self.departed.insert(
            connection,
            Departed {
                token: reattach_token,
                deadline,
                timer: None,
                disposed,
            },
        ) {
            if let Some(timer) = previous.timer {
                timer.abort();
            }
            // A second close keeps what the first one kept.
            if !previous.disposed.is_empty()
                && let Some(mut current) = self.departed.get_mut(&connection)
            {
                current.disposed.extend(previous.disposed);
            }
        }
        if self.queue_grace(deadline, connection) {
            return;
        }
        let registry = Arc::downgrade(self);
        let timer = runtime.spawn(async move {
            tokio::time::sleep_until(deadline).await;
            if let Some(registry) = registry.upgrade() {
                registry.grace_due(connection, deadline);
            }
        });
        // Unless a `Reattach` or another close came first.
        if let Some(mut departed) = self.departed.get_mut(&connection)
            && departed.deadline == deadline
        {
            departed.timer = Some(timer.abort_handle());
        }
    }

    /// Takes the tombstones that `connection` held while it was there (see
    /// `tombstoned`) and returns those still in effect, with why: each
    /// one's observer the engine disposed (or a `Reattach` gave it) within
    /// `observers.observer_grace_ms`, unless the connection observes its id
    /// again. Its work is in proportion to those tombstones.
    fn tombstones_in_effect(&self, connection: Uuid) -> Vec<(Arc<str>, Arc<ErrorBody>)> {
        let Some((_, held)) = self.tombstoned.remove(&connection) else {
            return Vec::new();
        };
        let now = Instant::now();
        held.into_iter()
            .filter(|(until, _)| *until > now)
            .filter_map(|(until, id)| {
                let reason = self
                    .tombstones
                    .get(&*id)
                    .filter(|tombstone| {
                        tombstone.connection == connection && tombstone.until == until
                    })?
                    .reason
                    .clone();
                let observed_again = self
                    .get(&id)
                    .is_some_and(|observer| observer.observing_connection() == connection);
                (!observed_again).then_some((id, reason))
            })
            .collect()
    }

    /// The grace period that `connection`'s close started, ending at
    /// `deadline`, ended: its observers are disposed, unless a `Reattach`
    /// came first or a later close started another grace period.
    pub(super) fn grace_due(&self, connection: Uuid, deadline: tokio::time::Instant) {
        // A `Reattach` that claimed the record first owns these observers now.
        let Some((_, departed)) = self
            .departed
            .remove_if(&connection, |_, departed| departed.deadline == deadline)
        else {
            return;
        };
        self.end_grace(connection, departed);
    }

    /// Ends `connection`'s grace period now, whatever its deadline.
    #[cfg(test)]
    fn grace_elapsed(&self, connection: Uuid) {
        let Some((_, departed)) = self.departed.remove(&connection) else {
            return;
        };
        self.end_grace(connection, departed);
    }

    fn end_grace(&self, connection: Uuid, departed: Departed) {
        // A disposal that raced the close may have filed a tombstone after
        // the close took them.
        self.tombstoned.remove(&connection);
        for observer in self.observing_for(connection) {
            if observer.observing_connection() == connection
                && !observer.with_flags(|flags| flags.observing_connected)
            {
                self.dispose(
                    &observer,
                    observe_codes::OBSERVER_DISCONNECTED,
                    "the observing worker did not reattach within observers.observer_grace_ms",
                );
            }
        }
        // Every observer the connection owned is disposed now, each with a
        // tombstone: the ones just disposed and any disposed while it was
        // away. With the ones disposed shortly before it closed, they make
        // the record, which outlives a tombstone for a worker that comes
        // back late.
        let now = Instant::now();
        let mut disposed = departed.disposed;
        disposed.extend(
            self.tombstones
                .iter()
                .filter(|tombstone| tombstone.connection == connection && tombstone.until > now)
                .map(|tombstone| (tombstone.key().clone(), tombstone.reason.clone())),
        );
        self.remember_expired(connection, departed.token, disposed);
    }

    /// `Reattach` from `connection`, presenting `token` for `previous`:
    /// re-parents `previous`'s suspended observers, counters intact. Returns
    /// whether it did. Works whether `previous` is still being torn down or
    /// is long gone, as long as its grace period has not ended. After that,
    /// see [`Self::reattach_late`].
    ///
    /// `max_per_connection` is not applied again: these were created already.
    /// RBAC is: the new connection's session may be narrower, so an observer
    /// whose function or target it may not invoke is disposed `FORBIDDEN`,
    /// and so is one its `rbac.on_observer_registration_function_id` denies,
    /// called as at creation with the new session's context. An object
    /// answer keeps the observer as it is: a rewrite applies to a new
    /// registration.
    pub async fn reattach(
        self: &Arc<Self>,
        engine: &Engine,
        previous: Uuid,
        token: Option<&str>,
        connection: &WorkerConnection,
    ) -> bool {
        let new_connection = connection;
        let connection = new_connection.id;
        let Some(token) = token else {
            return false;
        };
        let Some((_, departed)) = self
            .departed
            .remove_if(&previous, |_, d| d.token.to_string() == token)
        else {
            self.reattach_late(previous, token, connection);
            return false;
        };
        if let Some(timer) = &departed.timer {
            timer.abort();
        }
        // `Held` hashes and compares the observer's address, which never
        // changes while the entry holds it: its interior mutability is not
        // part of the key.
        #[allow(clippy::mutable_key_type)]
        let held = self
            .by_connection
            .remove(&previous)
            .map(|(_, held)| held)
            .unwrap_or_default();
        let mut admitted = Vec::new();
        for Held(observer) in held {
            let observing = observer.observing();
            if observing.connection != previous {
                continue;
            }
            observer.set_observing(Observing {
                connection,
                worker_name: observing.worker_name,
                namespace: observing.namespace,
                session: new_connection.session.clone(),
                answered: false,
            });
            self.by_connection
                .entry(connection)
                .or_default()
                .insert(Held(observer.clone()));
            // Disposed meanwhile: its index entries are gone, and so must the
            // one just added, or it would count against the cap forever.
            let live = self
                .by_id
                .get(&observer.id)
                .is_some_and(|current| Arc::ptr_eq(&current, &observer));
            if !live {
                let gone = Held(observer.clone());
                self.by_connection
                    .remove_if_mut(&connection, |_, observing| {
                        observing.remove(&gone);
                        observing.is_empty()
                    });
                continue;
            }
            if let Some(session) = &observer.observing().session {
                let denied = [&observer.function, &observer.target].into_iter().find(
                    |(namespace, function)| {
                        !engine.is_function_allowed(session, Some(namespace), function)
                    },
                );
                if let Some((namespace, function)) = denied {
                    self.dispose(
                        &observer,
                        observe_codes::FORBIDDEN,
                        format!(
                            "function '{function}' not allowed in namespace '{namespace}' \
                             for the reconnected worker"
                        ),
                    );
                    self.note_tombstone_of(connection, &observer.id);
                    continue;
                }
            }
            admitted.push(observer);
        }
        let mut given = Vec::new();
        for mut tombstone in self.tombstones.iter_mut() {
            if tombstone.connection == previous {
                tombstone.connection = connection;
                tombstone.heard = false;
                given.push((tombstone.until, tombstone.key().clone()));
            }
        }
        // The observers disposed shortly before `previous` closed get their
        // tombstones here, for the replay that follows.
        let until = Instant::now() + Duration::from_millis(self.config().observer_grace_ms);
        for (id, reason) in departed.disposed {
            self.remember_tombstone(
                id.clone(),
                Tombstone {
                    connection,
                    reason,
                    until,
                    // `connection` heard nothing of these disposals.
                    heard: false,
                },
            );
            given.push((until, id));
        }
        // They go with this connection's own `Reattach` too.
        given.sort_unstable_by_key(|(until, _)| *until);
        self.tombstoned.remove(&previous);
        self.note_tombstones(connection, given);
        let hook = new_connection.session.as_ref().and_then(|session| {
            let hook = session
                .config
                .rbac
                .as_ref()?
                .on_observer_registration_function_id
                .as_ref()?;
            Some((hook, &session.context))
        });
        let mut moved = Vec::with_capacity(admitted.len());
        for observer in admitted {
            if let Some((hook, context)) = hook {
                let input = requested_registration_hook_input(&observer, context);
                if registration_hook(engine, hook, connection, &observer.id, input)
                    .await
                    .is_none()
                {
                    self.dispose(
                        &observer,
                        observe_codes::FORBIDDEN,
                        "observer registration denied by the registration hook for the \
                         reconnected worker",
                    );
                    self.note_tombstone_of(connection, &observer.id);
                    continue;
                }
            }
            self.update(&observer, |flags| flags.observing_connected = true);
            moved.push(observer);
        }
        // Each function's owner let them go when the worker left; it hears
        // about them again, in creation order, and they go active on its ack.
        self.announce_returned(&moved);
        tracing::info!(
            previous = %previous,
            connection = %connection,
            observers = moved.len(),
            "Reattach: observers re-parented to the new connection"
        );
        true
    }

    /// `Reattach` after `previous`'s grace period ended: its observers are
    /// disposed, and the replay that follows on `connection` must not create
    /// them again with fresh counters (`max_fires`, `expires_in_ms`). Each one
    /// gets a tombstone on `connection`, so its replayed `observe` is answered
    /// with its disposal, as it would have been within the grace period.
    fn reattach_late(&self, previous: Uuid, token: &str, connection: Uuid) {
        let now = Instant::now();
        let Some((_, expired)) = self.expired.remove_if(&previous, |_, expired| {
            expired.until > now && expired.token.to_string() == token
        }) else {
            return;
        };
        let count = expired.disposed.len();
        let until = now + Duration::from_millis(self.config().observer_grace_ms);
        let mut given = Vec::with_capacity(count);
        for (id, reason) in expired.disposed {
            self.remember_tombstone(
                id.clone(),
                Tombstone {
                    connection,
                    reason,
                    until,
                    // `connection` heard nothing of these disposals.
                    heard: false,
                },
            );
            given.push((until, id));
        }
        self.note_tombstones(connection, given);
        tracing::info!(
            previous = %previous,
            connection = %connection,
            observers = count,
            "Reattach after the grace period: the observers stay disposed"
        );
    }

    /// Files the tombstone that `connection` holds for `id`, if it holds
    /// one (see [`Self::note_tombstones`]): that of an observer disposed as
    /// its `Reattach` moved it, before the connection observes with it.
    fn note_tombstone_of(&self, connection: Uuid, id: &str) {
        let Some(tombstone) = self
            .tombstones
            .get(id)
            .filter(|tombstone| tombstone.connection == connection)
            .map(|tombstone| (tombstone.until, tombstone.key().clone()))
        else {
            return;
        };
        self.note_tombstones(connection, [tombstone]);
    }

    /// Live observers, in any state but `disposed`.
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    /// Fires counted so far by `id`, for tests and introspection.
    pub fn fires(&self, id: &str) -> Option<u64> {
        self.get(id).map(|o| o.fires.load(Ordering::Relaxed))
    }
}

fn unsupported(field: &str, value: &str) -> ErrorBody {
    reject(
        observe_codes::UNSUPPORTED,
        format!("`{field}` value `{value}` is not supported by this engine"),
    )
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, atomic::Ordering};

    use serde_json::Value;

    use super::super::{ObserversConfig, events::test_support};
    use crate::{
        engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
        function::FunctionResult,
        protocol::{ObserverState, observe_codes},
    };

    fn engine_with_functions() -> Arc<Engine> {
        let engine = Arc::new(Engine::new());
        engine.observers.set_config(ObserversConfig {
            // The grace timer never fires on its own here.
            observer_grace_ms: 600_000,
            ..ObserversConfig::default()
        });
        for id in ["test::src", "test::dst"] {
            engine.register_function_handler(
                RegisterFunctionRequest {
                    function_id: id.to_string(),
                    description: None,
                    request_format: None,
                    response_format: None,
                    metadata: None,
                },
                Handler::new(|_: Value| async move { FunctionResult::Success(None) }),
            );
        }
        engine
    }

    fn state(engine: &Engine, id: &str) -> Option<ObserverState> {
        engine.observers.get(id).map(|o| o.state())
    }

    /// A refused `observe` leaves nothing in the engine (decision 56): its
    /// `observeresult` is its only answer, and an `unobserve` of its id from
    /// the same connection is ignored. It sends nothing, and the observer
    /// another connection holds under the id stays as it was.
    #[tokio::test]
    async fn a_refused_observe_stores_nothing_and_its_unobserve_is_ignored() {
        use crate::{engine::Outbound, protocol::Message};

        let engine = engine_with_functions();
        let registry = engine.observers.clone();
        let (holder, _holder_socket) = test_support::worker(&engine);
        test_support::observe_request(
            &engine,
            &holder,
            test_support::request("held", "test::src", "test::dst", None),
        )
        .await;
        let (worker, mut socket) = test_support::worker(&engine);
        for (request, code) in [
            (
                test_support::request("refused", "", "test::dst", None),
                observe_codes::INVALID_ARGS,
            ),
            (
                test_support::request("held", "test::src", "test::dst", None),
                observe_codes::OBSERVER_ID_CONFLICT,
            ),
        ] {
            let id = request.id.clone();
            registry.observe(&engine, &worker, request).await;
            match socket.try_recv() {
                Ok(Outbound::Protocol(Message::ObserveResult {
                    id: answered,
                    state: ObserverState::Error,
                    error: Some(error),
                    ..
                })) => {
                    assert_eq!(answered, id);
                    assert_eq!(error.code, code);
                }
                _ => panic!("`{id}` answered with its refusal"),
            }
            registry.unobserve(worker.id, &id);
            assert!(socket.try_recv().is_err(), "nothing answers the unobserve");
        }

        assert!(registry.get("refused").is_none());
        assert!(registry.tombstones.get("refused").is_none());
        let held = registry.get("held").unwrap();
        assert_eq!(held.state(), ObserverState::Active);
        assert_eq!(held.observing_connection(), holder.id);
        assert!(registry.observing_for(worker.id).is_empty());
    }

    /// Everything `socket` received so far.
    fn received(
        socket: &mut tokio::sync::mpsc::Receiver<crate::engine::Outbound>,
    ) -> Vec<crate::protocol::Message> {
        let mut received = Vec::new();
        while let Ok(outbound) = socket.try_recv() {
            if let crate::engine::Outbound::Protocol(message) = outbound {
                received.push(message);
            }
        }
        received
    }

    /// A reattached worker replaying an observer the engine holds in `error`
    /// is refused with that error, the handle's last answer (decision 56):
    /// the engine disposes the observer as it answers, sends nothing more
    /// about it and leaves no tombstone, so the id is free at once.
    #[tokio::test]
    async fn a_replay_of_an_observer_in_error_is_refused_and_disposes_it() {
        use crate::protocol::{ErrorBody, Message};

        let engine = engine_with_functions();
        let registry = engine.observers.clone();
        let (first, _first_socket) = test_support::worker(&engine);
        let request = test_support::request("obs", "test::src", "test::dst", None);
        test_support::observe_request(&engine, &first, request.clone()).await;
        registry.fail(
            &registry.get("obs").unwrap(),
            ErrorBody::new(observe_codes::OBSERVE_REJECTED, "rejected by its host"),
        );
        assert_eq!(state(&engine, "obs"), Some(ObserverState::Error));

        registry.connection_closed(first.id, first.reattach_token);
        let (second, mut socket) = test_support::worker(&engine);
        let token = first.reattach_token.to_string();
        assert!(
            registry
                .reattach(&engine, first.id, Some(&token), &second)
                .await
        );
        assert_eq!(state(&engine, "obs"), Some(ObserverState::Error));
        received(&mut socket);

        registry.observe(&engine, &second, request).await;
        match received(&mut socket).as_slice() {
            [
                Message::ObserveResult {
                    id,
                    state: ObserverState::Error,
                    error: Some(error),
                    ..
                },
            ] => {
                assert_eq!(id, "obs");
                assert_eq!(error.code, observe_codes::OBSERVE_REJECTED);
            }
            other => panic!("one refusal and nothing else, got {other:?}"),
        }
        assert!(registry.get("obs").is_none(), "disposed as it answered");
        assert!(registry.tombstones.get("obs").is_none());
        assert!(registry.observing_for(second.id).is_empty());
        registry.unobserve(second.id, "obs");
        assert!(received(&mut socket).is_empty(), "nothing answers it");

        let other = test_support::request("obs", "test::dst", "test::src", None);
        test_support::observe_request(&engine, &second, other).await;
        let observer = registry.get("obs").unwrap();
        assert_eq!(observer.observing_connection(), second.id);
        assert_eq!(observer.function.1, "test::dst");
    }

    /// An observer the engine disposes while its worker is connected frees
    /// its id on that connection, which heard the disposal: an `observe` of
    /// the id from it is a new observer (decision 56). A replay on a
    /// reattached connection is answered with the disposal, since the
    /// `disposed` may have been lost with the earlier connection, and that
    /// answer is the connection hearing it.
    #[tokio::test]
    async fn an_id_the_engine_disposed_is_free_on_the_connection_it_told() {
        use crate::protocol::Message;

        let engine = engine_with_functions();
        let registry = engine.observers.clone();
        let (first, _first_socket) = test_support::worker(&engine);
        // Held across the reattach, which re-parents it.
        test_support::observe_request(
            &engine,
            &first,
            test_support::request("kept", "test::dst", "test::src", None),
        )
        .await;
        let request = test_support::request("obs", "test::src", "test::dst", None);
        for _ in 0..2 {
            test_support::observe_request(&engine, &first, request.clone()).await;
            registry.dispose(
                &registry.get("obs").unwrap(),
                observe_codes::EXPIRED,
                "the observer's expires_in_ms elapsed",
            );
            assert!(registry.get("obs").is_none());
            let tombstone = registry.tombstones.get("obs").unwrap();
            assert!(tombstone.connection == first.id && tombstone.heard);
        }

        registry.connection_closed(first.id, first.reattach_token);
        let (second, mut socket) = test_support::worker(&engine);
        let token = first.reattach_token.to_string();
        assert!(
            registry
                .reattach(&engine, first.id, Some(&token), &second)
                .await
        );
        received(&mut socket);
        registry.observe(&engine, &second, request).await;
        assert!(
            registry.get("obs").is_none(),
            "the replay did not create it"
        );
        match received(&mut socket).as_slice() {
            [
                Message::ObserveResult {
                    state: ObserverState::Disposed,
                    error: Some(error),
                    ..
                },
            ] => assert_eq!(error.code, observe_codes::EXPIRED),
            other => panic!("the disposal answers the replay, got {other:?}"),
        }
        assert!(registry.tombstones.get("obs").unwrap().heard);
    }

    /// A connection whose only observer the engine disposed holds no
    /// observer when it closes, and waits for its `Reattach` all the same:
    /// the `Reattach` moves the tombstone, which answers the replay, and a
    /// `Reattach` after the grace period finds the disposal in the expired
    /// record. A connection with neither observers nor tombstones leaves
    /// nothing behind when it closes.
    #[tokio::test]
    async fn a_connection_left_only_tombstones_waits_for_its_reattach() {
        use crate::protocol::Message;

        fn answered_max_fires(messages: &[Message]) {
            match messages {
                [
                    Message::ObserveResult {
                        state: ObserverState::Disposed,
                        error: Some(error),
                        ..
                    },
                ] => assert_eq!(error.code, observe_codes::MAX_FIRES),
                other => panic!("the disposal answers the replay, got {other:?}"),
            }
        }

        let engine = engine_with_functions();
        let registry = engine.observers.clone();
        let (idle, _idle_socket) = test_support::worker(&engine);
        registry.connection_closed(idle.id, idle.reattach_token);
        assert!(registry.departed.is_empty());

        let (first, _first_socket) = test_support::worker(&engine);
        let request = test_support::request("obs", "test::src", "test::dst", None);
        test_support::observe_request(&engine, &first, request.clone()).await;
        registry.dispose(
            &registry.get("obs").unwrap(),
            observe_codes::MAX_FIRES,
            "the observer fired 1 times, its max_fires",
        );
        assert!(registry.observing_for(first.id).is_empty());
        registry.connection_closed(first.id, first.reattach_token);
        assert!(registry.departed.contains_key(&first.id));

        let (second, mut socket) = test_support::worker(&engine);
        let token = first.reattach_token.to_string();
        assert!(
            registry
                .reattach(&engine, first.id, Some(&token), &second)
                .await
        );
        received(&mut socket);
        registry.observe(&engine, &second, request.clone()).await;
        answered_max_fires(&received(&mut socket));
        assert!(registry.get("obs").is_none(), "the replay created nothing");

        registry.connection_closed(second.id, second.reattach_token);
        registry.grace_elapsed(second.id);
        let (third, mut socket) = test_support::worker(&engine);
        let token = second.reattach_token.to_string();
        assert!(
            !registry
                .reattach(&engine, second.id, Some(&token), &third)
                .await
        );
        received(&mut socket);
        registry.observe(&engine, &third, request).await;
        answered_max_fires(&received(&mut socket));
        assert!(registry.get("obs").is_none(), "the replay created nothing");

        assert!(registry.departed.is_empty());
        assert!(
            registry.tombstoned.len() == 1 && registry.tombstoned.contains_key(&third.id),
            "only the live connection given a tombstone is marked"
        );
    }

    /// An observer the engine disposed shortly before its connection closed
    /// is answered with its disposal after any `Reattach`: one within the
    /// grace period that comes after the observer's tombstone expired, and
    /// one after the grace period, from the expired record. The tombstone
    /// is made with a short grace period and the close starts a long one,
    /// so the order is the same whatever the timing.
    #[tokio::test]
    async fn an_observer_disposed_shortly_before_the_close_is_answered_after_any_reattach() {
        use std::time::{Duration, Instant};

        use crate::protocol::Message;

        for late in [false, true] {
            let engine = engine_with_functions();
            let registry = engine.observers.clone();
            let long = (*registry.config()).clone();
            registry.set_config(ObserversConfig {
                observer_grace_ms: 100,
                ..long.clone()
            });
            let (first, _first_socket) = test_support::worker(&engine);
            let request = test_support::request("obs", "test::src", "test::dst", None);
            test_support::observe_request(&engine, &first, request.clone()).await;
            registry.dispose(
                &registry.get("obs").unwrap(),
                observe_codes::MAX_FIRES,
                "the observer fired 1 times, its max_fires",
            );
            registry.set_config(long.clone());
            registry.connection_closed(first.id, first.reattach_token);
            tokio::time::sleep(Duration::from_millis(150)).await;
            assert!(
                registry
                    .tombstones
                    .get("obs")
                    .is_some_and(|tombstone| tombstone.until <= Instant::now()),
                "the tombstone expired"
            );
            if late {
                registry.grace_elapsed(first.id);
                assert!(registry.expired.contains_key(&first.id));
            }

            let (second, mut socket) = test_support::worker(&engine);
            let token = first.reattach_token.to_string();
            assert_eq!(
                registry
                    .reattach(&engine, first.id, Some(&token), &second)
                    .await,
                !late
            );
            received(&mut socket);
            registry.observe(&engine, &second, request).await;
            match received(&mut socket).as_slice() {
                [
                    Message::ObserveResult {
                        state: ObserverState::Disposed,
                        error: Some(error),
                        ..
                    },
                ] => assert_eq!(error.code, observe_codes::MAX_FIRES, "late: {late}"),
                other => panic!("late {late}: the disposal answers the replay, got {other:?}"),
            }
            assert!(registry.get("obs").is_none(), "the replay created nothing");
        }
    }

    /// A connection that reattached and holds neither observers nor
    /// tombstones leaves nothing behind when it closes.
    #[tokio::test]
    async fn a_reattached_connection_holding_nothing_leaves_no_record() {
        let engine = engine_with_functions();
        let registry = engine.observers.clone();
        let (first, _first_socket) = test_support::worker(&engine);
        let request = test_support::request("obs", "test::src", "test::dst", None);
        test_support::observe_request(&engine, &first, request).await;
        registry.connection_closed(first.id, first.reattach_token);
        let (second, _second_socket) = test_support::worker(&engine);
        let token = first.reattach_token.to_string();
        assert!(
            registry
                .reattach(&engine, first.id, Some(&token), &second)
                .await
        );
        assert!(registry.tombstoned.is_empty(), "no tombstone moved");
        registry.unobserve(second.id, "obs");
        assert!(registry.get("obs").is_none());
        registry.connection_closed(second.id, second.reattach_token);
        assert!(registry.departed.is_empty());
        assert!(registry.tombstoned.is_empty());
    }

    /// A replayed `observe` racing a failure of its observer is answered in
    /// the order of the two (decision 56): failed first, the replay is
    /// refused with the error, after the `observerstatechanged` that told
    /// it, and the engine holds no observer under the id; answered first,
    /// the `observeresult` has the state before the failure, the failure
    /// follows it, and the engine holds the observer in `error`. Threads
    /// pause at random inside the replay.
    #[test]
    fn a_replay_racing_a_failure_is_answered_in_their_order() {
        use std::sync::Barrier;

        use super::super::interleave;
        use crate::{
            engine::Outbound,
            protocol::{ErrorBody, Message},
        };

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let engine = runtime.block_on(async { engine_with_functions() });
        let registry = engine.observers.clone();
        let (worker, mut socket) = test_support::worker(&engine);
        for round in 0..300u64 {
            let id = format!("obs-{round}");
            let request = test_support::request(&id, "test::src", "test::dst", None);
            runtime.block_on(test_support::observe_request(
                &engine,
                &worker,
                request.clone(),
            ));
            while socket.try_recv().is_ok() {}
            let observer = registry.get(&id).unwrap();
            let barrier = Arc::new(Barrier::new(2));
            let replaying = {
                let (engine, worker, barrier) = (engine.clone(), worker.clone(), barrier.clone());
                let handle = runtime.handle().clone();
                std::thread::spawn(move || {
                    interleave::enable(0x9e37_79b9_7f4a_7c15 ^ (round + 1));
                    barrier.wait();
                    handle.block_on(engine.observers.observe(&engine, &worker, request));
                })
            };
            let failing = {
                let (registry, barrier) = (registry.clone(), barrier.clone());
                std::thread::spawn(move || {
                    barrier.wait();
                    for _ in 0..(round % 64) {
                        std::hint::spin_loop();
                    }
                    registry.fail(
                        &observer,
                        ErrorBody::new(observe_codes::OBSERVE_REJECTED, "rejected by its host"),
                    );
                })
            };
            replaying.join().unwrap();
            failing.join().unwrap();
            let mut answers = Vec::new();
            while let Ok(Outbound::Protocol(message)) = socket.try_recv() {
                match message {
                    Message::ObserveResult { state, .. } => answers.push(("result", state)),
                    Message::ObserverStateChanged { state, .. } => answers.push(("changed", state)),
                    _ => {}
                }
            }
            match answers.as_slice() {
                [
                    ("changed", ObserverState::Error),
                    ("result", ObserverState::Error),
                ] => {
                    assert!(registry.get(&id).is_none(), "{id}: refused, so not held");
                }
                [("result", answered), ("changed", ObserverState::Error)]
                    if *answered != ObserverState::Error =>
                {
                    assert_eq!(state(&engine, &id), Some(ObserverState::Error), "{id}");
                }
                other => panic!("{id}: answered out of order: {other:?}"),
            }
        }
    }

    /// An `observe` handled while its connection is being torn down joins
    /// the connection's other observers: suspended, waiting for a reattach.
    #[tokio::test]
    async fn an_observe_racing_its_connections_teardown_is_suspended_with_it() {
        let engine = engine_with_functions();
        let (worker, _socket) = test_support::worker(&engine);
        worker
            .cleanup_claimed
            .store(true, std::sync::atomic::Ordering::SeqCst);
        engine
            .observers
            .observe(
                &engine,
                &worker,
                test_support::request("obs", "test::src", "test::dst", None),
            )
            .await;
        assert_eq!(state(&engine, "obs"), Some(ObserverState::Suspended));
        assert_eq!(
            engine.observers.get("obs").unwrap().reason().unwrap().code,
            observe_codes::OBSERVER_DISCONNECTED
        );
    }

    /// A reattach and the end of the grace period exclude each other,
    /// whichever comes first.
    #[tokio::test]
    async fn a_reattach_and_the_grace_period_end_exclude_each_other() {
        let engine = engine_with_functions();
        let registry = engine.observers.clone();
        let (first, _first_socket) = test_support::worker(&engine);
        test_support::observe_request(
            &engine,
            &first,
            test_support::request("obs", "test::src", "test::dst", None),
        )
        .await;

        // Reattach first: the grace period ending late does nothing.
        registry.connection_closed(first.id, first.reattach_token);
        assert_eq!(state(&engine, "obs"), Some(ObserverState::Suspended));
        let (second, _second_socket) = test_support::worker(&engine);
        let token = first.reattach_token.to_string();
        assert!(
            registry
                .reattach(&engine, first.id, Some(&token), &second)
                .await
        );
        assert_eq!(state(&engine, "obs"), Some(ObserverState::Active));
        registry.grace_elapsed(first.id);
        assert_eq!(state(&engine, "obs"), Some(ObserverState::Active));
        assert_eq!(
            registry.get("obs").unwrap().observing().connection,
            second.id
        );

        // Grace first: a reattach that comes after finds nothing.
        registry.connection_closed(second.id, second.reattach_token);
        registry.grace_elapsed(second.id);
        assert!(registry.get("obs").is_none(), "disposed");
        let (third, _third_socket) = test_support::worker(&engine);
        let token = second.reattach_token.to_string();
        assert!(
            !registry
                .reattach(&engine, second.id, Some(&token), &third)
                .await
        );
    }

    /// A worker that reattaches after its grace period ended finds its
    /// observers disposed: its replay does not create them again with fresh
    /// counters. A wrong token neither learns that nor uses up the record.
    #[tokio::test]
    async fn a_late_reattach_keeps_the_observers_disposed() {
        let engine = engine_with_functions();
        let registry = engine.observers.clone();
        let (first, _first_socket) = test_support::worker(&engine);
        let mut request = test_support::request("obs", "test::src", "test::dst", None);
        request.max_fires = Some(3);
        test_support::observe_request(&engine, &first, request.clone()).await;
        registry.connection_closed(first.id, first.reattach_token);
        registry.grace_elapsed(first.id);
        assert!(registry.get("obs").is_none(), "disposed");

        let (intruder, _intruder_socket) = test_support::worker(&engine);
        assert!(
            !registry
                .reattach(&engine, first.id, Some("not-the-token"), &intruder)
                .await
        );
        assert!(
            registry
                .tombstones
                .get("obs")
                .is_none_or(|t| t.connection != intruder.id)
        );

        let (second, _second_socket) = test_support::worker(&engine);
        let token = first.reattach_token.to_string();
        assert!(
            !registry
                .reattach(&engine, first.id, Some(&token), &second)
                .await,
            "nothing to re-parent"
        );
        registry.observe(&engine, &second, request).await;
        assert!(
            registry.get("obs").is_none(),
            "the replay did not create it"
        );
        let tombstone = registry.tombstones.get("obs").unwrap();
        assert_eq!(tombstone.connection, second.id);
        assert_eq!(tombstone.reason.code, observe_codes::OBSERVER_DISCONNECTED);
    }

    /// A worker's disconnect takes its observers out of their function's
    /// active set in one change: rebuilt from the observers it keeps when the
    /// worker had many of them, one observer at a time (`O(log n)` each) when
    /// it had a few. Either way what each suspension copies does not grow
    /// with the function's observers. The end of the grace period disposes
    /// each without copying any set. Rebuilding the set for each would copy
    /// every observer, 16 times as many at 4,000 as at 250.
    #[tokio::test]
    async fn a_workers_observers_leave_their_active_set_in_one_change() {
        use super::super::work;
        /// Slots copied per suspension and per disposal when a worker with
        /// `leaving` of a function's observers leaves, another worker holding
        /// `staying`.
        async fn per_observer(leaving: usize, staying: usize) -> (f64, f64) {
            let engine = engine_with_functions();
            engine.observers.set_config(ObserversConfig {
                observer_grace_ms: 600_000,
                max_per_connection: leaving.max(staying),
                max_per_function: leaving + staying,
                ..ObserversConfig::default()
            });
            let (worker, mut socket) = test_support::worker(&engine);
            let drain = tokio::spawn(async move { while socket.recv().await.is_some() {} });
            let (other, mut other_socket) = test_support::worker(&engine);
            let other_drain =
                tokio::spawn(async move { while other_socket.recv().await.is_some() {} });
            for i in 0..leaving.max(staying) {
                for (observing, count, prefix) in
                    [(&worker, leaving, "obs"), (&other, staying, "kept")]
                {
                    if i < count {
                        let id = format!("{prefix}-{i}");
                        let request = test_support::request(&id, "test::src", "test::dst", None);
                        engine.observers.observe(&engine, observing, request).await;
                    }
                }
            }
            let registry = engine.observers.clone();
            assert_eq!(
                registry.count_in_state(&ObserverState::Active),
                (leaving + staying) as i64
            );
            work::take();
            registry.connection_closed(worker.id, worker.reattach_token);
            let suspended = work::take();
            assert_eq!(
                registry.count_in_state(&ObserverState::Suspended),
                leaving as i64
            );
            let kept: Vec<String> = registry
                .watch("default", "test::src")
                .map(|set| set.observers.iter().map(|o| o.id.clone()).collect())
                .unwrap_or_default();
            let expected: Vec<String> = (0..staying).map(|i| format!("kept-{i}")).collect();
            assert_eq!(kept, expected, "the other worker's, in creation order");
            registry.grace_elapsed(worker.id);
            let disposed = work::take();
            assert_eq!(registry.len(), staying);
            drain.abort();
            other_drain.abort();
            (
                suspended as f64 / leaving as f64,
                disposed as f64 / leaving as f64,
            )
        }
        for (leaving_small, staying_small, leaving_large, staying_large) in [
            (250, 250, 4_000, 4_000),
            (8, 250, 8, 4_000),
            (250, 0, 4_000, 0),
        ] {
            let (small, small_disposed) = per_observer(leaving_small, staying_small).await;
            let (large, large_disposed) = per_observer(leaving_large, staying_large).await;
            assert!(
                large <= 3.0 * small.max(1.0) && large < 256.0,
                "slots copied per suspension: {small:.1} with {leaving_small} of {} observers, \
                 {large:.1} with {leaving_large} of {}",
                leaving_small + staying_small,
                leaving_large + staying_large,
            );
            assert_eq!((small_disposed, large_disposed), (0.0, 0.0));
        }
    }

    /// A worker's `observe` and `unobserve` cost `O(log n)` each in its
    /// function's `n` observers: neither scans them (to count them against
    /// `max_per_function`, or to find the one leaving), so what each copies
    /// is at most the nodes on one path of the function's active set, a few
    /// hundred slots, not its size. A scan of the function's observers on
    /// each would visit every one, 4,000 at 4,000.
    #[tokio::test]
    async fn a_workers_observe_and_unobserve_cost_log_n_each() {
        use super::super::work;
        async fn per_change(n: usize) -> (f64, f64) {
            let engine = engine_with_functions();
            engine.observers.set_config(ObserversConfig {
                observer_grace_ms: 600_000,
                max_per_connection: n,
                max_per_function: n,
                ..ObserversConfig::default()
            });
            let (worker, mut socket) = test_support::worker(&engine);
            let drain = tokio::spawn(async move { while socket.recv().await.is_some() {} });
            let registry = engine.observers.clone();
            work::take();
            for i in 0..n {
                let request =
                    test_support::request(&format!("obs-{i}"), "test::src", "test::dst", None);
                registry.observe(&engine, &worker, request).await;
            }
            let observed = work::take();
            assert_eq!(registry.count_in_state(&ObserverState::Active), n as i64);
            for i in 0..n {
                registry.unobserve(worker.id, &format!("obs-{i}"));
            }
            let unobserved = work::take();
            assert!(registry.is_empty());
            assert!(registry.watch("default", "test::src").is_none());
            drain.abort();
            (observed as f64 / n as f64, unobserved as f64 / n as f64)
        }
        let (small, small_unobserve) = per_change(250).await;
        let (large, large_unobserve) = per_change(4_000).await;
        assert!(
            small < 256.0 && large < 256.0,
            "slots per observe: {small:.1} with 250 observers, {large:.1} with 4,000"
        );
        assert!(
            small_unobserve < 256.0 && large_unobserve < 256.0,
            "slots per unobserve: {small_unobserve:.1} with 250 observers, \
             {large_unobserve:.1} with 4,000"
        );
    }

    /// What the connection and target indexes hold that is not a live
    /// observer under its own connection and target.
    fn stale_index_entries(registry: &super::ObserverRegistry) -> Vec<String> {
        let live = |held: &super::Held| {
            registry
                .by_id
                .get(&held.0.id)
                .is_some_and(|current| Arc::ptr_eq(&current, &held.0))
        };
        let mut stale = Vec::new();
        for entry in registry.by_connection.iter() {
            for held in entry.value() {
                if !live(held) || held.0.observing_connection() != *entry.key() {
                    stale.push(format!("{} under connection {}", held.0.id, entry.key()));
                }
            }
        }
        for entry in registry.by_target.iter() {
            for held in entry.value() {
                if !live(held) || held.0.target != *entry.key() {
                    stale.push(format!("{} under target {}", held.0.id, entry.key().1));
                }
            }
        }
        let indexed: usize = registry.by_connection.iter().map(|e| e.value().len()).sum();
        if indexed != registry.len() {
            stale.push(format!(
                "{indexed} observers by connection, {} by id",
                registry.len()
            ));
        }
        stale
    }

    /// Every connection and target index entry is a live observer under its
    /// own connection and target, whatever lands between an observer's
    /// publish and its disposal. Two threads create observers under fresh
    /// ids, each on one of two targets, pausing at random inside publish;
    /// two others dispose of the newest ones as soon as they appear. A stale
    /// entry under a connection would count against `max_per_connection` for
    /// good; one under a target could make an observer that holds the id
    /// later, on another target, available.
    #[test]
    fn index_entries_are_live_observers_under_random_interleavings() {
        use std::sync::atomic::{AtomicBool, AtomicU64};

        use super::super::{EngineObserver, interleave};
        use crate::protocol::{DEFAULT_NAMESPACE, Deliver};
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let _entered = runtime.enter();
        let engine = Arc::new(Engine::new());
        let _source = test_support::capture(&engine, "test::src");
        let _targets = [
            test_support::capture(&engine, "test::dst-0"),
            test_support::capture(&engine, "test::dst-1"),
        ];
        let next = Arc::new(AtomicU64::new(0));
        let creating = Arc::new(AtomicBool::new(true));
        let creators: Vec<_> = (0..2u64)
            .map(|thread| {
                let (engine, next) = (engine.clone(), next.clone());
                let handle = runtime.handle().clone();
                std::thread::spawn(move || {
                    let _entered = handle.enter();
                    interleave::enable(0x2545_f491_4f6c_dd1d ^ (thread + 1));
                    for _ in 0..2_500 {
                        let n = next.fetch_add(1, Ordering::AcqRel);
                        let _ = engine.observers.observe_as_engine(EngineObserver {
                            id: format!("race-{n}"),
                            function: (DEFAULT_NAMESPACE.into(), "test::src".into()),
                            args: serde_json::json!({}),
                            filters: None,
                            target: (DEFAULT_NAMESPACE.into(), format!("test::dst-{}", n % 2)),
                            deliver: Deliver::Input,
                            metadata: None,
                        });
                    }
                })
            })
            .collect();
        let disposers: Vec<_> = (0..2u64)
            .map(|thread| {
                let (engine, next, creating) = (engine.clone(), next.clone(), creating.clone());
                std::thread::spawn(move || {
                    while creating.load(Ordering::Acquire) {
                        let newest = next.load(Ordering::Acquire);
                        for back in 1..=2 {
                            if let Some(n) = newest.checked_sub(back + thread) {
                                engine.observers.unobserve_as_engine(&format!("race-{n}"));
                            }
                        }
                        std::thread::yield_now();
                    }
                })
            })
            .collect();
        for thread in creators {
            thread.join().unwrap();
        }
        creating.store(false, Ordering::Release);
        for thread in disposers {
            thread.join().unwrap();
        }
        let registry = &engine.observers;
        let stale = stale_index_entries(registry);
        assert!(stale.is_empty(), "stale index entries: {stale:?}");
        for n in 0..next.load(Ordering::Acquire) {
            registry.unobserve_as_engine(&format!("race-{n}"));
        }
        assert!(registry.is_empty());
        assert!(registry.by_connection.is_empty(), "nothing by connection");
        assert!(registry.by_target.is_empty(), "nothing by target");
    }

    /// An engine whose observers wait `grace_ms` for their worker, with one
    /// active observer `obs` of `test::src` from the returned worker. The
    /// drop-export task runs: the worker's `observe` started it.
    async fn observing_with_grace(
        grace_ms: u64,
    ) -> (
        Arc<Engine>,
        crate::worker_connections::WorkerConnection,
        tokio::sync::mpsc::Receiver<crate::engine::Outbound>,
    ) {
        let engine = engine_with_functions();
        engine.observers.set_config(ObserversConfig {
            observer_grace_ms: grace_ms,
            ..(*engine.observers.config()).clone()
        });
        let (worker, socket) = test_support::worker(&engine);
        test_support::observe_request(
            &engine,
            &worker,
            test_support::request("obs", "test::src", "test::dst", None),
        )
        .await;
        assert!(engine.observers.drop_export.load(Ordering::Acquire));
        (engine, worker, socket)
    }

    /// The drop-export task ends a grace period at its deadline, longer than
    /// the task's interval or shorter: the disconnect starts no timer of its
    /// own.
    #[tokio::test(start_paused = true)]
    async fn the_drop_export_task_ends_a_grace_period_at_its_deadline() {
        use std::time::Duration;
        for grace_ms in [30_000, 100] {
            let (engine, worker, _socket) = observing_with_grace(grace_ms).await;
            let registry = engine.observers.clone();
            registry.connection_closed(worker.id, worker.reattach_token);
            assert!(
                registry
                    .departed
                    .get(&worker.id)
                    .is_some_and(|departed| departed.timer.is_none()),
                "no timer of its own"
            );
            tokio::time::sleep(Duration::from_millis(grace_ms - 1)).await;
            assert_eq!(state(&engine, "obs"), Some(ObserverState::Suspended));
            tokio::time::sleep(Duration::from_millis(2)).await;
            assert!(
                registry.get("obs").is_none(),
                "disposed after {grace_ms} ms"
            );
            assert!(registry.departed.is_empty());
        }
    }

    /// A second close of a connection starts a grace period of its own: the
    /// first one's deadline passes with the observers still suspended,
    /// whether the drop-export task or the connection's own timer ends it.
    #[tokio::test(start_paused = true)]
    async fn a_second_close_starts_a_grace_period_of_its_own() {
        use std::time::Duration;
        for swept in [true, false] {
            let (engine, worker, _socket) = observing_with_grace(300).await;
            let registry = engine.observers.clone();
            if !swept {
                registry.drop_export.store(false, Ordering::Release);
            }
            registry.connection_closed(worker.id, worker.reattach_token);
            tokio::time::sleep(Duration::from_millis(200)).await;
            registry.connection_closed(worker.id, worker.reattach_token);
            assert_eq!(
                registry
                    .departed
                    .get(&worker.id)
                    .is_some_and(|departed| departed.timer.is_some()),
                !swept
            );
            tokio::time::sleep(Duration::from_millis(150)).await;
            assert_eq!(state(&engine, "obs"), Some(ObserverState::Suspended));
            tokio::time::sleep(Duration::from_millis(200)).await;
            assert!(registry.get("obs").is_none(), "disposed");
        }
    }

    /// A worker that reattaches within its grace period keeps its observers
    /// past the deadline, whether the drop-export task or the connection's
    /// own timer would have ended it.
    #[tokio::test(start_paused = true)]
    async fn a_reattach_within_the_grace_period_outlives_its_deadline() {
        use std::time::Duration;
        for swept in [true, false] {
            let (engine, first, _first_socket) = observing_with_grace(100).await;
            let registry = engine.observers.clone();
            if !swept {
                registry.drop_export.store(false, Ordering::Release);
            }
            registry.connection_closed(first.id, first.reattach_token);
            tokio::time::sleep(Duration::from_millis(50)).await;
            let (second, _second_socket) = test_support::worker(&engine);
            let token = first.reattach_token.to_string();
            assert!(
                registry
                    .reattach(&engine, first.id, Some(&token), &second)
                    .await
            );
            tokio::time::sleep(Duration::from_millis(200)).await;
            assert_eq!(state(&engine, "obs"), Some(ObserverState::Active));
            assert_eq!(
                registry.get("obs").unwrap().observing().connection,
                second.id
            );
        }
    }

    /// A drop-export task whose runtime shuts down before polling it ends
    /// all the same: the next `observe`, on another runtime, starts the task
    /// again, and it ends the grace period of a disconnect there.
    #[test]
    fn a_drop_export_task_dropped_unpolled_leaves_the_next_observe_to_start_one() {
        use std::time::Duration;
        let first = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let engine = {
            let _entered = first.enter();
            crate::workers::observability::metrics::ensure_default_meter();
            let engine = engine_with_functions();
            engine.observers.remember_runtime();
            assert!(engine.observers.drop_export.load(Ordering::Acquire));
            engine
        };
        drop(first);
        assert!(!engine.observers.drop_export.load(Ordering::Acquire));
        engine.observers.set_config(ObserversConfig {
            observer_grace_ms: 100,
            ..(*engine.observers.config()).clone()
        });
        let second = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .start_paused(true)
            .build()
            .unwrap();
        second.block_on(async {
            let (worker, _socket) = test_support::worker(&engine);
            test_support::observe_request(
                &engine,
                &worker,
                test_support::request("obs", "test::src", "test::dst", None),
            )
            .await;
            assert!(engine.observers.drop_export.load(Ordering::Acquire));
            engine
                .observers
                .connection_closed(worker.id, worker.reattach_token);
            assert!(
                engine
                    .observers
                    .departed
                    .get(&worker.id)
                    .is_some_and(|departed| departed.timer.is_none()),
                "the task ends it"
            );
            tokio::time::sleep(Duration::from_millis(101)).await;
            assert!(engine.observers.get("obs").is_none(), "disposed");
            assert!(engine.observers.departed.is_empty());
        });
    }

    /// A function whose last active observer leaves stays in the routed
    /// view, found by nobody: the view is not copied, and the function's
    /// next active observer takes the same entry. A copy of the view, when
    /// another function joins it, leaves the function out.
    #[tokio::test]
    async fn a_function_without_active_observers_keeps_its_routed_entry_unfound() {
        use crate::function::FunctionKey;
        let engine = engine_with_functions();
        let registry = engine.observers.clone();
        let entry = |function: &str| {
            registry
                .routed
                .load()
                .get(&("default", function) as &dyn FunctionKey)
                .cloned()
        };
        let (first, _first_socket) = test_support::worker(&engine);
        test_support::observe_request(
            &engine,
            &first,
            test_support::request("obs", "test::src", "test::dst", None),
        )
        .await;
        let held = entry("test::src").expect("in the view");
        let view = registry.routed.load_full();

        registry.connection_closed(first.id, first.reattach_token);
        assert!(
            Arc::ptr_eq(&registry.routed.load_full(), &view),
            "not copied"
        );
        assert!(!registry.is_observed("default", "test::src"));
        assert!(registry.watch("default", "test::src").is_none());
        assert!(matches!(
            registry.route("default", "test::src", None),
            Ok(None)
        ));
        assert!(!registry.hosts_privately("default", "test::src", first.id));
        assert_eq!(registry.routed_live.load(Ordering::Acquire), 0);

        let (second, _second_socket) = test_support::worker(&engine);
        let token = first.reattach_token.to_string();
        assert!(
            registry
                .reattach(&engine, first.id, Some(&token), &second)
                .await
        );
        assert_eq!(state(&engine, "obs"), Some(ObserverState::Active));
        assert!(
            Arc::ptr_eq(&registry.routed.load_full(), &view),
            "not copied"
        );
        assert!(Arc::ptr_eq(&entry("test::src").unwrap(), &held));
        assert!(registry.is_observed("default", "test::src"));
        assert_eq!(registry.routed_live.load(Ordering::Acquire), 1);

        registry.unobserve(second.id, "obs");
        test_support::observe_request(
            &engine,
            &second,
            test_support::request("back", "test::dst", "test::src", None),
        )
        .await;
        assert!(entry("test::src").is_none(), "left out of the copy");
        assert!(registry.is_observed("default", "test::dst"));
        assert_eq!(registry.routed.load().len(), 1);
        assert_eq!(registry.routed_live.load(Ordering::Acquire), 1);
    }

    /// With `disposed_retention_ms: 0` nothing is remembered past the grace
    /// period: a late replay creates the observer again, as after an engine
    /// restart.
    #[tokio::test]
    async fn without_retention_a_late_replay_creates_the_observer_again() {
        let engine = engine_with_functions();
        let registry = engine.observers.clone();
        registry.set_config(ObserversConfig {
            disposed_retention_ms: 0,
            ..(*registry.config()).clone()
        });
        let (first, _first_socket) = test_support::worker(&engine);
        let request = test_support::request("obs", "test::src", "test::dst", None);
        test_support::observe_request(&engine, &first, request.clone()).await;
        registry.connection_closed(first.id, first.reattach_token);
        registry.grace_elapsed(first.id);

        let (second, _second_socket) = test_support::worker(&engine);
        let token = first.reattach_token.to_string();
        registry
            .reattach(&engine, first.id, Some(&token), &second)
            .await;
        test_support::observe_request(&engine, &second, request).await;
        assert_eq!(
            registry.get("obs").unwrap().observing().connection,
            second.id
        );
    }
}

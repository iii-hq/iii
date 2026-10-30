// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Observers the engine creates for its own modules: telemetry's queue topics
//! (`queue::on-message`), the Console UI assets of `iii-observability`
//! (`console::ui::asset`).
//!
//! - The engine is the observing party ([`ENGINE_CONNECTION`],
//!   `worker_name: "iii-engine"`): such an observer never hears
//!   `observeresult` or `observerstatechanged`, is never suspended, and
//!   counts against neither `observers.max_per_connection` nor
//!   `observers.max_per_function`.
//! - Everything else is an observer like any other: the function's host
//!   learns it in `functionregistered` / `observeradded`, validates its args
//!   and acks it, and calls through it.
//! - A standing observer ([`ObserverRegistry::observe_everywhere`]) is kept in
//!   every namespace that registers its function, since a worker hosts its
//!   functions in its own namespace.

use std::{
    sync::{Arc, atomic::Ordering},
    time::Instant,
};

use iii_filter::Filter;
use serde_json::{Map, Value};
use uuid::Uuid;

use super::{
    FnKey, ObserverRegistry,
    context::ENGINE_CALLER_NAME,
    lifecycle::{CaptureSet, Flags, NewObserver, ObserveSpec, Observer, Observing, Policy},
    owner::check_options,
};
use crate::protocol::{
    DEFAULT_NAMESPACE, Deliver, ErrorBody, ObserverState, On, default_capture, observe_codes,
};

/// The observing "connection" of every engine-owned observer.
pub const ENGINE_CONNECTION: Uuid = Uuid::nil();

/// An observer the engine creates: `function` observed with `args` and
/// `filters`, `target` invoked with what `deliver` names and `metadata` as
/// the sidecar.
#[derive(Debug, Clone, PartialEq)]
pub struct EngineObserver {
    pub id: String,
    pub function: FnKey,
    pub args: Value,
    pub filters: Option<Value>,
    pub target: FnKey,
    pub deliver: Deliver,
    pub metadata: Option<Value>,
}

/// An engine-owned observer of `function_id` kept in every namespace that
/// registers it, with its target in `default`.
#[derive(Debug, Clone, PartialEq)]
pub struct StandingObserver {
    /// The id in `default`; in any other namespace `{id}@{namespace}`.
    pub id: String,
    pub function_id: String,
    pub args: Value,
    pub target_id: String,
    pub deliver: Deliver,
    pub metadata: Option<Value>,
}

impl StandingObserver {
    /// The observer this standing observer keeps in `namespace`.
    pub fn in_namespace(&self, namespace: &str) -> EngineObserver {
        let id = if namespace == DEFAULT_NAMESPACE {
            self.id.clone()
        } else {
            format!("{}@{namespace}", self.id)
        };
        EngineObserver {
            id,
            function: (namespace.to_string(), self.function_id.clone()),
            args: self.args.clone(),
            filters: None,
            target: (DEFAULT_NAMESPACE.to_string(), self.target_id.clone()),
            deliver: self.deliver.clone(),
            metadata: self.metadata.clone(),
        }
    }
}

impl Observer {
    /// Whether the engine created this observer for one of its own modules.
    pub fn is_engine_owned(&self) -> bool {
        self.observing().connection == ENGINE_CONNECTION
    }
}

impl ObserverRegistry {
    /// Creates `request` as an engine-owned observer. Idempotent: an
    /// engine-owned observer with the same id and spec is left as it is,
    /// unless it failed (its host rejected it, or never acked it), which
    /// makes a new one; one with another spec is replaced. An `Err` is a
    /// refusal: the id belongs to a worker's observer, the observer targets
    /// its own function or a private one, its filters do not parse, or the
    /// function is registered with options that refuse it.
    pub fn observe_as_engine(self: &Arc<Self>, request: EngineObserver) -> Result<(), ErrorBody> {
        self.remember_runtime();
        let EngineObserver {
            id,
            function,
            args,
            filters,
            target,
            deliver,
            metadata,
        } = request;
        if function == target {
            return Err(ErrorBody::new(
                observe_codes::SELF_TARGET,
                format!("`{}` cannot be the target of its own observer", function.1),
            ));
        }
        self.check_target_invocable(&target)?;
        let filter = match &filters {
            Some(src) => {
                Filter::parse(src).map_err(|err| ErrorBody::new(err.code.as_str(), err.message))?
            }
            None => Filter::match_all(),
        };
        if self.is_registered(&function) {
            let options = self.function_entry(&function).and_then(|e| e.options());
            check_options(options.as_ref(), &function, &args)?;
        }
        let settings = self.config();
        let capture = CaptureSet::from_list(&default_capture());
        let spec = ObserveSpec {
            function: function.clone(),
            target: target.clone(),
            args: args.clone(),
            filters: filter.normalized().clone(),
            capture: capture.to_list(),
            on: On::default(),
            deliver: deliver.clone(),
            metadata: metadata.clone(),
            max_fires: None,
            expires_in_ms: None,
            max_in_flight: settings.default_max_in_flight,
        };

        if let Some(existing) = self.get(&id) {
            if !existing.is_engine_owned() {
                return Err(ErrorBody::new(
                    observe_codes::OBSERVER_ID_CONFLICT,
                    format!("observer `{id}` belongs to a worker"),
                ));
            }
            if existing.spec == spec && existing.state() != ObserverState::Error {
                return Ok(());
            }
            self.dispose(
                &existing,
                observe_codes::UNOBSERVED,
                "replaced by the engine",
            );
        }
        self.tombstones.remove(&id);

        let observer = Arc::new(Observer::new(NewObserver {
            id: id.clone(),
            target: target.clone(),
            spec,
            args,
            filter,
            capture,
            on: On::default(),
            deliver,
            metadata,
            policy: Policy {
                max_fires: None,
                expires_in_ms: None,
                expires_at: None::<Instant>,
                max_in_flight: settings.default_max_in_flight.max(1),
                max_queue: settings.max_queue_per_observer,
            },
            warnings: Vec::new(),
            observing: Observing {
                connection: ENGINE_CONNECTION,
                worker_name: Some(ENGINE_CALLER_NAME.to_string()),
                namespace: DEFAULT_NAMESPACE.to_string(),
                session: None,
            },
            flags: Flags {
                observing_connected: true,
                function_available: self.is_registered(&function),
                target_available: self.is_registered(&target),
                owner_acked: self.acked_by_default(&function),
                announced: None,
                rejected_by_owner: false,
                activated_once: false,
                failed: None,
                disposed: None,
            },
            function,
        }));
        if !self.publish(&observer, ENGINE_CONNECTION, Vec::new()) {
            return Err(ErrorBody::new(
                observe_codes::OBSERVER_ID_CONFLICT,
                format!("observer `{id}` was created concurrently"),
            ));
        }
        let function_available = self.is_registered(&observer.function);
        let target_available = self.is_registered(&observer.target);
        self.update(&observer, |flags| {
            flags.function_available = function_available;
            flags.target_available = target_available;
        });
        tracing::debug!(
            observer_id = %id,
            function_id = %observer.function.1,
            namespace = %observer.function.0,
            target = %observer.target.1,
            "engine-owned observer created"
        );
        Ok(())
    }

    /// Disposes the engine-owned observer `id`. Returns whether there was
    /// one.
    pub fn unobserve_as_engine(&self, id: &str) -> bool {
        match self.get(id) {
            Some(observer) if observer.is_engine_owned() => {
                self.dispose(
                    &observer,
                    observe_codes::UNOBSERVED,
                    "disposed by the engine",
                );
                true
            }
            _ => false,
        }
    }

    /// Keeps `standing` in every namespace that registers its function: now
    /// in each one that does, and from now on in each one that registers it
    /// later (see [`Self::on_function_change`]). Calling it again with the
    /// same id replaces the earlier one for the observers made from now on.
    pub fn observe_everywhere(self: &Arc<Self>, standing: StandingObserver) {
        self.remember_runtime();
        let namespaces: Vec<String> = self
            .deps
            .get()
            .map(|deps| {
                deps.functions
                    .functions
                    .iter()
                    .filter(|entry| entry.key().1 == standing.function_id)
                    .map(|entry| entry.key().0.clone())
                    .collect()
            })
            .unwrap_or_default();
        {
            let mut all = super::lifecycle::lock(&self.standing);
            all.retain(|existing| existing.id != standing.id);
            all.push(standing.clone());
        }
        self.standing_count.store(
            super::lifecycle::lock(&self.standing).len(),
            Ordering::Release,
        );
        for namespace in namespaces {
            self.keep_standing(&standing, &namespace);
        }
    }

    /// `(namespace, function_id)` was registered: the standing observers of
    /// `function_id` are made there, off the registration's own path (it may
    /// hold the locks an observer's creation takes). One atomic load when
    /// there are none.
    pub(super) fn standing_on_registration(&self, namespace: &str, function_id: &str) {
        if self.standing_count.load(Ordering::Acquire) == 0 {
            return;
        }
        let matching: Vec<StandingObserver> = super::lifecycle::lock(&self.standing)
            .iter()
            .filter(|standing| standing.function_id == function_id)
            .cloned()
            .collect();
        if matching.is_empty() {
            return;
        }
        let (Some(registry), Some(runtime)) = (self.this(), self.runtime()) else {
            return;
        };
        let namespace = namespace.to_string();
        runtime.spawn(async move {
            for standing in &matching {
                registry.keep_standing(standing, &namespace);
            }
        });
    }

    fn keep_standing(self: &Arc<Self>, standing: &StandingObserver, namespace: &str) {
        let request = standing.in_namespace(namespace);
        let id = request.id.clone();
        if let Err(error) = self.observe_as_engine(request) {
            tracing::warn!(
                observer_id = %id,
                namespace = %namespace,
                function_id = %standing.function_id,
                code = %error.code,
                message = %error.message,
                "the engine could not observe a function it consumes"
            );
        }
    }
}

/// `{}`: an engine-owned observer of a function without args.
pub fn no_args() -> Value {
    Value::Object(Map::new())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::json;

    use super::*;
    use crate::{
        engine::{Engine, EngineTrait},
        observer::events::test_support,
        protocol::observe_codes,
    };

    async fn until(mut done: impl FnMut() -> bool) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while !done() {
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .expect("in time");
    }

    fn standing() -> StandingObserver {
        StandingObserver {
            id: "engine::test-watch".to_string(),
            function_id: "test::source".to_string(),
            args: no_args(),
            target_id: "test::sink".to_string(),
            deliver: Deliver::Input,
            metadata: None,
        }
    }

    /// An engine-owned observer activates its target like any observer, is
    /// left alone when asked for again, and counts against no limit.
    #[tokio::test]
    async fn an_engine_owned_observer_activates_its_target() {
        let engine = Arc::new(Engine::new());
        let mut config = (*engine.observers.config()).clone();
        config.max_per_function = 1;
        engine.observers.set_config(config);
        let _source = test_support::capture(&engine, "test::source");
        let mut sink = test_support::capture(&engine, "test::sink");
        let request = standing().in_namespace(DEFAULT_NAMESPACE);
        engine.observers.observe_as_engine(request.clone()).unwrap();
        let _socket =
            test_support::observe(&engine, "worker-obs", "test::source", "test::sink", None).await;
        engine.observers.observe_as_engine(request).unwrap();

        let observer = engine.observers.get("engine::test-watch").unwrap();
        assert!(observer.is_engine_owned());
        assert_eq!(observer.state(), ObserverState::Active);
        engine
            .call("test::source", json!({ "n": 1 }))
            .await
            .expect("called");
        assert_eq!(test_support::next(&mut sink).await, json!({ "n": 1 }));

        assert!(engine.observers.unobserve_as_engine("engine::test-watch"));
        assert!(!engine.observers.unobserve_as_engine("worker-obs"));
    }

    /// An id a worker's observer holds, a self target and a private target
    /// are refused.
    #[tokio::test]
    async fn an_engine_owned_observer_is_validated() {
        let engine = Arc::new(Engine::new());
        let _source = test_support::capture(&engine, "test::source");
        let _sink = test_support::capture(&engine, "test::sink");
        let _socket =
            test_support::observe(&engine, "worker-obs", "test::source", "test::sink", None).await;
        let mut request = standing().in_namespace(DEFAULT_NAMESPACE);
        request.id = "worker-obs".to_string();
        assert_eq!(
            engine
                .observers
                .observe_as_engine(request)
                .unwrap_err()
                .code,
            observe_codes::OBSERVER_ID_CONFLICT
        );

        let mut request = standing().in_namespace(DEFAULT_NAMESPACE);
        request.target = request.function.clone();
        assert_eq!(
            engine
                .observers
                .observe_as_engine(request)
                .unwrap_err()
                .code,
            observe_codes::SELF_TARGET
        );

        engine.observers.set_options(
            DEFAULT_NAMESPACE,
            "test::sink",
            crate::observer::FunctionOptions::private(),
        );
        assert_eq!(
            engine
                .observers
                .observe_as_engine(standing().in_namespace(DEFAULT_NAMESPACE))
                .unwrap_err()
                .code,
            observe_codes::FORBIDDEN
        );
    }

    /// A standing observer is made in every namespace that registers its
    /// function, now and later, under a namespaced id.
    #[tokio::test]
    async fn a_standing_observer_follows_its_function_into_every_namespace() {
        let engine = Arc::new(Engine::new());
        let _source = test_support::capture(&engine, "test::source");
        let _sink = test_support::capture(&engine, "test::sink");
        engine.observers.observe_everywhere(standing());
        assert!(engine.observers.get("engine::test-watch").is_some());
        assert!(engine.observers.get("engine::test-watch@team").is_none());

        engine.register_function_handler_ns(
            "team",
            crate::engine::RegisterFunctionRequest {
                function_id: "test::source".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            crate::engine::Handler::new(|input: Value| async move {
                crate::function::FunctionResult::Success(Some(input))
            }),
        );
        until(|| engine.observers.get("engine::test-watch@team").is_some()).await;
        let observer = engine.observers.get("engine::test-watch@team").unwrap();
        assert_eq!(observer.function.0, "team");
        assert_eq!(observer.target.0, DEFAULT_NAMESPACE);
        assert!(observer.is_engine_owned());
    }
}

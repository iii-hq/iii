// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! `engine::observers::list | get | dispose` (protocol.md §3.6).
//!
//! What a caller may see is gated like an invocation: an observer is visible
//! when the caller's session may invoke the observed function. Disposing one
//! takes more: the observing worker itself, or an unrestricted caller (one
//! with no RBAC in force: an in-process engine call, or a connection to a
//! worker manager without an `rbac` section, which is trusted with every
//! function already).

use std::{collections::BTreeMap, sync::Arc};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{ObserverRegistry, lifecycle::Observer};
use crate::{
    engine::Engine,
    protocol::{ErrorBody, observe_codes},
    workers::worker::rbac_session::Session,
};

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct ListObserversInput {
    /// Only observers of this function.
    #[serde(default)]
    pub function_id: Option<String>,
    /// Only observers of functions in this namespace.
    #[serde(default)]
    pub namespace: Option<String>,
    /// Only observers created by this worker (its name or its connection id).
    #[serde(default)]
    pub worker: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ObserverIdInput {
    /// The observer id.
    pub id: String,
    /// Injected by the engine from the calling worker.
    #[serde(rename = "_caller_worker_id", default)]
    pub caller_worker_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ObserverTargetInfo {
    pub function_id: String,
    pub namespace: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ObservingWorkerInfo {
    /// The observing connection's id; the nil UUID for an observer the
    /// engine owns.
    pub worker_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worker_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ReasonInfo {
    pub code: String,
    pub message: String,
}

/// One observer, as `engine::observers::list` and `get` report it. The
/// activation metadata is not included.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ObserverInfo {
    pub id: String,
    /// The observed function.
    pub function_id: String,
    pub namespace: String,
    pub args: Value,
    /// The observer's filters, normalized.
    pub filters: Value,
    pub target: ObserverTargetInfo,
    pub capture: Vec<String>,
    pub on: String,
    pub deliver: String,
    pub worker: ObservingWorkerInfo,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<ReasonInfo>,
    pub fires: u64,
    /// Dropped events by drop reason; only non-zero counts.
    pub dropped: BTreeMap<String, u64>,
    /// Activations that started and did not complete, by reason (`TIMEOUT`,
    /// `TARGET_UNAVAILABLE`, `TARGET_ERROR`); only non-zero counts.
    pub activation_failures: BTreeMap<String, u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_fires: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_in_ms: Option<u64>,
    pub max_in_flight: u32,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ObserversListResult {
    pub observers: Vec<ObserverInfo>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DisposeObserverResult {
    /// Whether an observer with this id existed and was disposed.
    pub disposed: bool,
}

impl ObserverRegistry {
    fn info(&self, observer: &Observer) -> ObserverInfo {
        let (state, reason) = observer.snapshot_state();
        let observing = observer.observing();
        ObserverInfo {
            id: observer.id.clone(),
            function_id: observer.function.1.clone(),
            namespace: observer.function.0.clone(),
            args: observer.args.clone(),
            filters: observer.filter.normalized().clone(),
            target: ObserverTargetInfo {
                function_id: observer.target.1.clone(),
                namespace: observer.target.0.clone(),
            },
            capture: observer
                .capture
                .to_list()
                .iter()
                .map(|c| c.to_string())
                .collect(),
            on: observer.on.to_string(),
            deliver: observer.deliver.to_string(),
            worker: ObservingWorkerInfo {
                worker_id: observing.connection.to_string(),
                worker_name: observing.worker_name,
            },
            state: state.to_string(),
            reason: reason.map(|r| ReasonInfo {
                code: r.code,
                message: r.message,
            }),
            fires: observer.fires(),
            dropped: observer.drops.snapshot(),
            activation_failures: observer.failures.snapshot(),
            max_fires: observer.policy.max_fires,
            expires_in_ms: observer.policy.expires_in_ms,
            max_in_flight: observer.policy.max_in_flight,
            created_at_ms: observer.created_at_ms,
        }
    }

    fn visible(engine: &Engine, session: Option<&Arc<Session>>, observer: &Observer) -> bool {
        session.is_none_or(|session| {
            engine.is_function_allowed(session, Some(&observer.function.0), &observer.function.1)
        })
    }

    /// `engine::observers::list`.
    pub fn list_infos(
        &self,
        engine: &Engine,
        session: Option<&Arc<Session>>,
        input: &ListObserversInput,
    ) -> ObserversListResult {
        let mut observers: Vec<ObserverInfo> = self
            .observers()
            .iter()
            .filter(|o| {
                input
                    .function_id
                    .as_ref()
                    .is_none_or(|f| *f == o.function.1)
            })
            .filter(|o| input.namespace.as_ref().is_none_or(|n| *n == o.function.0))
            .filter(|o| {
                input.worker.as_ref().is_none_or(|w| {
                    let observing = o.observing();
                    observing.connection.to_string() == *w
                        || observing.worker_name.as_deref() == Some(w.as_str())
                })
            })
            .filter(|o| Self::visible(engine, session, o))
            .map(|o| self.info(o))
            .collect();
        observers.sort_by(|a, b| (a.created_at_ms, &a.id).cmp(&(b.created_at_ms, &b.id)));
        ObserversListResult { observers }
    }

    /// `engine::observers::get`. An observer the caller may not see does not
    /// exist for it.
    pub fn get_info(
        &self,
        engine: &Engine,
        session: Option<&Arc<Session>>,
        id: &str,
    ) -> Result<ObserverInfo, ErrorBody> {
        self.get(id)
            .filter(|o| Self::visible(engine, session, o))
            .map(|o| self.info(&o))
            .ok_or_else(|| not_found(id))
    }

    /// `engine::observers::dispose`: by the observing worker (whatever it may
    /// see of the observed function now), or by an unrestricted caller.
    pub fn dispose_by_caller(
        &self,
        engine: &Engine,
        session: Option<&Arc<Session>>,
        input: &ObserverIdInput,
    ) -> Result<DisposeObserverResult, ErrorBody> {
        let Some(observer) = self.get(&input.id) else {
            return Err(not_found(&input.id));
        };
        let unrestricted = session.is_none_or(|s| s.config.rbac.is_none());
        let is_observing_worker = input
            .caller_worker_id
            .as_deref()
            .and_then(|id| uuid::Uuid::parse_str(id).ok())
            .is_some_and(|id| id == observer.observing().connection);
        if !is_observing_worker && !Self::visible(engine, session, &observer) {
            return Err(not_found(&input.id));
        }
        if !unrestricted && !is_observing_worker {
            return Err(ErrorBody::new(
                observe_codes::FORBIDDEN,
                format!(
                    "observer `{}` can only be disposed by the worker that created it",
                    input.id
                ),
            ));
        }
        self.dispose(
            &observer,
            observe_codes::UNOBSERVED,
            "disposed through engine::observers::dispose",
        );
        Ok(DisposeObserverResult { disposed: true })
    }
}

fn not_found(id: &str) -> ErrorBody {
    ErrorBody::new(
        observe_codes::OBSERVER_NOT_FOUND,
        format!("no observer `{id}`"),
    )
}

// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! The daemon's own connection to the engine.
//!
//! The daemon is an ordinary worker: it connects with the SDK, registers under
//! its own namespace (`--id`) and exposes `compose::*` there. Two daemons
//! therefore never compete for the same function ids — the namespace is what
//! addresses one.
//!
//! It is also the daemon's window into the engine: readiness is "the child
//! showed up in `engine::workers::list` under `(namespace, container)`", and
//! configuration is fetched here before any child is spawned.

use std::time::Duration;

use iii_sdk::{IIIClient, InitOptions, protocol::TriggerRequest, register_worker};
use serde_json::json;

use crate::{
    error::{ComposeError, Result},
    process::Supervised,
};

/// How often readiness is re-checked. Short enough that a fast worker is not
/// held back, long enough not to hammer the engine for a minute.
const READINESS_POLL_INTERVAL: Duration = Duration::from_millis(200);

/// Budget for a single introspection or configuration call.
const CALL_TIMEOUT_MS: u64 = 10_000;

pub struct EngineClient {
    client: IIIClient,
    /// Namespace this daemon registered in — the same one `compose::*` lands
    /// in. Children live in the *project* namespace, which is separate.
    namespace: String,
}

impl EngineClient {
    /// Connects as `daemon_id`, in the namespace of the same name.
    ///
    /// A second daemon started with the same `--id` collides on
    /// `(namespace, worker_name)`, and the engine rejects it fatally — see
    /// [`EngineClient::fatal_error`].
    pub fn connect(address: &str, daemon_id: &str) -> Self {
        let mut metadata = iii_sdk::iii::WorkerMetadata {
            name: daemon_id.to_string(),
            description: Some("compose daemon".to_string()),
            ..Default::default()
        };
        metadata.namespace = Some(daemon_id.to_string());

        let client = register_worker(
            address,
            InitOptions {
                metadata: Some(metadata),
                namespace: Some(daemon_id.to_string()),
                ..Default::default()
            },
        );

        Self {
            client,
            namespace: daemon_id.to_string(),
        }
    }

    pub fn client(&self) -> &IIIClient {
        &self.client
    }

    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    /// The registration rejection that stopped this daemon, if any. Populated
    /// when another daemon already holds this `--id`.
    pub fn fatal_error(&self) -> Option<iii_sdk::Error> {
        self.client.fatal_error()
    }

    /// Fetches a configuration entry from the configuration worker.
    ///
    /// Fetch-or-fail: a container that declares `config_name` does not start
    /// unless its configuration resolves. Starting it with defaults would be a
    /// silent downgrade.
    pub async fn fetch_config(&self, name: &str) -> Result<serde_yaml::Value> {
        let response = self
            .client
            .trigger(TriggerRequest {
                function_id: "configuration::get".to_string(),
                payload: json!({ "id": name }),
                action: None,
                timeout_ms: Some(CALL_TIMEOUT_MS),
            })
            .await
            .map_err(|source| ComposeError::ConfigFetchFailed {
                name: name.to_string(),
                message: source.to_string(),
            })?;

        let value = response.get("value").cloned().unwrap_or(response);
        serde_yaml::to_value(value).map_err(|err| ComposeError::ConfigFetchFailed {
            name: name.to_string(),
            message: err.to_string(),
        })
    }

    /// Waits until `container` is registered in `namespace`, or the budget runs
    /// out.
    ///
    /// A child that exits while we wait short-circuits: reporting a readiness
    /// timeout for a process that is already gone sends the operator looking in
    /// the wrong place.
    pub async fn wait_until_ready(
        &self,
        namespace: &str,
        container: &str,
        child: &Supervised,
        timeout: Duration,
    ) -> Result<()> {
        let deadline = tokio::time::Instant::now() + timeout;

        loop {
            if let crate::process::Outcome::Exited(status) = child.poll() {
                return Err(ComposeError::ChildExitedBeforeReady {
                    container: container.to_string(),
                    code: status.code().unwrap_or(-1),
                });
            }

            if self.is_registered(namespace, container).await? {
                return Ok(());
            }

            if tokio::time::Instant::now() >= deadline {
                return Err(ComposeError::ReadinessTimeout {
                    container: container.to_string(),
                    seconds: timeout.as_secs(),
                });
            }

            tokio::time::sleep(READINESS_POLL_INTERVAL).await;
        }
    }

    /// Whether a worker named `container` is connected in `namespace`.
    ///
    /// `engine::workers::list` has no namespace filter, so the pair is matched
    /// here. Matching on the name alone would accept another project's worker
    /// of the same name.
    pub async fn is_registered(&self, namespace: &str, container: &str) -> Result<bool> {
        let response = self
            .client
            .trigger(TriggerRequest {
                function_id: "engine::workers::list".to_string(),
                payload: json!({}),
                action: None,
                timeout_ms: Some(CALL_TIMEOUT_MS),
            })
            .await
            .map_err(|source| ComposeError::EngineCallFailed {
                function: "engine::workers::list".to_string(),
                message: source.to_string(),
            })?;

        let Some(workers) = response.get("workers").and_then(|w| w.as_array()) else {
            return Ok(false);
        };

        Ok(workers.iter().any(|worker| {
            let name = worker.get("name").and_then(|n| n.as_str());
            let worker_namespace = worker
                .get("namespace")
                .and_then(|n| n.as_str())
                .unwrap_or("default");
            name == Some(container) && worker_namespace == namespace
        }))
    }

    pub async fn shutdown(&self) {
        self.client.shutdown_async().await;
    }
}

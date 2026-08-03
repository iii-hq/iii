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

/// Where a worker lands when it does not know about namespaces.
const DEFAULT_NAMESPACE: &str = "default";

/// Budget for a single introspection or configuration call.
const CALL_TIMEOUT_MS: u64 = 10_000;

pub struct EngineClient {
    client: IIIClient,
    /// Namespace this daemon registered in — the same one `compose::*` lands
    /// in. Children live in the *project* namespace, which is separate.
    namespace: String,
}

impl EngineClient {
    /// Connects as `daemon_id` in `namespace`, the same one its containers
    /// register in.
    ///
    /// A second daemon on the same pair collides on `(namespace, worker_name)`
    /// and the engine rejects it fatally — see [`EngineClient::fatal_error`].
    /// Two copies of one project are a duplicate, not two projects.
    pub fn connect(address: &str, daemon_id: &str, namespace: &str) -> Self {
        let mut metadata = iii_sdk::iii::WorkerMetadata {
            name: daemon_id.to_string(),
            description: Some("compose daemon".to_string()),
            ..Default::default()
        };
        metadata.namespace = Some(namespace.to_string());

        let client = register_worker(
            address,
            InitOptions {
                metadata: Some(metadata),
                namespace: Some(namespace.to_string()),
                ..Default::default()
            },
        );

        Self {
            client,
            namespace: namespace.to_string(),
        }
    }

    pub fn client(&self) -> &IIIClient {
        &self.client
    }

    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    /// Whether the socket is up right now.
    ///
    /// The SDK reconnects on its own and replays the daemon's registrations,
    /// but says nothing about the children — so the daemon polls this to notice
    /// the connection coming back and re-check the project against the engine.
    pub fn is_connected(&self) -> bool {
        matches!(
            self.client.get_connection_state(),
            iii_sdk::runtime::IIIConnectionState::Connected
        )
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

        // Who was already in the project namespace before this child started.
        // Anything that appears while it is starting and is not this container
        // is a candidate for having named itself — the registry `state` worker
        // does exactly that, and under a container called `store` it would
        // otherwise be a bare timeout over a healthy process.
        let present_before = self.workers_in(namespace).await.unwrap_or_default();

        // Functions owned by this *name* that already existed. Two projects
        // may each declare a container called `api`, and the engine reports
        // both under that one name — so only an entry that appears while this
        // child is starting can be attributed to it.
        let functions_before = self.functions_of(container).await.unwrap_or_default();

        // A worker of this name already in `default` is not ours: an unrelated
        // one was there before we spawned. Only a newcomer can be blamed on this
        // child, so the comparison is against what was there at the start.
        let stray_before = namespace != DEFAULT_NAMESPACE
            && self
                .registered_namespaces(container)
                .await?
                .contains(&DEFAULT_NAMESPACE.to_string());

        loop {
            if let crate::process::Outcome::Exited(status) = child.poll() {
                return Err(ComposeError::ChildExitedBeforeReady {
                    container: container.to_string(),
                    code: status.code().unwrap_or(-1),
                });
            }

            let namespaces = self.registered_namespaces(container).await?;

            if namespaces.iter().any(|found| found == namespace) {
                // The worker is where it should be. Its functions are a
                // separate fact: they travel as their own messages, and the
                // engine can only file them once it knows the connection's
                // namespace — which arrives inside `engine::workers::register`,
                // a call whose latency is unbounded. Under load that call is
                // processed after the engine gives up waiting, and the queued
                // registrations are filed in `default` while the worker itself
                // still lands here. Checking only the worker would call that
                // ready, and the first invocation would find nothing.
                if let Some((function, found_in)) = self
                    .misplaced_function(container, namespace, &functions_before)
                    .await?
                {
                    return Err(ComposeError::FunctionsInWrongNamespace {
                        container: container.to_string(),
                        function,
                        found_in,
                        expected: namespace.to_string(),
                    });
                }
                return Ok(());
            }

            // A worker built against an SDK older than namespace routing never
            // reads III_NAMESPACE: it connects, registers in `default`, and is
            // perfectly healthy — just not where this project can see it. Once it
            // shows up there, waiting out the timeout only delays a verdict we
            // already have; it will never register where we are watching.
            if !stray_before && namespaces.iter().any(|found| found == DEFAULT_NAMESPACE) {
                return Err(ComposeError::WorkerIgnoredNamespace {
                    container: container.to_string(),
                    found_in: DEFAULT_NAMESPACE.to_string(),
                    expected: namespace.to_string(),
                });
            }

            if tokio::time::Instant::now() >= deadline {
                // A worker that arrived under a different name is alive and in
                // the right namespace — only unreachable through the name the
                // project uses. Saying which name it took is the difference
                // between a fix and a hunt.
                if let Some(name) = self
                    .workers_in(namespace)
                    .await
                    .unwrap_or_default()
                    .into_iter()
                    .find(|name| name != container && !present_before.contains(name))
                {
                    return Err(ComposeError::WorkerNameMismatch {
                        container: container.to_string(),
                        registered_as: name,
                        namespace: namespace.to_string(),
                    });
                }

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
    /// The pair is matched, never the name alone: that would accept another
    /// project's worker of the same name.
    pub async fn is_registered(&self, namespace: &str, container: &str) -> Result<bool> {
        Ok(self
            .registered_namespaces(container)
            .await?
            .iter()
            .any(|found| found == namespace))
    }

    /// Every worker name currently connected in `namespace`.
    pub async fn workers_in(&self, namespace: &str) -> Result<Vec<String>> {
        Ok(self
            .list_workers()
            .await?
            .into_iter()
            .filter(|(_, worker_namespace)| worker_namespace == namespace)
            .map(|(name, _)| name)
            .collect())
    }

    /// Every namespace where a worker named `container` is connected.
    ///
    /// `engine::workers::list` has no namespace filter, so the grouping happens
    /// here. One call answers both questions readiness asks — is it here yet,
    /// and did it land somewhere else — so polling costs no more than before.
    async fn registered_namespaces(&self, container: &str) -> Result<Vec<String>> {
        Ok(self
            .list_workers()
            .await?
            .into_iter()
            .filter(|(name, _)| name == container)
            .map(|(_, namespace)| namespace)
            .collect())
    }

    /// A function owned by `container` that is registered somewhere other than
    /// `namespace`, if there is one.
    ///
    /// A worker with no functions at all answers `None`: exporting nothing is
    /// legitimate for a worker that only registers triggers.
    async fn misplaced_function(
        &self,
        container: &str,
        namespace: &str,
        before: &[(String, String)],
    ) -> Result<Option<(String, String)>> {
        let functions = self.functions_of(container).await?;
        Ok(first_misplaced(&functions, namespace, before))
    }

    /// Every `(function id, namespace)` owned by `worker_name`.
    async fn functions_of(&self, worker_name: &str) -> Result<Vec<(String, String)>> {
        let response = self
            .client
            .trigger(TriggerRequest {
                function_id: "engine::functions::list".to_string(),
                payload: json!({ "include_internal": true }),
                action: None,
                timeout_ms: Some(CALL_TIMEOUT_MS),
            })
            .await
            .map_err(|source| ComposeError::EngineCallFailed {
                function: "engine::functions::list".to_string(),
                message: source.to_string(),
            })?;

        let Some(functions) = response.get("functions").and_then(|f| f.as_array()) else {
            return Ok(Vec::new());
        };

        Ok(functions
            .iter()
            .filter(|function| {
                function.get("worker_name").and_then(|n| n.as_str()) == Some(worker_name)
            })
            .filter_map(|function| {
                let id = function.get("function_id").and_then(|i| i.as_str())?;
                let namespace = function
                    .get("namespace")
                    .and_then(|n| n.as_str())
                    .unwrap_or(DEFAULT_NAMESPACE);
                Some((id.to_string(), namespace.to_string()))
            })
            .collect())
    }

    /// Every `(worker name, namespace)` the engine currently holds.
    async fn list_workers(&self) -> Result<Vec<(String, String)>> {
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
            return Ok(Vec::new());
        };

        Ok(workers
            .iter()
            .filter_map(|worker| {
                let name = worker.get("name").and_then(|n| n.as_str())?;
                let namespace = worker
                    .get("namespace")
                    .and_then(|n| n.as_str())
                    .unwrap_or(DEFAULT_NAMESPACE);
                Some((name.to_string(), namespace.to_string()))
            })
            .collect())
    }

    pub async fn shutdown(&self) {
        self.client.shutdown_async().await;
    }
}

/// First function that appeared outside `expected` while this container was
/// starting.
///
/// `before` is what the engine already held under this worker name. Without it
/// the rule would accuse another project: two compose projects may each run a
/// container called `api`, and the engine lists both under that name. Only a
/// newcomer can belong to the child we just spawned.
///
/// Kept separate from the engine call so the decision is testable without one:
/// it is the rule that matters, not the transport.
fn first_misplaced(
    functions: &[(String, String)],
    expected: &str,
    before: &[(String, String)],
) -> Option<(String, String)> {
    functions
        .iter()
        .find(|entry| entry.1 != expected && !before.contains(entry))
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::first_misplaced;

    fn functions(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(id, ns)| (id.to_string(), ns.to_string()))
            .collect()
    }

    #[test]
    fn functions_in_the_expected_namespace_are_fine() {
        let listed = functions(&[("api::ping", "orders"), ("api::echo", "orders")]);
        assert_eq!(first_misplaced(&listed, "orders", &[]), None);
    }

    #[test]
    fn a_worker_with_no_functions_is_fine() {
        // Registering only triggers is legitimate; silence is not a symptom.
        assert_eq!(first_misplaced(&[], "orders", &[]), None);
    }

    #[test]
    fn one_function_left_behind_is_enough_to_report() {
        // The split the engine's registration grace produces: the worker landed
        // in the project namespace, some of its functions did not.
        let listed = functions(&[("api::ping", "orders"), ("api::echo", "default")]);
        assert_eq!(
            first_misplaced(&listed, "orders", &[]),
            Some(("api::echo".to_string(), "default".to_string()))
        );
    }

    #[test]
    fn the_report_names_the_namespace_they_landed_in() {
        let listed = functions(&[("api::ping", "somewhere-else")]);
        let (function, found_in) = first_misplaced(&listed, "orders", &[]).expect("misplaced");
        assert_eq!(function, "api::ping");
        assert_eq!(found_in, "somewhere-else", "not assumed to be `default`");
    }

    #[test]
    fn another_project_running_the_same_container_name_is_not_accused() {
        // Two projects, each with an `api`. The engine lists both under that
        // one name, so B's readiness sees A's functions in A's namespace —
        // which is exactly where they belong.
        let already_there = functions(&[("api::ping", "shop-aaaaaaaa")]);
        let listed = functions(&[("api::ping", "shop-aaaaaaaa"), ("api::ping", "shop-bbbbbbbb")]);

        assert_eq!(
            first_misplaced(&listed, "shop-bbbbbbbb", &already_there),
            None
        );
    }

    #[test]
    fn a_newcomer_outside_the_namespace_is_still_caught() {
        // Same two projects, but this time B's own function landed in
        // `default`. It was not there before B started, so it is B's.
        let already_there = functions(&[("api::ping", "shop-aaaaaaaa")]);
        let listed = functions(&[("api::ping", "shop-aaaaaaaa"), ("api::ping", "default")]);

        assert_eq!(
            first_misplaced(&listed, "shop-bbbbbbbb", &already_there),
            Some(("api::ping".to_string(), "default".to_string()))
        );
    }
}

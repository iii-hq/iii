// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! The daemon's own connection to the engine.
//!
//! The daemon is an ordinary worker: it connects with the SDK, registers in
//! `default` as `compose`, and exposes `compose::*` there — where a trigger
//! with no namespace flag lands. One connection serves every project it holds,
//! whatever namespaces those declare.
//!
//! It is also the daemon's window into the engine: readiness is "the child
//! showed up in `engine::workers::list` under `(namespace, container)`", and
//! configuration is fetched here before any child is spawned.

use std::time::Duration;

use iii_sdk::{IIIClient, InitOptions, protocol::TriggerRequest, register_worker};

use serde_json::{Value, json};

use crate::{
    error::{ComposeError, Result},
    process::Supervised,
};

/// How often readiness is re-checked. Short enough that a fast worker is not
/// held back, long enough not to hammer the engine for a minute.
const READINESS_POLL_INTERVAL: Duration = Duration::from_millis(200);

/// Where a worker lands when it does not know about namespaces, and where the
/// engine serves its own functions.
///
/// The daemon lives in its own namespace, and the SDK sends a worker's calls
/// there unless told otherwise. `configuration::*` and `engine::*` are compiled
/// into the engine and exist only here, so every call the daemon makes to one
/// names this namespace. Unqualified, they would look for the engine's
/// functions in the daemon's own and find nothing.
const DEFAULT_NAMESPACE: &str = "default";

/// Budget for a single introspection or configuration call.
const CALL_TIMEOUT_MS: u64 = 10_000;

pub struct EngineClient {
    client: IIIClient,
    /// Namespace this daemon registered in — the same one `compose::*` lands
    /// in. Children live in the *project* namespace, which is separate.
    namespace: String,
}

/// The last few lines a container printed, for an error that would otherwise
/// carry only an exit code.
///
/// Bounded on both axes — a handful of lines, and each one cut — because this
/// travels inside an error message that a caller will see on one terminal
/// line. The whole log stays on disk; `compose::status` says where.
fn log_tail(log_dir: &std::path::Path, container: &str) -> Option<String> {
    const LINES: usize = 5;
    const WIDTH: usize = 200;

    let text = std::fs::read_to_string(log_dir.join(format!("{container}.log"))).ok()?;
    let tail: Vec<&str> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .rev()
        .take(LINES)
        .collect();
    if tail.is_empty() {
        return None;
    }
    Some(
        tail.into_iter()
            .rev()
            .map(|line| {
                let trimmed: String = line.chars().take(WIDTH).collect();
                format!("  {trimmed}")
            })
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

/// Whether a trigger failure is the configuration worker reporting an
/// unregistered id, as opposed to anything that went wrong reaching it.
///
/// The distinction is the whole of fetch-or-fail: one means "nothing stored
/// yet", the other means "we do not know what is stored", and only the second
/// is a reason to refuse to start.
fn is_not_found(error: &iii_sdk::Error) -> bool {
    matches!(error, iii_sdk::Error::Remote { code, .. } if code == "NOT_FOUND")
}

/// What the engine showed for one container before compose started it.
///
/// Readiness reports differences, so it needs a "before" that predates the
/// child. See [`EngineClient::readiness_baseline`].
#[derive(Debug, Clone, Default)]
pub struct ReadinessBaseline {
    /// Workers already in the project namespace.
    present: Vec<String>,
    /// `(function id, namespace)` already owned by this container name.
    functions: Vec<(String, String)>,
    /// A worker of this name was already in `default`, so one showing up there
    /// is not evidence that ours ignored `III_NAMESPACE`.
    stray: bool,
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
    /// Fetch-or-fail, with one deliberate exception. A configuration worker
    /// that errors or cannot be reached fails the container: starting it on
    /// defaults it did not ask for is a silent downgrade, and an http worker on
    /// the wrong port is worse than no http worker.
    ///
    /// An entry that simply *does not exist yet* is not that. The worker is
    /// what registers its own id, with its own schema, on the boot compose
    /// would be refusing — so treating a first boot as a fetch failure
    /// deadlocks every project that names a configuration before it has ever
    /// run. Absent resolves to `None`: the layer contributes nothing, and what
    /// the compose file declares still reaches the child.
    pub async fn fetch_config(&self, name: &str) -> Result<Option<serde_yaml::Value>> {
        let response = match self
            .client
            .trigger(
                TriggerRequest {
                    function_id: "configuration::get".to_string(),
                    // Raw, so `${VAR}` placeholders survive the round trip. The
                    // worker pushes this value back into the store at boot, and an
                    // expanded fetch would persist the secret a lazy reference was
                    // there to avoid — turning `password: ${DB_PASSWORD}` into the
                    // password, permanently. Expansion belongs to the read that
                    // uses the value, not to a copy passing through.
                    payload: json!({ "id": name, "raw": true }),
                    action: None,
                    timeout_ms: Some(CALL_TIMEOUT_MS),
                }
                .namespace(DEFAULT_NAMESPACE),
            )
            .await
        {
            Ok(response) => response,
            Err(source) if is_not_found(&source) => return Ok(None),
            Err(source) => {
                return Err(ComposeError::ConfigFetchFailed {
                    name: name.to_string(),
                    message: source.to_string(),
                });
            }
        };

        let value = response.get("value").cloned().unwrap_or(response);
        // A stored `null` is an entry with nothing in it, which is what an
        // absent one is. Passing it on as a value makes a container start
        // against a configuration of `null` instead of its own defaults.
        if value.is_null() {
            return Ok(None);
        }
        serde_yaml::to_value(value)
            .map(Some)
            .map_err(|err| ComposeError::ConfigFetchFailed {
                name: name.to_string(),
                message: err.to_string(),
            })
    }

    /// Writes the resolved configuration into the configuration worker, under
    /// the entry the container named.
    ///
    /// This is what makes a worker configurable by compose without the worker
    /// knowing compose exists. A worker reads its configuration from the
    /// configuration worker, and re-registering its own schema without an
    /// `initial_value` reuses whatever is stored — so a value written here,
    /// before the child starts, is the value it boots on. Nothing in the fleet
    /// has to change.
    ///
    /// The existing schema, name and description are carried over rather than
    /// replaced. A worker that has run before keeps its schema, which means the
    /// value written here is validated against it; and the console keeps the
    /// name the worker gave the entry instead of a placeholder from compose.
    /// On a first boot there is nothing to carry, so the write is permissive
    /// and the worker's own registration fills the metadata in moments later.
    pub async fn publish_config(&self, name: &str, value: &serde_yaml::Value) -> Result<()> {
        let existing = self.config_metadata(name).await;
        let value = serde_json::to_value(value).map_err(|err| ComposeError::ConfigFetchFailed {
            name: name.to_string(),
            message: err.to_string(),
        })?;

        let existing = existing.unwrap_or_default();
        // An entry can exist with no schema at all — one seeded from a config
        // file, or registered before its worker declared one. `null` is not a
        // JSON Schema, and carrying it through would be rejected, so absent and
        // null both mean the same permissive thing here. The worker replaces it
        // with its own on its next boot.
        let schema = match existing.get("schema") {
            Some(schema) if !schema.is_null() => schema.clone(),
            _ => json!({}),
        };

        let payload = json!({
            "id": name,
            "name": existing.get("name").cloned().unwrap_or_else(|| json!(name)),
            "description": existing
                .get("description")
                .cloned()
                .unwrap_or_else(|| json!("resolved by compose")),
            "schema": schema,
            "initial_value": value,
        });

        self.client
            .trigger(
                TriggerRequest {
                    function_id: "configuration::register".to_string(),
                    payload,
                    action: None,
                    timeout_ms: Some(CALL_TIMEOUT_MS),
                }
                .namespace(DEFAULT_NAMESPACE),
            )
            .await
            .map(|_| ())
            .map_err(|source| ComposeError::ConfigPublishFailed {
                name: name.to_string(),
                message: source.to_string(),
            })
    }

    /// The schema and metadata already registered for `name`, if any.
    async fn config_metadata(&self, name: &str) -> Option<serde_json::Map<String, Value>> {
        self.client
            .trigger(
                TriggerRequest {
                    function_id: "configuration::schema".to_string(),
                    payload: json!({ "id": name }),
                    action: None,
                    timeout_ms: Some(CALL_TIMEOUT_MS),
                }
                .namespace(DEFAULT_NAMESPACE),
            )
            .await
            .ok()
            .and_then(|response| response.as_object().cloned())
    }

    /// What the engine already showed for this container, captured *before*
    /// compose starts anything for it.
    ///
    /// Every question readiness asks is a difference: did this worker, this
    /// function, this stray registration appear *because of the child we
    /// started? A snapshot taken once the child is already running answers
    /// that with the child's own registration, so the caller takes this before
    /// the spawn — before the package install, even, since a download is
    /// seconds during which anything may arrive.
    pub async fn readiness_baseline(
        &self,
        namespace: &str,
        container: &str,
    ) -> Result<ReadinessBaseline> {
        Ok(ReadinessBaseline {
            // Anything that appears while the child is starting and is not this
            // container is a candidate for having named itself — the registry
            // `state` worker does exactly that, and under a container called
            // `store` it would otherwise be a bare timeout over a healthy
            // process.
            present: self.workers_in(namespace).await.unwrap_or_default(),
            // Two projects may each declare a container called `api`, and the
            // engine reports both under that one name — so only an entry that
            // appears while this child is starting can be attributed to it.
            functions: self.functions_of(container).await.unwrap_or_default(),
            // A worker of this name already in `default` is not ours: an
            // unrelated one was there before we spawned, and blaming the child
            // for it would turn a healthy neighbour into our failure.
            stray: namespace != DEFAULT_NAMESPACE
                && self
                    .registered_namespaces(container)
                    .await?
                    .contains(&DEFAULT_NAMESPACE.to_string()),
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
        baseline: &ReadinessBaseline,
        log_dir: &std::path::Path,
    ) -> Result<()> {
        match tokio::time::timeout(
            timeout,
            self.wait_until_ready_inner(namespace, container, child, timeout, baseline, log_dir),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => Err(ComposeError::ReadinessTimeout {
                container: container.to_string(),
                seconds: timeout.as_secs(),
            }),
        }
    }

    async fn wait_until_ready_inner(
        &self,
        namespace: &str,
        container: &str,
        child: &Supervised,
        timeout: Duration,
        baseline: &ReadinessBaseline,
        log_dir: &std::path::Path,
    ) -> Result<()> {
        let deadline = tokio::time::Instant::now() + timeout;
        let ReadinessBaseline {
            present: present_before,
            functions: functions_before,
            stray: stray_before,
        } = baseline;
        let stray_before = *stray_before;

        loop {
            if let crate::process::Outcome::Exited(status) = child.poll() {
                return Err(ComposeError::ChildExitedBeforeReady {
                    container: container.to_string(),
                    code: status.code().unwrap_or(-1),
                    tail: log_tail(log_dir, container),
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
                    .misplaced_function(container, namespace, functions_before, &namespaces)
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
        occupied: &[String],
    ) -> Result<Option<(String, String)>> {
        let functions = self.functions_of(container).await?;
        Ok(first_misplaced(&functions, namespace, before, occupied))
    }

    /// Every `(function id, namespace)` owned by `worker_name`.
    async fn functions_of(&self, worker_name: &str) -> Result<Vec<(String, String)>> {
        let response = self
            .client
            .trigger(
                TriggerRequest {
                    function_id: "engine::functions::list".to_string(),
                    payload: json!({ "include_internal": true }),
                    action: None,
                    timeout_ms: Some(CALL_TIMEOUT_MS),
                }
                .namespace(DEFAULT_NAMESPACE),
            )
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
            .trigger(
                TriggerRequest {
                    function_id: "engine::workers::list".to_string(),
                    payload: json!({}),
                    action: None,
                    timeout_ms: Some(CALL_TIMEOUT_MS),
                }
                .namespace(DEFAULT_NAMESPACE),
            )
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
    occupied: &[String],
) -> Option<(String, String)> {
    functions
        .iter()
        .find(|entry| {
            let namespace = &entry.1;
            namespace != expected
                // A namespace holding a live worker of this name owns what is
                // filed under it. Two projects each running `state` list their
                // functions under one name, and the neighbour's arrive whenever
                // it happens to register them — including in the window between
                // our snapshot and our spawn. Without this, its timing becomes
                // our failure.
                && !occupied.iter().any(|held| held == namespace)
                && !before.contains(entry)
        })
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
    fn a_namespace_with_a_live_worker_of_that_name_owns_its_own_functions() {
        // Two projects each running `state`. The neighbour registers a function
        // late — after our snapshot and before our child is up — and it lands
        // where it belongs, in the neighbour's namespace. Attributing it to us
        // turns its timing into our failure, which is what happened the first
        // time a project took long enough to make the window visible.
        let listed = functions(&[
            ("state::get", "shop-a"),
            ("state::barrier", "shop-a"),
            ("state::get", "shop-b"),
        ]);
        let occupied = vec!["shop-a".to_string(), "shop-b".to_string()];

        assert_eq!(
            first_misplaced(&listed, "shop-b", &[], &occupied),
            None,
            "a namespace holding a worker of this name owns what is filed there"
        );

        // The failure it exists for is still caught: registrations that landed
        // where no worker of that name lives are nobody else's.
        let orphaned = functions(&[("state::get", "default")]);
        assert!(first_misplaced(&orphaned, "shop-b", &[], &occupied).is_some());
    }

    #[test]
    fn functions_in_the_expected_namespace_are_fine() {
        let listed = functions(&[("api::ping", "orders"), ("api::echo", "orders")]);
        assert_eq!(first_misplaced(&listed, "orders", &[], &[]), None);
    }

    #[test]
    fn a_worker_with_no_functions_is_fine() {
        // Registering only triggers is legitimate; silence is not a symptom.
        assert_eq!(first_misplaced(&[], "orders", &[], &[]), None);
    }

    #[test]
    fn one_function_left_behind_is_enough_to_report() {
        // The split the engine's registration grace produces: the worker landed
        // in the project namespace, some of its functions did not.
        let listed = functions(&[("api::ping", "orders"), ("api::echo", "default")]);
        assert_eq!(
            first_misplaced(&listed, "orders", &[], &[]),
            Some(("api::echo".to_string(), "default".to_string()))
        );
    }

    #[test]
    fn the_report_names_the_namespace_they_landed_in() {
        let listed = functions(&[("api::ping", "somewhere-else")]);
        let (function, found_in) = first_misplaced(&listed, "orders", &[], &[]).expect("misplaced");
        assert_eq!(function, "api::ping");
        assert_eq!(found_in, "somewhere-else", "not assumed to be `default`");
    }

    #[test]
    fn another_project_running_the_same_container_name_is_not_accused() {
        // Two projects, each with an `api`. The engine lists both under that
        // one name, so B's readiness sees A's functions in A's namespace —
        // which is exactly where they belong.
        let already_there = functions(&[("api::ping", "shop-aaaaaaaa")]);
        let listed = functions(&[
            ("api::ping", "shop-aaaaaaaa"),
            ("api::ping", "shop-bbbbbbbb"),
        ]);

        assert_eq!(
            first_misplaced(&listed, "shop-bbbbbbbb", &already_there, &[]),
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
            first_misplaced(&listed, "shop-bbbbbbbb", &already_there, &[]),
            Some(("api::ping".to_string(), "default".to_string()))
        );
    }
}

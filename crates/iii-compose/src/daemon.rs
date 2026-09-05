// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! The daemon: one engine connection, many projects.
//!
//! Two addresses, and they answer different questions. `--namespace` is the daemon —
//! the namespace it serves `compose::*` in, and how an operator reaches this
//! machine rather than a neighbour. `file=` is the project: a daemon holds as
//! many as it is given, and the compose file is the only thing that names one.
//!
//! The worker name stays `compose` on every machine, so the lease the engine
//! arbitrates is `(namespace, compose)`. Two daemons with different namespaces
//! coexist; two claiming one namespace cannot, and the loser is told
//! immediately rather than left holding projects nobody can address.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use futures::StreamExt;

use tokio::sync::{Mutex, OnceCell};

use crate::{
    config::{ComposeFile, EngineSpec},
    engine::EngineClient,
    error::{ComposeError, Result},
    lifecycle::{OpResult, OpStatus},
    logs::{LogCursor, LogStream, LogsOutcome},
    project::Project,
};

/// How often the supervisor checks whether a ready child is still alive.
/// Fast enough that a crash is reported while the operator is still watching,
/// slow enough that an idle daemon costs nothing.
const SUPERVISION_INTERVAL: Duration = Duration::from_millis(250);
/// How many times `add_configured` redoes its unlocked plan because the file
/// changed before it took the mutation lock. Beyond this it fails with
/// `AddPlanStale` rather than edit from a plan made against another file.
const ADD_REPLAN_LIMIT: u32 = 2;

struct AddPlan {
    snapshot: String,
    wanted: Vec<crate::edit::NewContainer>,
    aliases: Vec<crate::dependencies::Alias>,
    selected_versions: BTreeMap<String, String>,
    resolved_graphs: BTreeMap<String, BTreeSet<String>>,
}

#[derive(Debug, Clone)]
pub enum EnginePolicy {
    Managed {
        owner: PathBuf,
        spec: EngineSpec,
    },
    External,
    /// An external engine selected while the invocation file still contains
    /// `engine:`. `expected` is tracked when that section supplied the URL;
    /// `None` means an explicit CLI URL overrides the section entirely.
    ExternalFile {
        owner: PathBuf,
        expected: Option<EngineSpec>,
    },
}

impl EnginePolicy {
    pub fn managed(file: &ComposeFile) -> Option<Self> {
        file.engine.as_ref().map(|spec| Self::Managed {
            owner: file.path.clone(),
            spec: spec.clone(),
        })
    }

    pub fn external_from_file(file: &ComposeFile) -> Self {
        Self::ExternalFile {
            owner: file.path.clone(),
            expected: file.engine.clone(),
        }
    }

    pub fn external_overriding(file: &ComposeFile) -> Self {
        Self::ExternalFile {
            owner: file.path.clone(),
            expected: None,
        }
    }

    fn validate_project(&self, file: &ComposeFile) -> Result<()> {
        self.validate_engine_section(&file.path, file.engine.as_ref())
    }

    fn validate_engine_section(&self, path: &Path, engine: Option<&EngineSpec>) -> Result<()> {
        let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        match self {
            Self::External if engine.is_some() => {
                Err(ComposeError::EngineSectionRequiresManagedStart { path })
            }
            Self::External => Ok(()),
            Self::ExternalFile { owner, expected } if &path == owner => {
                if expected.as_ref().is_some_and(|spec| engine != Some(spec)) {
                    Err(ComposeError::EngineRestartRequired { path })
                } else {
                    Ok(())
                }
            }
            Self::ExternalFile { owner, .. } if engine.is_some() => {
                Err(ComposeError::EngineAlreadyOwned {
                    owner: owner.clone(),
                    path,
                })
            }
            Self::ExternalFile { .. } => Ok(()),
            Self::Managed { owner, spec } if &path == owner => {
                if engine == Some(spec) {
                    Ok(())
                } else {
                    Err(ComposeError::EngineRestartRequired { path })
                }
            }
            Self::Managed { owner, .. } if engine.is_some() => {
                Err(ComposeError::EngineAlreadyOwned {
                    owner: owner.clone(),
                    path,
                })
            }
            Self::Managed { .. } => Ok(()),
        }
    }
}

/// Keeps one declaration for each container in a mutation batch.
///
/// Registry graphs and direct request lists can contain the same container
/// more than once. Identical declarations collapse into one edit. Conflicting
/// declarations reject the batch before the file is edited, so
/// argument order cannot select the winning declaration.
fn coalesce_containers(
    containers: Vec<crate::edit::NewContainer>,
) -> Result<Vec<crate::edit::NewContainer>> {
    let mut positions = BTreeMap::new();
    let mut unique: Vec<crate::edit::NewContainer> = Vec::with_capacity(containers.len());

    for container in containers {
        if let Some(&position) = positions.get(&container.key) {
            if unique[position] != container {
                return Err(ComposeError::InvalidWorkerSpec {
                    spec: container.key.clone(),
                    reason: format!(
                        "the requested workers resolve container '{}' to conflicting sources, \
                         versions, dependencies, or settings",
                        container.key
                    ),
                });
            }
            continue;
        }

        positions.insert(container.key.clone(), unique.len());
        unique.push(container);
    }

    Ok(unique)
}

/// Compares fields that change which workers run or how they depend on each other.
fn runtime_topology_changed(previous: &ComposeFile, next: &ComposeFile) -> bool {
    if previous.containers.len() != next.containers.len() {
        return true;
    }
    previous.containers.iter().any(|(key, container)| {
        next.containers.get(key).is_none_or(|next| {
            container.worker != next.worker || container.start_after != next.start_after
        })
    })
}

fn update_selector(explicit: Option<&str>, current: &str) -> String {
    explicit.unwrap_or(current).to_string()
}

fn graph_members(containers: &[crate::edit::NewContainer]) -> BTreeSet<String> {
    containers
        .iter()
        .flat_map(|container| {
            std::iter::once(container.key.clone()).chain(container.start_after.iter().cloned())
        })
        .collect()
}

/// Returns one root's reachable declarations after alias and instance reuse.
fn graph_members_for_root(
    containers: &[crate::edit::NewContainer],
    root: &str,
) -> BTreeSet<String> {
    let containers = containers
        .iter()
        .map(|container| (container.key.as_str(), container))
        .collect::<BTreeMap<_, _>>();
    let mut members = BTreeSet::new();
    let mut visit = vec![root.to_string()];
    while let Some(key) = visit.pop() {
        if !members.insert(key.clone()) {
            continue;
        }
        if let Some(container) = containers.get(key.as_str()) {
            visit.extend(container.start_after.iter().cloned());
        }
    }
    members
}

fn stale_graph_members(
    previous: &BTreeMap<String, BTreeSet<String>>,
    replacements: &BTreeMap<String, BTreeSet<String>>,
) -> BTreeSet<String> {
    let mut next = previous.clone();
    for (root, nodes) in replacements {
        next.insert(root.clone(), nodes.clone());
    }
    let retained = next.values().flatten().collect::<BTreeSet<_>>();
    let roots = next.keys().collect::<BTreeSet<_>>();
    replacements
        .keys()
        .filter_map(|root| previous.get(root))
        .flatten()
        .filter(|node| !retained.contains(node) && !roots.contains(node))
        .cloned()
        .collect()
}

/// Select declared packages when no worker specs were supplied.
fn workers_to_update(
    compose: &crate::ComposeFile,
    workers: &[String],
) -> Result<Vec<crate::edit::NewContainer>> {
    if !workers.is_empty() {
        return workers
            .iter()
            .map(|worker| crate::edit::parse_worker(worker))
            .collect();
    }

    Ok(compose
        .containers
        .iter()
        .filter_map(|(key, container)| match &container.worker {
            crate::config::WorkerSource::Package { reference } => Some(crate::edit::NewContainer {
                key: key.clone(),
                source: crate::edit::Source::Package {
                    reference: reference.clone(),
                    version: Some("latest".to_string()),
                },
                start_after: container.start_after.clone(),
                fields: serde_yaml::Mapping::new(),
            }),
            crate::config::WorkerSource::Path { .. } => None,
        })
        .collect())
}

pub struct Daemon {
    /// What this daemon registered as. Fixed, and the same on every machine:
    /// what tells two daemons apart is the namespace, not the name.
    pub worker_name: String,
    /// This machine's identity — the `--id`, and the namespace it answers
    /// `compose::*` in. `id=` on a call is checked against it.
    pub daemon_namespace: String,
    /// An explicit CLI namespace for every project loaded by this daemon.
    /// When absent, each project keeps the namespace from its own file.
    project_namespace_override: Option<String>,
    pub engine_url: String,
    engine: Arc<EngineClient>,
    engine_policy: EnginePolicy,
    /// Every project this daemon has been asked about, keyed by the canonical
    /// path of its compose file. Nothing else identifies a project: a name
    /// someone chose would be a second identity, and a second identity can be
    /// pointed at the wrong file.
    /// The value is a per-key cell rather than the project itself, so two
    /// callers naming the same file wait on one load instead of each running
    /// their own. Two `Project::open` calls on one file both adopt the same
    /// surviving children, and the loser of the insert is still handed out.
    projects: Mutex<BTreeMap<PathBuf, Arc<OnceCell<Arc<Project>>>>>,
    /// Serialises read-edit-write-restart operations for each compose file.
    /// Different projects can still change in parallel, while two edits to one
    /// file cannot overwrite each other after reading the same source text.
    mutations: Mutex<BTreeMap<PathBuf, Arc<Mutex<()>>>>,
    /// Long-running mutations publish progress independently of their caller.
    pub operations: crate::operation::OperationManager,
    /// Set by `compose::stop`. The serve loop reads it and leaves through the
    /// same path a SIGTERM takes, so a remote stop and a local one cannot
    /// diverge in what they tear down.
    stop_requested: std::sync::atomic::AtomicBool,
}

impl Daemon {
    /// Connects and returns a daemon ready to serve `compose::*`.
    ///
    /// No project is loaded here: a daemon starts knowing nothing and learns
    /// about a project the first time a call names one.
    pub fn start(
        requested_engine_url: String,
        daemon_namespace: String,
        project_namespace_override: Option<String>,
        engine_policy: EnginePolicy,
    ) -> Arc<Self> {
        // A managed file is the sole engine source. Public callers receive the
        // same guarantee as the CLI: workers and policy checks cannot point at
        // different engines even if a stale URL was passed separately.
        let engine_url = match &engine_policy {
            EnginePolicy::Managed { spec, .. } => spec.url.clone(),
            EnginePolicy::External | EnginePolicy::ExternalFile { .. } => requested_engine_url,
        };
        // The name stays fixed and the *namespace* carries the identity, so
        // the lease is `(daemon_namespace, compose)`: two machines coexist, and two
        // daemons claiming to be the same machine cannot.
        let engine = Arc::new(EngineClient::connect(
            &engine_url,
            DAEMON_WORKER_NAME,
            &daemon_namespace,
        ));

        let daemon = Arc::new(Self {
            worker_name: DAEMON_WORKER_NAME.to_string(),
            daemon_namespace,
            project_namespace_override,
            engine_url,
            engine: Arc::clone(&engine),
            engine_policy,
            projects: Mutex::new(BTreeMap::new()),
            mutations: Mutex::new(BTreeMap::new()),
            operations: crate::operation::OperationManager::new(engine.client()),
            stop_requested: std::sync::atomic::AtomicBool::new(false),
        });

        Self::supervise(&daemon);
        daemon
    }

    pub fn engine(&self) -> &EngineClient {
        &self.engine
    }

    /// The registration rejection that stopped this daemon, if any.
    pub fn fatal_error(&self) -> Option<iii_sdk::Error> {
        self.engine.fatal_error()
    }

    fn project_namespace(&self, file: &ComposeFile) -> String {
        crate::namespace::project_namespace(
            self.project_namespace_override.as_deref(),
            file.namespace.as_deref(),
        )
    }

    /// Resolves, acquires, and persists package locks before loading a project.
    ///
    /// The caller holds this compose file's mutation lock through this call, so
    /// another mutation cannot replace the lock between persistence and the
    /// resolved-package attachment.
    async fn prepare_start_project(&self, file: &Path, frozen: bool) -> Result<Arc<Project>> {
        let mut compose = ComposeFile::load(file)?;
        self.engine_policy.validate_project(&compose)?;
        let namespace = self.project_namespace(&compose);
        crate::manifest::validate_offline(&compose, &namespace)?;
        let package_cache = crate::state::StateStore::package_cache()?;
        if frozen {
            crate::lockfile::prepare_frozen(&mut compose, &package_cache).await?;
        } else {
            let prepared =
                crate::lockfile::prepare(&mut compose, &package_cache, &BTreeSet::new()).await?;
            prepared.write_if_changed()?;
        }
        let project = self.project(file).await?;
        project.attach_resolved_packages(&compose).await;
        Ok(project)
    }

    /// The project `file` declares, loading it if this is the first time.
    ///
    /// Loading is idempotent: the same file reached twice is the same project,
    /// whether it was spelled relatively or absolutely, so there is no way to
    /// rebind one and no rebind to refuse.
    pub async fn project(&self, file: &Path) -> Result<Arc<Project>> {
        if file.is_relative() && !file.exists() {
            return Err(ComposeError::RelativeFileMissing {
                path: file.to_path_buf(),
                cwd: std::env::current_dir().unwrap_or_default(),
            });
        }
        let key = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());

        // The map lock is held only long enough to claim the cell. Loading under
        // it would make one slow project block `compose::list` and every other
        // project on the daemon.
        let cell = {
            let mut projects = self.projects.lock().await;
            Arc::clone(projects.entry(key.clone()).or_default())
        };

        cell.get_or_try_init(|| async {
            let mut compose = ComposeFile::load(file)?;
            self.engine_policy.validate_project(&compose)?;
            // Validate before announcing: a project that cannot start is better
            // refused here than half-started later.
            let namespace = self.project_namespace(&compose);
            crate::manifest::validate_offline(&compose, &namespace)?;

            crate::lockfile::attach(&mut compose)?;

            let project = Project::open(
                &self.daemon_namespace,
                namespace.clone(),
                compose,
                Arc::clone(&self.engine),
                self.engine_url.clone(),
            )
            .await?;

            crate::report::daemon_line(
                &format!("project {} loaded into {namespace}", key.display()),
                false,
            );
            Ok(project)
        })
        .await
        .cloned()
    }

    /// Every project that finished loading. A cell still being filled has no
    /// project to act on yet, so it is skipped rather than waited for: the
    /// caller asking for a list must not block on somebody else's `up`.
    async fn loaded(&self) -> Vec<Arc<Project>> {
        self.projects
            .lock()
            .await
            .values()
            .filter_map(|cell| cell.get().cloned())
            .collect()
    }

    /// Every project this daemon knows, for `compose::list`.
    pub async fn list(&self) -> Vec<serde_json::Value> {
        let projects: Vec<Arc<Project>> = self.loaded().await;
        let mut listed = Vec::new();
        for project in projects {
            listed.push(serde_json::json!({
                "namespace": project.project_namespace,
                "file": project.file_path(),
                "containers": project.status().await,
            }));
        }
        listed
    }

    /// Brings a project up, loading its file if this is the first time.
    ///
    /// Only `up` falls back to the compose file in the daemon's own directory:
    /// starting compose inside a project and saying `up` should be enough.
    pub async fn up(
        &self,
        file: Option<&Path>,
        container: Option<&str>,
        operation_id: String,
    ) -> Result<OpResult> {
        let file = self.resolve_file(file)?;
        let _mutation = self.lock_mutation(file).await;
        let project = self.prepare_start_project(file, false).await?;
        Ok(project.up(container, operation_id).await)
    }

    /// Brings a project up using only a matching existing lock.
    pub async fn up_frozen(
        &self,
        file: Option<&Path>,
        container: Option<&str>,
        operation_id: String,
    ) -> Result<OpResult> {
        let file = self.resolve_file(file)?;
        let _mutation = self.lock_mutation(file).await;
        let project = self.prepare_start_project(file, true).await?;
        Ok(project.up(container, operation_id).await)
    }

    /// Brings the initial foreground project up until the process is asked to
    /// stop. Remote `compose::up` calls use [`Self::up`] and are not tied to a
    /// signal received by the foreground CLI.
    pub(crate) async fn up_until_shutdown(
        &self,
        file: Option<&Path>,
        container: Option<&str>,
        operation_id: String,
        shutdown: crate::shutdown::ShutdownSignal,
        frozen: bool,
    ) -> Result<Option<OpResult>> {
        let file = self.resolve_file(file)?;
        let _mutation = self.lock_mutation(file).await;
        let project = self.prepare_start_project(file, frozen).await?;
        if shutdown.requested() {
            return Ok(None);
        }
        Ok(project
            .up_until_shutdown(container, operation_id, shutdown)
            .await)
    }

    /// Adds containers to a project's file, then reconciles the project once.
    ///
    /// The file is the operator's, so it is edited rather than rewritten: see
    /// [`crate::edit`]. A request without a selector is pinned to the resolved
    /// version in the compose file. An explicit selector such as `next` stays
    /// in that file and its concrete result is recorded in the compose lock.
    ///
    /// Reconciliation leaves unchanged containers running, restarts existing
    /// declarations whose resolved version changed, and starts declarations
    /// that are new. A failed start leaves the file as edited and reports the
    /// failure: the edit is what was asked for, and undoing it would hide the
    /// reason the project will not start.
    pub async fn add(
        &self,
        file: Option<&Path>,
        workers: &[String],
        operation_id: String,
    ) -> Result<MutationOutcome> {
        let workers = workers
            .iter()
            .cloned()
            .map(crate::edit::WorkerInput::Spec)
            .collect::<Vec<_>>();
        self.add_configured(file, &workers, operation_id).await
    }

    /// Adds worker specs or full container declarations in one atomic file edit.
    pub async fn add_configured(
        &self,
        file: Option<&Path>,
        workers: &[crate::edit::WorkerInput],
        operation_id: String,
    ) -> Result<MutationOutcome> {
        if workers.is_empty() {
            return Err(ComposeError::InvalidWorkerSpec {
                spec: String::new(),
                reason: "no worker was named. Pass one or more worker=<name|name@version|./path> arguments"
                    .to_string(),
            });
        }

        let path = self.resolve_file(file)?;
        self.validate_engine_policy_file(path)?;
        let asked = workers
            .iter()
            .map(crate::edit::WorkerInput::parse)
            .collect::<Result<Vec<_>>>()?;
        let declarations = coalesce_containers(asked.clone())?;
        let asked_keys: BTreeSet<String> = asked.iter().map(|worker| worker.key.clone()).collect();
        let operation = crate::operation::active(&operation_id);
        // The plan is made against a snapshot, unlocked. Only the file it was
        // made against is ever edited: a file that moved underneath (another
        // add declaring one of this graph's dependencies as `path://`, say)
        // is planned again, and past the limit the add fails for a retry.
        let mut replans = 0;
        let (_mutation, text, wanted, aliases, selected_versions, resolved_graphs) = loop {
            let AddPlan {
                snapshot,
                wanted,
                aliases,
                selected_versions,
                resolved_graphs,
            } = self
                .plan_add(path, &declarations, &asked_keys, &operation_id)
                .await?;
            let mutation = self.lock_mutation(path).await;
            if operation
                .as_ref()
                .is_some_and(|operation| operation.is_cancelled())
            {
                return Err(ComposeError::OperationCancelled { operation_id });
            }
            let text = std::fs::read_to_string(path).map_err(|source| ComposeError::Io {
                path: path.to_path_buf(),
                source,
            })?;
            if text == snapshot {
                break (
                    mutation,
                    text,
                    wanted,
                    aliases,
                    selected_versions,
                    resolved_graphs,
                );
            }
            if replans == ADD_REPLAN_LIMIT {
                return Err(ComposeError::AddPlanStale {
                    path: path.to_path_buf(),
                    replans,
                });
            }
            replans += 1;
            crate::report::daemon_line(
                &format!(
                    "{}: changed while this add resolved its graph; planning again",
                    path.display()
                ),
                true,
            );
        };
        self.validate_engine_policy_text(path, &text)?;
        let declared = crate::ComposeFile::parse(&text, path)?;
        for alias in aliases {
            crate::registry::warn_alias(
                &alias.container,
                &alias.reference,
                Some(&alias.canonical),
                operation.as_deref(),
            )
            .await;
        }
        let mut edited = text.clone();
        let mut added: Vec<String> = Vec::new();
        let mut replaced: Vec<String> = Vec::new();
        let mut restart: Vec<String> = Vec::new();
        for container in &wanted {
            match crate::edit::upsert_container(&edited, container)? {
                crate::edit::Outcome::Unchanged => {}
                crate::edit::Outcome::Added(text) => {
                    edited = text;
                    added.push(container.key.clone());
                }
                crate::edit::Outcome::Replaced { text, from, to } => {
                    edited = text;
                    replaced.push(format!("{} {from} to {to}", container.key));
                    restart.push(container.key.clone());
                }
            }
        }

        let requested = asked
            .iter()
            .map(|worker| worker.key.clone())
            .collect::<Vec<_>>();
        let container = requested[0].clone();

        let yaml_changed = !added.is_empty() || !replaced.is_empty();
        let mut current = crate::ComposeFile::parse(&edited, path)?;
        let package_cache = crate::state::StateStore::package_cache()?;
        let force = selected_versions.keys().cloned().collect();
        let mut prepared = crate::lockfile::prepare_with_versions(
            &mut current,
            &package_cache,
            &force,
            &selected_versions,
        )
        .await?;
        for (root, nodes) in resolved_graphs {
            prepared.replace_graph(&root, nodes);
        }
        restart.retain(|key| {
            current.containers.get(key).is_some_and(|container| {
                matches!(container.worker, crate::config::WorkerSource::Path { .. })
                    || prepared.package_changed(key)
            })
        });
        for key in declared.containers.keys() {
            if prepared.package_changed(key) && !restart.contains(key) {
                restart.push(key.clone());
            }
        }

        if !yaml_changed && !prepared.changed() {
            return Ok(MutationOutcome::from_operations(
                OpStatus::Ok,
                false,
                Some(&container),
                Some(&requested),
                prepared.resolved_version(&container).map(str::to_string),
                std::iter::empty::<&OpResult>(),
            ));
        }

        persist_mutation(path, &text, &edited, &prepared)?;

        let project = self.project(path).await?;
        let root_operation_id = operation_id.clone();
        let (restarted, up, interrupted) = project
            .reconcile_file(current, &restart, operation_id)
            .await;
        if interrupted {
            return Err(ComposeError::OperationCancelled {
                operation_id: root_operation_id,
            });
        }
        let status = if up.status == OpStatus::Failed
            || restarted
                .iter()
                .any(|result| result.status == OpStatus::Failed)
        {
            OpStatus::Failed
        } else {
            OpStatus::Ok
        };
        let version = prepared.resolved_version(&container).map(str::to_string);
        let operations = restarted.iter().chain(std::iter::once(&up));
        Ok(MutationOutcome::from_operations(
            status,
            true,
            Some(&container),
            Some(&requested),
            version,
            operations,
        ))
    }

    /// Everything `add_configured` does before it takes the mutation lock:
    /// expand the asked workers into their registry graphs against the
    /// declaration as it stands, keep the pins the operator already wrote, and
    /// retain the exact registry selections needed to prepare the lock. Registry
    /// calls run unlocked; artifact acquisition happens only after the plan is
    /// confirmed against the same file snapshot.
    /// Returns the text the plan was made against with the plan, so the caller
    /// can tell whether that declaration is still the one it is editing.
    async fn plan_add(
        &self,
        path: &Path,
        declarations: &[crate::edit::NewContainer],
        asked_keys: &BTreeSet<String>,
        operation_id: &str,
    ) -> Result<AddPlan> {
        let snapshot = std::fs::read_to_string(path).map_err(|source| ComposeError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let mut declared = ComposeFile::parse(&snapshot, path.to_path_buf())?;
        crate::lockfile::attach(&mut declared)?;
        let mut plan = crate::dependencies::plan(&declared, declarations).await?;
        let mut selected_versions = std::mem::take(&mut plan.selected_versions);
        plan.containers = keep_declared_dependencies(
            plan.containers,
            asked_keys,
            &declared,
            &mut selected_versions,
        );
        let resolved_graphs = declarations
            .iter()
            .filter(|worker| matches!(worker.source, crate::edit::Source::Package { .. }))
            .map(|worker| {
                (
                    worker.key.clone(),
                    graph_members_for_root(&plan.containers, &worker.key),
                )
            })
            .collect();
        let operation = crate::operation::active(operation_id);
        if operation
            .as_ref()
            .is_some_and(|operation| operation.is_cancelled())
        {
            return Err(ComposeError::OperationCancelled {
                operation_id: operation_id.to_string(),
            });
        }
        Ok(AddPlan {
            snapshot,
            wanted: plan.containers,
            aliases: plan.aliases,
            selected_versions,
            resolved_graphs,
        })
    }

    /// The worker asked for, plus everything it needs, in start order.
    ///
    /// A `path://` worker is taken alone: its dependencies are declared in a
    /// manifest on disk, and resolving those means asking the registry per name
    /// rather than reading one answer. That is worth doing, and is not done
    /// here yet.
    ///
    /// A registry dependency already declared as `path://` is also taken as an
    /// operator-owned boundary. The package keeps its edge to that container,
    /// but neither the local worker nor the package dependencies below it are
    /// added from the registry graph.
    ///
    /// `engine` workers are skipped. They are compiled into the engine and are
    /// already serving before compose starts anything; declaring one would
    /// produce a container with no artefact to install.
    async fn expand(
        &self,
        asked: &crate::edit::NewContainer,
        path_workers: &BTreeSet<String>,
    ) -> Result<(Vec<crate::edit::NewContainer>, BTreeMap<String, String>)> {
        let crate::edit::Source::Package { reference, version } = &asked.source else {
            return Ok((vec![asked.clone()], BTreeMap::new()));
        };

        let range = version.clone().unwrap_or_else(|| "*".to_string());
        let graph = crate::registry::resolve_graph(&asked.key, reference, &range).await?;
        let selected_versions = graph
            .nodes
            .iter()
            .map(|node| (node.name.clone(), node.version.clone()))
            .collect::<BTreeMap<_, _>>();
        let containers = expand_graph(asked, reference, graph, path_workers)?;
        let selected_versions = containers
            .iter()
            .filter(|container| matches!(&container.source, crate::edit::Source::Package { .. }))
            .filter_map(|container| {
                selected_versions
                    .get(&container.key)
                    .map(|version| (container.key.clone(), version.clone()))
            })
            .collect();
        Ok((containers, selected_versions))
    }

    /// Moves declared containers to other versions of the same packages.
    ///
    /// `worker=state` refreshes the selector already declared in the file,
    /// including an exact version. `worker=state@latest` explicitly moves it
    /// to the registry's latest channel. The complete dependency graph is
    /// resolved again and generated dependencies are reconciled with it.
    /// An empty worker list selects every declared package at its latest
    /// version, using its existing registry reference. Path workers are skipped.
    ///
    /// The complete batch is validated and edited in memory before one atomic
    /// write. A changed batch restarts the project once.
    pub async fn update(
        &self,
        file: Option<&Path>,
        workers: &[String],
        operation_id: String,
    ) -> Result<MutationOutcome> {
        let path = self.resolve_file(file)?;
        let _mutation = self.lock_mutation(path).await;
        let text = std::fs::read_to_string(path).map_err(|source| ComposeError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let compose = crate::ComposeFile::parse(&text, path)?;
        self.engine_policy.validate_project(&compose)?;
        let asked = workers_to_update(&compose, workers)?;
        let requested = asked
            .iter()
            .map(|worker| worker.key.clone())
            .collect::<Vec<_>>();
        let primary = requested.first().map(String::as_str);
        let asked = coalesce_containers(asked)?;
        let previous_graphs = crate::lockfile::graphs(path)?;
        let previous_graph_nodes = previous_graphs
            .values()
            .flatten()
            .cloned()
            .collect::<BTreeSet<_>>();
        let path_workers = compose
            .containers
            .iter()
            .filter(|(_, container)| {
                matches!(container.worker, crate::config::WorkerSource::Path { .. })
            })
            .map(|(key, _)| key.clone())
            .collect::<BTreeSet<_>>();
        let mut roots = Vec::with_capacity(asked.len());
        let mut all_explicit_exact_unchanged = true;
        for worker in &asked {
            let crate::edit::Source::Package { version, .. } = &worker.source else {
                return Err(ComposeError::NotAPackageContainer {
                    container: worker.key.clone(),
                    kind: "path".to_string(),
                });
            };
            let Some(container) = compose.containers.get(&worker.key) else {
                return Err(ComposeError::UnknownContainer {
                    container: worker.key.clone(),
                });
            };
            let crate::config::WorkerSource::Package { reference } = &container.worker else {
                return Err(ComposeError::NotAPackageContainer {
                    container: worker.key.clone(),
                    kind: "path".to_string(),
                });
            };

            let current_selector = container.version.as_deref().unwrap_or("*");
            all_explicit_exact_unchanged &= version.as_deref().is_some_and(|version| {
                version == current_selector && semver::Version::parse(version).is_ok()
            });
            roots.push(crate::edit::NewContainer {
                key: worker.key.clone(),
                source: crate::edit::Source::Package {
                    reference: reference.clone(),
                    version: Some(update_selector(version.as_deref(), current_selector)),
                },
                start_after: Vec::new(),
                fields: serde_yaml::Mapping::new(),
            });
        }

        if all_explicit_exact_unchanged {
            return Ok(MutationOutcome::from_operations(
                OpStatus::Ok,
                false,
                primary,
                Some(&requested),
                primary.and_then(|primary| {
                    compose
                        .containers
                        .get(primary)
                        .and_then(|container| container.version.clone())
                }),
                std::iter::empty::<&OpResult>(),
            ));
        }

        let path_workers = &path_workers;
        let mut expanded = futures::stream::iter(roots.into_iter().enumerate().map(
            |(index, worker)| async move { (index, self.expand(&worker, path_workers).await) },
        ))
        .buffer_unordered(4)
        .collect::<Vec<_>>()
        .await;
        expanded.sort_by_key(|(index, _)| *index);

        let mut wanted = Vec::new();
        let mut selected_versions = BTreeMap::new();
        let mut resolved_graphs = BTreeMap::new();
        for (index, expansion) in expanded {
            let (containers, versions) = expansion?;
            let nodes = graph_members(&containers);
            resolved_graphs.insert(asked[index].key.clone(), nodes);
            for (container, version) in versions {
                if let Some(current) = selected_versions.insert(container.clone(), version.clone())
                    && current != version
                {
                    return Err(ComposeError::InvalidWorkerSpec {
                        spec: container.clone(),
                        reason: format!(
                            "the requested workers resolve container '{container}' to conflicting \
                             versions {current} and {version}"
                        ),
                    });
                }
            }
            wanted.extend(containers);
        }
        let mut wanted = coalesce_containers(wanted)?;
        for container in &mut wanted {
            if let Some(existing) = compose.containers.get(&container.key) {
                container.start_after.extend(
                    existing
                        .start_after
                        .iter()
                        .filter(|dependency| !previous_graph_nodes.contains(*dependency))
                        .cloned(),
                );
                container.start_after.sort();
                container.start_after.dedup();
            }
        }

        let stale = stale_graph_members(&previous_graphs, &resolved_graphs);
        let mut edited = text.clone();
        let mut yaml_changed = false;
        for worker in &wanted {
            match crate::edit::upsert_container(&edited, worker)? {
                crate::edit::Outcome::Unchanged => {}
                crate::edit::Outcome::Replaced { text, .. } | crate::edit::Outcome::Added(text) => {
                    edited = text;
                    yaml_changed = true;
                }
            }
        }
        for container in stale {
            let removable = compose.containers.get(&container).is_some_and(|container| {
                matches!(
                    container.worker,
                    crate::config::WorkerSource::Package { .. }
                )
            }) && crate::edit::is_generated_container(&edited, &container)?;
            if removable && let Some(next) = crate::edit::remove_container(&edited, &container)? {
                edited = next;
                yaml_changed = true;
            }
        }

        let mut current = crate::ComposeFile::parse(&edited, path)?;
        let topology_changed = runtime_topology_changed(&compose, &current);
        let package_cache = crate::state::StateStore::package_cache()?;
        let force = selected_versions.keys().cloned().collect();
        let mut prepared = crate::lockfile::prepare_with_versions(
            &mut current,
            &package_cache,
            &force,
            &selected_versions,
        )
        .await?;
        for (root, nodes) in resolved_graphs {
            prepared.replace_graph(&root, nodes);
        }
        let package_changed = selected_versions
            .keys()
            .any(|container| prepared.package_changed(container));
        let version = primary
            .and_then(|primary| prepared.resolved_version(primary))
            .map(str::to_string);

        if !yaml_changed && !prepared.changed() {
            return Ok(MutationOutcome::from_operations(
                OpStatus::Ok,
                false,
                primary,
                Some(&requested),
                version,
                std::iter::empty::<&OpResult>(),
            ));
        }

        persist_mutation(path, &text, &edited, &prepared)?;

        if !package_changed && !topology_changed {
            return Ok(MutationOutcome::from_operations(
                OpStatus::Ok,
                true,
                primary,
                Some(&requested),
                version,
                std::iter::empty::<&OpResult>(),
            ));
        }

        // The whole project, not just this container, and deliberately so. A
        // cached project is the file as it was read, so the new version is only
        // picked up once the project is dropped and re-read — and dropping it
        // while its other children run would leave them supervised by nothing.
        // `compose::restart worker=` is the surgical one; this is the safe one.
        let (down, up) = self.restart_project(path, None, &operation_id).await?;
        Ok(MutationOutcome::from_operations(
            up.status,
            true,
            primary,
            Some(&requested),
            version,
            [&down, &up].into_iter(),
        ))
    }

    /// Removes declared workers and reconciles the running project once.
    ///
    /// Dependency edges pointing at removed workers are deleted with them.
    /// The edited declaration is fully validated before the file or any
    /// process changes. Only the removed containers stop; normal idempotent
    /// `up` then starts anything else that was already missing.
    pub async fn remove(
        &self,
        file: Option<&Path>,
        workers: &[String],
        operation_id: String,
    ) -> Result<MutationOutcome> {
        if workers.is_empty() {
            return Err(ComposeError::InvalidWorkerSpec {
                spec: String::new(),
                reason: "no worker was named. Pass one or more worker=<name> arguments".to_string(),
            });
        }
        let workers = workers
            .iter()
            .map(|worker| {
                let worker = worker.trim();
                if worker.is_empty() {
                    return Err(ComposeError::InvalidWorkerSpec {
                        spec: worker.to_string(),
                        reason: "worker names cannot be blank".to_string(),
                    });
                }
                Ok(worker.to_string())
            })
            .collect::<Result<Vec<_>>>()?;

        let path = self.resolve_file(file)?;
        let _mutation = self.lock_mutation(path).await;
        let text = std::fs::read_to_string(path).map_err(|source| ComposeError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        self.validate_engine_policy_text(path, &text)?;
        let requested = workers.iter().map(String::as_str).collect::<BTreeSet<_>>();
        let removal_order = crate::ComposeFile::parse(&text, path)?
            .start_order()?
            .into_iter()
            .rev()
            .filter(|worker| requested.contains(worker.as_str()))
            .collect::<Vec<_>>();
        let mut edited = text.clone();
        for worker in &workers {
            let Some(next) = crate::edit::remove_container(&edited, worker)? else {
                return Err(ComposeError::UnknownContainer {
                    container: worker.to_string(),
                });
            };
            edited = next;
        }

        let mut current = crate::ComposeFile::parse(&edited, path)?;
        self.engine_policy.validate_project(&current)?;
        let namespace = self.project_namespace(&current);
        crate::manifest::validate_offline(&current, &namespace)?;
        let prepared = crate::lockfile::prepare_metadata(&mut current, &BTreeSet::new()).await?;

        // Claim or load the old project before replacing the file: cleanup of
        // the removed container needs its old scripts and environment.
        let project = self.project(path).await?;
        persist_mutation(path, &text, &edited, &prepared)?;

        let (stopped, up) = project
            .reconcile_removals(current, &removal_order, operation_id)
            .await;
        let status = if up.status == OpStatus::Failed
            || stopped
                .iter()
                .any(|result| result.status == OpStatus::Failed)
        {
            OpStatus::Failed
        } else {
            OpStatus::Ok
        };
        let operations = stopped.iter().chain(std::iter::once(&up));
        Ok(MutationOutcome::from_operations(
            status,
            true,
            workers.first().map(String::as_str),
            Some(&workers),
            None,
            operations,
        ))
    }

    /// Stops a project and starts it again, or bounces one container of it.
    ///
    /// Named, the container is the only thing that stops and starts: not what
    /// it depends on, and not what depends on it. Compose could restart the
    /// dependents to hide the drop, but which of them tolerate one is the
    /// operator's knowledge, not compose's.
    ///
    /// Whole-project is down then up, and nothing cleverer yet: no rolling
    /// restart, no keeping what did not change. It is the shape every later
    /// refinement will be measured against, so it is worth having plainly
    /// first.
    pub async fn restart(
        &self,
        file: Option<&Path>,
        container: Option<&str>,
        operation_id: String,
    ) -> Result<MutationOutcome> {
        let path = self.resolve_file(file)?;
        self.validate_engine_policy_file(path)?;
        if let Some(key) = container {
            let _mutation = self.lock_mutation(path).await;
            let project = self.prepare_start_project(path, false).await?;
            let result = project.restart_one(key, operation_id).await;
            return Ok(MutationOutcome::from_operations(
                result.status,
                result.changed,
                Some(key),
                None,
                None,
                std::iter::once(&result),
            ));
        }

        let _mutation = self.lock_mutation(path).await;
        let (down, up) = self.restart_project(path, None, &operation_id).await?;
        Ok(MutationOutcome::from_operations(
            up.status,
            down.changed || up.changed,
            None,
            None,
            None,
            [&down, &up].into_iter(),
        ))
    }

    /// The two halves of a restart, with the project re-read between them.
    ///
    /// Dropped from the cache only once its children are stopped: the entry
    /// owns their handles, and forgetting it while they run would leave them
    /// supervised by nothing. Re-reading is the point — a project is held as
    /// its file was when it was first loaded, so without this a restart would
    /// start exactly what was already running and report success.
    /// The caller holds the compose file's mutation lock.
    async fn restart_project(
        &self,
        path: &Path,
        container: Option<&str>,
        operation_id: &str,
    ) -> Result<(OpResult, OpResult)> {
        let down = self
            .down(Some(path), container, format!("{operation_id}-down"))
            .await?;
        self.forget(path).await;
        let project = self.prepare_start_project(path, false).await?;
        let up = project.up(container, format!("{operation_id}-up")).await;
        Ok((down, up))
    }

    /// Drops a project from the cache, so the next call re-reads its file.
    ///
    /// Only safe once the project is stopped: the cached entry owns the child
    /// handles, and forgetting it while they run would leave them supervised by
    /// nothing.
    async fn forget(&self, file: &Path) {
        let key = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
        self.projects.lock().await.remove(&key);
    }

    /// Acquires the mutation lock for one canonical compose-file path.
    async fn lock_mutation(&self, file: &Path) -> tokio::sync::OwnedMutexGuard<()> {
        let key = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
        let lock = {
            let mut mutations = self.mutations.lock().await;
            Arc::clone(
                mutations
                    .entry(key)
                    .or_insert_with(|| Arc::new(Mutex::new(()))),
            )
        };
        lock.lock_owned().await
    }

    /// Takes a project down.
    pub async fn down(
        &self,
        file: Option<&Path>,
        container: Option<&str>,
        operation_id: String,
    ) -> Result<OpResult> {
        let path = self.resolve_file(file)?;
        self.validate_engine_policy_file(path)?;
        let project = self.project(path).await?;
        Ok(project.down(container, operation_id).await)
    }

    /// The file a call meant: the one it named, else `worker-compose.yaml` in
    /// the daemon's own directory when that exists.
    fn resolve_file<'a>(&self, file: Option<&'a Path>) -> Result<&'a Path> {
        static DEFAULT: &str = crate::cli::DEFAULT_COMPOSE_FILE;
        match file {
            Some(file) => Ok(file),
            None if Path::new(DEFAULT).exists() => Ok(Path::new(DEFAULT)),
            None => Err(ComposeError::NoComposeFileHere { expected: DEFAULT }),
        }
    }

    /// Checks a declaration without taking it on.
    ///
    /// The file is read and answered for, and nothing is kept: this is the
    /// call a CI job makes, and it must not leave the daemon holding a project.
    pub async fn validate(&self, file: Option<&Path>) -> Result<crate::manifest::ValidationReport> {
        let file = self.resolve_file(file)?;
        if file.is_relative() && !file.exists() {
            return Err(ComposeError::RelativeFileMissing {
                path: file.to_path_buf(),
                cwd: std::env::current_dir().unwrap_or_default(),
            });
        }
        let compose = ComposeFile::load(file)?;
        self.engine_policy.validate_project(&compose)?;
        let namespace = self.project_namespace(&compose);
        crate::manifest::validate_offline(&compose, &namespace)
    }

    pub async fn status(&self, file: Option<&Path>) -> Result<Arc<Project>> {
        let path = self.resolve_file(file)?;
        self.validate_engine_policy_file(path)?;
        self.project(path).await
    }

    pub async fn logs(
        &self,
        file: Option<&Path>,
        container: Option<&str>,
        cursors: BTreeMap<String, LogCursor>,
        tail: usize,
        stream: Option<LogStream>,
        wait_ms: u64,
    ) -> Result<LogsOutcome> {
        let project = self.status(file).await?;
        project
            .logs(container, cursors, tail, stream, wait_ms)
            .await
    }

    fn validate_engine_policy_file(&self, path: &Path) -> Result<()> {
        if path.is_relative() && !path.exists() {
            return Err(ComposeError::RelativeFileMissing {
                path: path.to_path_buf(),
                cwd: std::env::current_dir().unwrap_or_default(),
            });
        }
        let text = std::fs::read_to_string(path).map_err(|source| ComposeError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        self.validate_engine_policy_text(path, &text)
    }

    fn validate_engine_policy_text(&self, path: &Path, text: &str) -> Result<()> {
        let engine = crate::config::parse_engine_section(text, path)?;
        self.engine_policy
            .validate_engine_section(path, engine.as_ref())
    }

    /// Asks the daemon to shut down, and reports what it is about to stop.
    ///
    /// The teardown happens on the serve loop rather than here: the caller is
    /// waiting on this invocation, and a daemon that tore its engine connection
    /// down mid-reply would leave them holding a broken socket instead of an
    /// answer.
    pub async fn request_stop(&self) -> serde_json::Value {
        self.operations.cancel_all().await;
        self.stop_requested
            .store(true, std::sync::atomic::Ordering::SeqCst);

        let projects = self.projects.lock().await;
        serde_json::json!({
            "daemon": self.worker_name,
            "daemon_pid": std::process::id(),
            "stopping": projects.keys().map(|p| p.display().to_string()).collect::<Vec<_>>(),
        })
    }

    pub fn stop_requested(&self) -> bool {
        self.stop_requested
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Intentional shutdown: every project goes down, then the connection.
    pub async fn shutdown(&self) {
        let projects: Vec<Arc<Project>> = self.loaded().await;
        for project in projects {
            project.shutdown().await;
        }
        self.engine.shutdown().await;
    }

    /// Leaves without touching what was not started here. Used when the engine
    /// refuses this daemon's registration.
    pub async fn abandon(&self) {
        let projects: Vec<Arc<Project>> = self.loaded().await;
        for project in projects {
            project.abandon().await;
        }
        self.engine.shutdown().await;
    }

    /// Starts the loop that notices a child dying after it was ready, and the
    /// connection coming back.
    ///
    /// One loop for every project rather than one per project: it holds each
    /// project's lock for microseconds at a time, and a daemon with ten
    /// projects should not cost ten timers.
    fn supervise(daemon: &Arc<Self>) {
        let weak = Arc::downgrade(daemon);
        tokio::spawn(async move {
            let mut was_connected = true;
            loop {
                tokio::time::sleep(SUPERVISION_INTERVAL).await;
                let Some(daemon) = weak.upgrade() else { return };

                let connected = daemon.engine.is_connected();
                let reconnected = connected && !was_connected;
                was_connected = connected;

                let projects: Vec<Arc<Project>> = daemon.loaded().await;
                for project in projects {
                    if reconnected {
                        project.reconcile_after_reconnect().await;
                    }
                    project.reap_unexpected_exits().await;
                    // After the reap, so a container that has just exited
                    // spends its first attempt on the tick that noticed rather
                    // than waiting for the next one.
                    project.drive_restarts().await;
                }
            }
        });
    }
}

/// The public result of a compose mutation.
///
/// Full per-container state remains available through status, logs, and daemon tracing.
#[derive(Debug, Clone, serde::Serialize, schemars::JsonSchema, PartialEq, Eq)]
pub struct MutationOutcome {
    status: OpStatus,
    changed: bool,
    /// The primary worker named by a targeted mutation.
    #[serde(skip_serializing_if = "Option::is_none")]
    worker: Option<String>,
    /// Every explicitly requested worker when more than one was supplied.
    #[serde(skip_serializing_if = "Option::is_none")]
    workers: Option<Vec<String>>,
    /// Resolved package version when the mutation resolves one.
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
    /// Other workers that the mutation had to change.
    #[serde(skip_serializing_if = "Option::is_none")]
    affected_workers: Option<Vec<String>>,
    /// Concise failure for the first worker that could not reach its target state.
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<MutationError>,
    /// Workers that failed while the operation still succeeded, which is only
    /// possible for a container declaring `required: false`.
    ///
    /// `status: ok` used to mean every planned container is up. It now means
    /// every *required* one is, so the return has to name the rest rather than
    /// leave a caller to compare the plan against a later status call.
    #[serde(skip_serializing_if = "Option::is_none")]
    not_required_failures: Option<Vec<String>>,
}

impl MutationOutcome {
    /// Projects internal reconciliation results into the small mutation contract.
    pub(crate) fn from_operations<'a>(
        status: OpStatus,
        changed: bool,
        worker: Option<&str>,
        workers: Option<&[String]>,
        version: Option<String>,
        operations: impl Iterator<Item = &'a OpResult>,
    ) -> Self {
        let requested: std::collections::BTreeSet<&str> = workers
            .unwrap_or(&[])
            .iter()
            .map(String::as_str)
            .chain(worker)
            .collect();
        let mut affected_workers = std::collections::BTreeSet::new();
        let mut primary_error = None;
        let mut first_container_error = None;
        let mut failed = Vec::new();

        for operation in operations {
            if primary_error.is_none() {
                primary_error = operation.primary_error.as_ref().map(MutationError::from);
            }
            for result in &operation.containers {
                if result.changed && !requested.contains(result.container.as_str()) {
                    affected_workers.insert(result.container.clone());
                }
                if result.error.is_some() {
                    failed.push(result.container.clone());
                }
                if first_container_error.is_none() {
                    first_container_error = result.error.as_ref().map(MutationError::from);
                }
            }
        }

        let error = if status == OpStatus::Failed {
            primary_error.or(first_container_error)
        } else {
            first_container_error
        };
        // A succeeding operation with a failed container is the `required:
        // false` case and nothing else: a required failure is what makes the
        // status `failed` in the first place.
        let not_required_failures =
            (status == OpStatus::Ok && !failed.is_empty()).then_some(failed);

        Self {
            status,
            changed,
            worker: worker.map(str::to_owned),
            workers: workers
                .filter(|workers| workers.len() > 1)
                .map(|workers| workers.to_vec()),
            version,
            affected_workers: (!affected_workers.is_empty())
                .then(|| affected_workers.into_iter().collect()),
            error,
            not_required_failures,
        }
    }

    pub(crate) fn is_failed(&self) -> bool {
        self.status == OpStatus::Failed
    }
}

#[derive(Debug, Clone, serde::Serialize, schemars::JsonSchema, PartialEq, Eq)]
struct MutationError {
    code: String,
    message: String,
}

impl From<&crate::lifecycle::OpError> for MutationError {
    fn from(error: &crate::lifecycle::OpError) -> Self {
        let message = if error.code == "CHILD_EXITED_BEFORE_REGISTRATION" {
            "Worker exited before registration".to_string()
        } else {
            error
                .message
                .split(". It last said:\n")
                .next()
                .unwrap_or(&error.message)
                .to_string()
        };

        Self {
            code: error.code.clone(),
            message,
        }
    }
}

/// What every compose daemon registers as.
///
/// Fixed, and the exclusion depends on it: `(default, compose)` is a lease the
/// engine hands to one connection, so a second daemon is refused at
/// registration rather than left running unreachable.
pub const DAEMON_WORKER_NAME: &str = "compose";

/// Writes through a temporary file in the same directory, then renames.
///
/// A compose file half-written is a project that will not start and an operator
/// with no copy of what it said before. The rename is atomic within a
/// filesystem, so a reader sees the old file or the new one.
fn write_atomically(path: &Path, text: &str) -> Result<()> {
    use std::io::Write;

    let permissions = std::fs::metadata(path)
        .map_err(|source| ComposeError::Io {
            path: path.to_path_buf(),
            source,
        })?
        .permissions();
    let temp = path.with_extension(format!("compose-edit-{}.tmp", uuid::Uuid::new_v4()));
    let mut file = open_private_temp(&temp).map_err(|source| ComposeError::Io {
        path: temp.clone(),
        source,
    })?;
    if let Err(source) = file.set_permissions(permissions) {
        drop(file);
        let _ = std::fs::remove_file(&temp);
        return Err(ComposeError::Io { path: temp, source });
    }
    if let Err(source) = file.write_all(text.as_bytes()) {
        drop(file);
        let _ = std::fs::remove_file(&temp);
        return Err(ComposeError::Io { path: temp, source });
    }
    if let Err(source) = file.sync_all() {
        drop(file);
        let _ = std::fs::remove_file(&temp);
        return Err(ComposeError::Io { path: temp, source });
    }
    drop(file);
    std::fs::rename(&temp, path).map_err(|source| {
        let _ = std::fs::remove_file(&temp);
        ComposeError::Io {
            path: path.to_path_buf(),
            source,
        }
    })
}

/// Persists one compose mutation while keeping its YAML and lock consistent.
///
/// The lock writer is atomic by itself. If it fails after the compose file was
/// replaced, restore the previous compose text while the mutation lock is held.
fn persist_mutation(
    path: &Path,
    previous: &str,
    edited: &str,
    prepared: &crate::lockfile::PreparedLock,
) -> Result<()> {
    let compose_changed = previous != edited;
    if compose_changed {
        write_atomically(path, edited)?;
    }
    if let Err(lock_error) = prepared.write_if_changed() {
        if compose_changed && let Err(rollback_error) = write_atomically(path, previous) {
            return Err(ComposeError::MutationRollbackFailed {
                path: path.to_path_buf(),
                lock_error: lock_error.to_string(),
                rollback_error: rollback_error.to_string(),
            });
        }
        return Err(lock_error);
    }
    Ok(())
}

/// Creates an empty, collision-safe staging file, with mode 0600 on Unix.
fn open_private_temp(path: &Path) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

/// Drop graph-expanded dependencies the compose file already declares.
///
/// `compose::add worker=X` resolves X's whole dependency graph. Only nodes not
/// yet declared are added, plus what the caller explicitly asked for: a pin the
/// operator wrote stays theirs, and `compose::update worker=<dep>` is how a
/// version moves. A declared dependency whose pin differs from what the
/// registry resolved is logged, never rewritten or forced into the lock.
fn keep_declared_dependencies(
    wanted: Vec<crate::edit::NewContainer>,
    asked: &BTreeSet<String>,
    declared: &ComposeFile,
    selected_versions: &mut BTreeMap<String, String>,
) -> Vec<crate::edit::NewContainer> {
    let wanted = wanted
        .into_iter()
        .filter(|worker| {
            if asked.contains(&worker.key) {
                return true;
            }
            let Some(existing) = declared.containers.get(&worker.key) else {
                return true;
            };
            if let crate::edit::Source::Package {
                version: Some(resolved),
                ..
            } = &worker.source
                && existing.version.as_deref() != Some(resolved.as_str())
            {
                crate::report::daemon_line(
                    &format!(
                        "{}: kept the declared version {} (the registry resolved {resolved} for this \
                         add); run compose::update worker={} to move it",
                        worker.key,
                        existing.version.as_deref().unwrap_or("unpinned"),
                        worker.key
                    ),
                    true,
                );
            }
            false
        })
        .collect::<Vec<_>>();
    let wanted_keys = wanted
        .iter()
        .map(|worker| worker.key.as_str())
        .collect::<BTreeSet<_>>();
    selected_versions.retain(|key, _| wanted_keys.contains(key.as_str()));
    wanted
}

/// Turns one registry answer into the declarations Compose can own.
///
/// Engine-kind dependencies are omitted because the engine already provides
/// them. Existing local dependencies are kept as opaque boundaries because
/// their source and dependency tree belong to the operator. An engine-kind
/// root is different: silently omitting the exact worker the caller requested
/// would make `compose::add` report success without changing the project, so
/// reject it with migration guidance instead.
pub(crate) fn expand_graph(
    asked: &crate::edit::NewContainer,
    reference: &str,
    graph: crate::registry::Graph,
    path_workers: &BTreeSet<String>,
) -> Result<Vec<crate::edit::NewContainer>> {
    if let Some(root) = graph.nodes.iter().find(|node| node.name == asked.key)
        && root.kind == "engine"
    {
        let guidance = if crate::config::CONFIGURABLE_ENGINE_WORKERS.contains(&root.name.as_str()) {
            format!("Configure it under engine.workers.{} instead.", root.name)
        } else {
            "It is injected automatically and must not be declared.".to_string()
        };
        return Err(ComposeError::EngineWorkerIsBuiltin {
            container: asked.key.clone(),
            name: root.name.clone(),
            guidance,
        });
    }

    let host = reference
        .rsplit_once('/')
        .map(|(host, _)| host)
        .unwrap_or("");

    let nodes: BTreeMap<&str, &crate::registry::Node> = graph
        .nodes
        .iter()
        .map(|node| (node.name.as_str(), node))
        .collect();

    // A local declaration is an operator-owned implementation of that worker.
    // Keep it as the dependency boundary instead of replacing it with the
    // registry package or inheriting the published package's dependency tree.
    let mut reachable = BTreeSet::new();
    let mut satisfied_by_path = BTreeSet::new();
    let mut visit = vec![asked.key.clone()];
    while let Some(name) = visit.pop() {
        if name != asked.key && path_workers.contains(&name) {
            satisfied_by_path.insert(name);
            continue;
        }
        if nodes
            .get(name.as_str())
            .is_some_and(|node| node.kind == "engine")
        {
            continue;
        }
        if !reachable.insert(name.clone()) {
            continue;
        }
        visit.extend(
            graph
                .edges
                .iter()
                .filter(|(from, to)| from == &name && to != &name)
                .map(|(_, to)| to.clone()),
        );
    }

    let mut declarable = if satisfied_by_path.is_empty() {
        graph
            .nodes
            .iter()
            .filter(|node| node.kind != "engine")
            .map(|node| node.name.clone())
            .collect()
    } else {
        reachable
    };
    declarable.insert(asked.key.clone());

    let needs = |name: &str| -> Vec<String> {
        let mut needed: Vec<String> = graph
            .edges
            .iter()
            .filter(|(from, to)| {
                from == name
                    && to != name
                    && (declarable.contains(to) || satisfied_by_path.contains(to))
            })
            .map(|(_, to)| to.clone())
            .collect();
        needed.sort();
        needed.dedup();
        needed
    };

    let container = |node: &crate::registry::Node| crate::edit::NewContainer {
        key: node.name.clone(),
        source: crate::edit::Source::Package {
            reference: if host.is_empty() {
                node.name.clone()
            } else {
                format!("{host}/{}", node.name)
            },
            version: Some(node.version.clone()),
        },
        start_after: needs(&node.name),
        fields: serde_yaml::Mapping::new(),
    };

    // Registry nodes are a set, not an ordered plan. Derive a deterministic
    // dependency-first order from the edges, preferring the requested root
    // only after other ready nodes so independent registry entries cannot put
    // it before a dependency that becomes ready in the same wave.
    let mut pending: BTreeMap<String, usize> = BTreeMap::new();
    let mut dependents: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for name in &declarable {
        let dependencies: Vec<String> = needs(name)
            .into_iter()
            .filter(|dependency| declarable.contains(dependency))
            .collect();
        pending.insert(name.clone(), dependencies.len());
        for dependency in dependencies {
            dependents.entry(dependency).or_default().push(name.clone());
        }
    }
    for entries in dependents.values_mut() {
        entries.sort();
        entries.dedup();
    }

    let mut ready: std::collections::BTreeSet<String> = pending
        .iter()
        .filter(|(_, count)| **count == 0)
        .map(|(name, _)| name.clone())
        .collect();
    let mut order = Vec::with_capacity(declarable.len());
    while !ready.is_empty() {
        let next = ready
            .iter()
            .find(|name| name.as_str() != asked.key)
            .or_else(|| ready.iter().next())
            .cloned()
            .expect("ready is known to be non-empty");
        ready.remove(&next);
        order.push(next.clone());

        for dependent in dependents.get(&next).cloned().unwrap_or_default() {
            if let Some(count) = pending.get_mut(&dependent) {
                *count -= 1;
                if *count == 0 {
                    ready.insert(dependent);
                }
            }
        }
    }
    if order.len() != declarable.len() {
        return Err(ComposeError::DependencyCycle {
            path: "unresolved registry dependencies".to_string(),
        });
    }

    order
        .into_iter()
        .map(|name| {
            if name == asked.key {
                let mut root = asked.clone();
                root.start_after = needs(&asked.key);
                if let Some(node) = nodes.get(asked.key.as_str()) {
                    let version = match &root.source {
                        crate::edit::Source::Package {
                            version: Some(version),
                            ..
                        } => version.clone(),
                        _ => node.version.clone(),
                    };
                    root.source = crate::edit::Source::Package {
                        reference: reference.to_string(),
                        version: Some(version),
                    };
                }
                Ok(root)
            } else {
                nodes.get(name.as_str()).map(|node| container(node)).ok_or(
                    ComposeError::UnknownDependency {
                        container: asked.key.clone(),
                        dependency: name,
                    },
                )
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_without_workers_selects_packages_with_their_declared_references() {
        let tmp = tempfile::tempdir().unwrap();
        let compose = crate::ComposeFile::parse(
            r#"
containers:
  database:
    worker: package://private.example/team/state
    version: '1.0.0'
  api:
    worker: path://.
    scripts:
      run: ./api
  cache:
    worker: package://api.workers.iii.dev/cache
    version: '2.0.0'
    start_after: [database]
"#,
            tmp.path().join("worker-compose.yaml"),
        )
        .unwrap();

        let selected = workers_to_update(&compose, &[]).unwrap();

        assert_eq!(
            selected,
            vec![
                crate::edit::NewContainer {
                    key: "database".to_string(),
                    source: crate::edit::Source::Package {
                        reference: "private.example/team/state".to_string(),
                        version: Some("latest".to_string()),
                    },
                    start_after: Vec::new(),
                    fields: serde_yaml::Mapping::new(),
                },
                crate::edit::NewContainer {
                    key: "cache".to_string(),
                    source: crate::edit::Source::Package {
                        reference: "api.workers.iii.dev/cache".to_string(),
                        version: Some("latest".to_string()),
                    },
                    start_after: vec!["database".to_string()],
                    fields: serde_yaml::Mapping::new(),
                },
            ]
        );
    }

    #[test]
    fn update_with_a_worker_keeps_the_explicit_selection_and_version() {
        let compose = crate::ComposeFile::parse(
            "containers:\n  state:\n    worker: package://state\n    version: '1.0.0'\n  cache:\n    worker: package://cache\n    version: '2.0.0'\n",
            Path::new("worker-compose.yaml"),
        )
        .unwrap();

        let selected = workers_to_update(&compose, &["state@1.2.3".to_string()]).unwrap();

        assert_eq!(
            selected,
            vec![crate::edit::parse_worker("state@1.2.3").unwrap()]
        );
    }

    fn package(name: &str) -> crate::edit::NewContainer {
        crate::edit::NewContainer {
            key: name.to_string(),
            source: crate::edit::Source::Package {
                reference: name.to_string(),
                version: None,
            },
            start_after: Vec::new(),
            fields: serde_yaml::Mapping::new(),
        }
    }

    #[test]
    fn update_without_a_selector_keeps_the_declared_tag() {
        assert_eq!(update_selector(None, "next"), "next");
    }

    #[test]
    fn graph_update_removes_only_nodes_no_root_still_owns() {
        let previous = BTreeMap::from([
            (
                "api".to_string(),
                BTreeSet::from(["api".to_string(), "state".to_string(), "queue".to_string()]),
            ),
            (
                "jobs".to_string(),
                BTreeSet::from(["jobs".to_string(), "queue".to_string()]),
            ),
        ]);
        let replacements = BTreeMap::from([(
            "api".to_string(),
            BTreeSet::from(["api".to_string(), "cache".to_string()]),
        )]);

        assert_eq!(
            stale_graph_members(&previous, &replacements),
            BTreeSet::from(["state".to_string()])
        );
    }

    #[test]
    fn graph_members_include_local_dependency_boundaries() {
        let mut api = package("api");
        api.start_after = vec!["local-state".to_string()];

        assert_eq!(
            graph_members(&[api]),
            BTreeSet::from(["api".to_string(), "local-state".to_string()])
        );
    }

    #[test]
    fn update_without_a_selector_keeps_an_exact_version() {
        assert_eq!(update_selector(None, "0.22.8"), "0.22.8");
    }

    #[test]
    fn update_changes_the_selector_only_when_it_is_explicit() {
        assert_eq!(update_selector(Some("latest"), "0.22.8"), "latest");
    }

    #[test]
    fn expanded_graph_keeps_an_explicit_root_selector() {
        let mut asked = package("state");
        let crate::edit::Source::Package { version, .. } = &mut asked.source else {
            unreachable!();
        };
        *version = Some("next".to_string());
        let graph = crate::registry::Graph {
            nodes: vec![crate::registry::Node {
                name: "state".to_string(),
                version: "0.22.8".to_string(),
                kind: "binary".to_string(),
                ..Default::default()
            }],
            edges: Vec::new(),
        };

        let expanded = expand_graph(&asked, "state", graph, &BTreeSet::new()).unwrap();

        assert_eq!(
            expanded[0].source,
            crate::edit::Source::Package {
                reference: "state".to_string(),
                version: Some("next".to_string()),
            }
        );
    }

    #[test]
    fn configurable_engine_root_points_to_engine_workers() {
        let graph = crate::registry::Graph {
            nodes: vec![crate::registry::Node {
                name: "configuration".to_string(),
                version: "0.23.0".to_string(),
                kind: "engine".to_string(),
                ..Default::default()
            }],
            edges: Vec::new(),
        };

        let error = expand_graph(
            &package("configuration"),
            "configuration",
            graph,
            &BTreeSet::new(),
        )
        .expect_err("engine-owned roots must not become compose containers");
        assert_eq!(error.code(), "ENGINE_WORKER_IS_BUILTIN");
        let message = error.to_string();
        assert!(message.contains("supplied by the engine"), "{message}");
        assert!(
            message.contains("engine.workers.configuration"),
            "{message}"
        );
    }

    #[test]
    fn injected_engine_root_says_no_declaration_is_needed() {
        let graph = crate::registry::Graph {
            nodes: vec![crate::registry::Node {
                name: "iii-engine-functions".to_string(),
                version: "0.23.0".to_string(),
                kind: "engine".to_string(),
                ..Default::default()
            }],
            edges: Vec::new(),
        };

        let error = expand_graph(
            &package("iii-engine-functions"),
            "iii-engine-functions",
            graph,
            &BTreeSet::new(),
        )
        .expect_err("injected engine roots must not become compose containers");
        let message = error.to_string();
        assert!(message.contains("injected automatically"), "{message}");
        assert!(message.contains("must not be declared"), "{message}");
    }

    #[test]
    fn engine_dependencies_are_filtered_from_expanded_graphs() {
        let graph = crate::registry::Graph {
            nodes: vec![
                crate::registry::Node {
                    name: "api".to_string(),
                    version: "1.0.0".to_string(),
                    kind: "binary".to_string(),
                    ..Default::default()
                },
                crate::registry::Node {
                    name: "configuration".to_string(),
                    version: "0.23.0".to_string(),
                    kind: "engine".to_string(),
                    ..Default::default()
                },
                crate::registry::Node {
                    name: "state".to_string(),
                    version: "2.0.0".to_string(),
                    kind: "binary".to_string(),
                    ..Default::default()
                },
            ],
            edges: vec![
                ("api".to_string(), "configuration".to_string()),
                ("api".to_string(), "state".to_string()),
            ],
        };

        let expanded =
            expand_graph(&package("api"), "api", graph, &BTreeSet::new()).expect("expand graph");
        let names: Vec<&str> = expanded.iter().map(|entry| entry.key.as_str()).collect();
        assert_eq!(names, vec!["state", "api"]);
        assert_eq!(expanded[1].start_after, vec!["state"]);
    }

    #[test]
    fn expanded_graph_is_dependency_first_even_when_registry_nodes_are_not() {
        let graph = crate::registry::Graph {
            nodes: vec![
                crate::registry::Node {
                    name: "state".to_string(),
                    version: "2.0.0".to_string(),
                    kind: "binary".to_string(),
                    ..Default::default()
                },
                crate::registry::Node {
                    name: "db".to_string(),
                    version: "3.0.0".to_string(),
                    kind: "binary".to_string(),
                    ..Default::default()
                },
                crate::registry::Node {
                    name: "api".to_string(),
                    version: "1.0.0".to_string(),
                    kind: "binary".to_string(),
                    ..Default::default()
                },
            ],
            edges: vec![
                ("api".to_string(), "state".to_string()),
                ("state".to_string(), "db".to_string()),
            ],
        };

        let expanded =
            expand_graph(&package("api"), "api", graph, &BTreeSet::new()).expect("expand graph");
        let names: Vec<&str> = expanded.iter().map(|entry| entry.key.as_str()).collect();
        assert_eq!(names, vec!["db", "state", "api"]);
    }

    #[test]
    fn expanded_graph_rejects_registry_dependency_cycles() {
        let graph = crate::registry::Graph {
            nodes: vec![
                crate::registry::Node {
                    name: "api".to_string(),
                    version: "1.0.0".to_string(),
                    kind: "binary".to_string(),
                    ..Default::default()
                },
                crate::registry::Node {
                    name: "state".to_string(),
                    version: "2.0.0".to_string(),
                    kind: "binary".to_string(),
                    ..Default::default()
                },
            ],
            edges: vec![
                ("api".to_string(), "state".to_string()),
                ("state".to_string(), "api".to_string()),
            ],
        };

        let error = expand_graph(&package("api"), "api", graph, &BTreeSet::new())
            .expect_err("registry dependency cycles must be rejected");
        assert_eq!(error.code(), "DEPENDENCY_CYCLE");
    }

    #[test]
    fn local_path_worker_satisfies_a_registry_dependency() {
        let graph = crate::registry::Graph {
            nodes: vec![
                crate::registry::Node {
                    name: "tailscale".to_string(),
                    version: "1.0.0".to_string(),
                    kind: "binary".to_string(),
                    ..Default::default()
                },
                crate::registry::Node {
                    name: "console".to_string(),
                    version: "1.9.11".to_string(),
                    kind: "binary".to_string(),
                    ..Default::default()
                },
            ],
            edges: vec![("tailscale".to_string(), "console".to_string())],
        };
        let path_workers = BTreeSet::from(["console".to_string()]);

        let expanded =
            expand_graph(&package("tailscale"), "tailscale", graph, &path_workers).unwrap();

        assert_eq!(
            expanded,
            vec![crate::edit::NewContainer {
                key: "tailscale".to_string(),
                source: crate::edit::Source::Package {
                    reference: "tailscale".to_string(),
                    version: Some("1.0.0".to_string()),
                },
                start_after: vec!["console".to_string()],
                fields: serde_yaml::Mapping::new(),
            }]
        );
    }

    #[test]
    fn local_path_worker_stops_registry_dependency_expansion() {
        let graph = crate::registry::Graph {
            nodes: vec![
                crate::registry::Node {
                    name: "tailscale".to_string(),
                    version: "1.0.0".to_string(),
                    kind: "binary".to_string(),
                    ..Default::default()
                },
                crate::registry::Node {
                    name: "console".to_string(),
                    version: "1.9.11".to_string(),
                    kind: "binary".to_string(),
                    ..Default::default()
                },
                crate::registry::Node {
                    name: "state".to_string(),
                    version: "2.0.0".to_string(),
                    kind: "binary".to_string(),
                    ..Default::default()
                },
            ],
            edges: vec![
                ("tailscale".to_string(), "console".to_string()),
                ("console".to_string(), "state".to_string()),
            ],
        };
        let path_workers = BTreeSet::from(["console".to_string()]);

        let expanded =
            expand_graph(&package("tailscale"), "tailscale", graph, &path_workers).unwrap();

        assert_eq!(
            expanded,
            vec![crate::edit::NewContainer {
                key: "tailscale".to_string(),
                source: crate::edit::Source::Package {
                    reference: "tailscale".to_string(),
                    version: Some("1.0.0".to_string()),
                },
                start_after: vec!["console".to_string()],
                fields: serde_yaml::Mapping::new(),
            }]
        );
    }

    #[test]
    fn dependency_reachable_without_the_local_worker_is_still_declared() {
        let graph = crate::registry::Graph {
            nodes: vec![
                crate::registry::Node {
                    name: "tailscale".to_string(),
                    version: "1.0.0".to_string(),
                    kind: "binary".to_string(),
                    ..Default::default()
                },
                crate::registry::Node {
                    name: "console".to_string(),
                    version: "1.9.11".to_string(),
                    kind: "binary".to_string(),
                    ..Default::default()
                },
                crate::registry::Node {
                    name: "state".to_string(),
                    version: "2.0.0".to_string(),
                    kind: "binary".to_string(),
                    ..Default::default()
                },
            ],
            edges: vec![
                ("tailscale".to_string(), "console".to_string()),
                ("tailscale".to_string(), "state".to_string()),
                ("console".to_string(), "state".to_string()),
            ],
        };
        let path_workers = BTreeSet::from(["console".to_string()]);

        let expanded =
            expand_graph(&package("tailscale"), "tailscale", graph, &path_workers).unwrap();
        let names: Vec<&str> = expanded.iter().map(|worker| worker.key.as_str()).collect();

        assert_eq!(names, vec!["state", "tailscale"]);
    }

    #[test]
    fn explicitly_requested_path_worker_is_still_a_source_conflict() {
        let graph = crate::registry::Graph {
            nodes: vec![crate::registry::Node {
                name: "console".to_string(),
                version: "1.9.11".to_string(),
                kind: "binary".to_string(),
                ..Default::default()
            }],
            edges: Vec::new(),
        };
        let path_workers = BTreeSet::from(["console".to_string()]);
        let expanded = expand_graph(&package("console"), "console", graph, &path_workers).unwrap();
        let text = "containers:\n  console:\n    worker: path://../console\n";

        let error = crate::edit::upsert_container(text, &expanded[0]).unwrap_err();

        assert_eq!(error.code(), "WORKER_SOURCE_CHANGED");
    }

    #[tokio::test]
    async fn managed_daemon_uses_the_policy_engine_url() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("worker-compose.yaml");
        std::fs::write(
            &path,
            "engine: { url: 'ws://127.0.0.1:2/ws', workers: {} }\ncontainers: {}\n",
        )
        .unwrap();
        let compose = ComposeFile::load(&path).unwrap();
        let policy = EnginePolicy::managed(&compose).unwrap();

        let daemon = Daemon::start(
            "ws://127.0.0.1:1/ws".to_string(),
            "managed-url-test".to_string(),
            None,
            policy,
        );

        assert_eq!(daemon.engine_url, "ws://127.0.0.1:2/ws");
    }

    #[test]
    fn identical_shared_dependencies_are_declared_once() {
        let state = crate::edit::parse_worker("state@1.0.0").unwrap();
        let queue = crate::edit::parse_worker("queue@1.0.0").unwrap();

        let merged =
            coalesce_containers(vec![state.clone(), queue.clone(), state.clone()]).unwrap();

        assert_eq!(merged, vec![state, queue]);
    }

    #[test]
    fn conflicting_shared_dependencies_are_rejected_before_editing() {
        let first = crate::edit::parse_worker("state@1.0.0").unwrap();
        let second = crate::edit::parse_worker("state@2.0.0").unwrap();

        for expanded in [vec![first.clone(), second.clone()], vec![second, first]] {
            let error = coalesce_containers(expanded)
                .expect_err("two versions of one shared dependency must not be order-dependent");

            assert_eq!(error.code(), "INVALID_WORKER_SPEC");
            assert!(error.to_string().contains("state"), "{error}");
            assert!(error.to_string().contains("conflicting"), "{error}");
        }
    }
    #[cfg(unix)]
    #[test]
    fn atomic_write_preserves_the_source_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("worker-compose.yaml");
        std::fs::write(&path, "before\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();

        write_atomically(&path, "after\n").unwrap();

        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        assert_eq!(std::fs::read_to_string(path).unwrap(), "after\n");
    }

    #[tokio::test]
    async fn failed_lock_write_restores_the_previous_compose_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("worker-compose.yaml");
        let previous = "containers:\n  api:\n    worker: path://.\n";
        let edited = "namespace: changed\ncontainers:\n  api:\n    worker: path://.\n";
        std::fs::write(&path, previous).unwrap();

        let mut compose = crate::ComposeFile::parse(edited, &path).unwrap();
        let prepared = crate::lockfile::prepare_metadata(&mut compose, &BTreeSet::new())
            .await
            .unwrap();
        std::fs::create_dir(crate::lockfile::lock_path(&path)).unwrap();

        let error = persist_mutation(&path, previous, edited, &prepared)
            .expect_err("the lock path is a directory");

        assert_eq!(error.code(), "IO_ERROR");
        assert_eq!(std::fs::read_to_string(path).unwrap(), previous);
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_staging_file_is_private_from_creation() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("worker-compose.staging");

        let file = open_private_temp(&path).unwrap();

        let mode = file.metadata().unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}

#[cfg(test)]
mod mutation_outcome_tests {
    use super::*;
    use crate::{
        lifecycle::{ContainerResult, OpError},
        state::ChildStatus,
    };

    #[test]
    fn concise_outcome_omits_healthy_containers_and_log_tails() {
        let primary_error = OpError {
            code: "CHILD_EXITED_BEFORE_REGISTRATION".into(),
            message: "container 'tailscale' exited with 1 before it registered. It last said:\nretry secret output".into(),
        };
        let result = OpResult {
            operation_id: "diagnostic-only".into(),
            status: OpStatus::Failed,
            changed: true,
            containers: vec![
                ContainerResult {
                    container: "queue".into(),
                    state: ChildStatus::Ready,
                    changed: false,
                    error: None,
                },
                ContainerResult {
                    container: "console".into(),
                    state: ChildStatus::Ready,
                    changed: true,
                    error: None,
                },
                ContainerResult {
                    container: "tailscale".into(),
                    state: ChildStatus::Failed,
                    changed: false,
                    error: Some(primary_error.clone()),
                },
            ],
            primary_error: Some(primary_error),
        };

        let outcome = MutationOutcome::from_operations(
            OpStatus::Failed,
            true,
            Some("tailscale"),
            None,
            Some("0.1.3-experimental".into()),
            std::iter::once(&result),
        );

        let encoded = serde_json::to_value(&outcome).unwrap();
        assert_eq!(encoded["status"], "failed");
        assert_eq!(encoded["worker"], "tailscale");
        assert_eq!(encoded["affected_workers"], serde_json::json!(["console"]));
        assert_eq!(encoded["error"]["code"], "CHILD_EXITED_BEFORE_REGISTRATION");
        assert_eq!(
            encoded["error"]["message"],
            "Worker exited before registration"
        );
        assert!(encoded.get("workers").is_none());
        let encoded = encoded.to_string();
        for internal in ["operation_id", "containers", "queue", "retry secret output"] {
            assert!(!encoded.contains(internal), "leaked {internal}: {encoded}");
        }
    }

    #[test]
    fn failed_outcome_prefers_primary_error_over_earlier_container_error() {
        let primary_error = OpError {
            code: "CHILD_EXITED_BEFORE_REGISTRATION".into(),
            message: "container 'api' exited with 9 before it registered".into(),
        };

        let result = OpResult {
            operation_id: "mixed-failure".into(),
            status: OpStatus::Failed,
            changed: false,
            containers: vec![
                ContainerResult {
                    container: "mailer".into(),
                    state: ChildStatus::Failed,
                    changed: false,
                    error: Some(OpError {
                        code: "STARTUP_TIMEOUT".into(),
                        message: "container 'mailer' was not ready after 2s".into(),
                    }),
                },
                ContainerResult {
                    container: "api".into(),
                    state: ChildStatus::Failed,
                    changed: false,
                    error: Some(primary_error.clone()),
                },
            ],
            primary_error: Some(primary_error),
        };

        let outcome = MutationOutcome::from_operations(
            OpStatus::Failed,
            false,
            None,
            None,
            None,
            std::iter::once(&result),
        );
        let encoded = serde_json::to_value(&outcome).unwrap();
        assert_eq!(
            encoded["error"]["code"], "CHILD_EXITED_BEFORE_REGISTRATION",
            "{encoded}"
        );
    }
}

#[cfg(test)]
mod declared_dependency_tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::keep_declared_dependencies;
    use crate::edit::{NewContainer, Source};

    fn resolved(name: &str, version: &str) -> NewContainer {
        NewContainer {
            key: name.to_string(),
            source: Source::Package {
                reference: format!("api.workers.iii.dev/{name}"),
                version: Some(version.to_string()),
            },
            start_after: Vec::new(),
            fields: serde_yaml::Mapping::new(),
        }
    }

    #[test]
    fn an_add_leaves_already_declared_dependencies_alone() {
        let declared = crate::ComposeFile::parse(
            "namespace: default\ncontainers:\n  state:\n    worker: package://state\n    version: \"0.22.8\"\n  http:\n    worker: package://http\n    version: \"0.21.9\"\n",
            "/tmp/worker-compose.yaml",
        )
        .unwrap();
        let asked: BTreeSet<String> = ["provider-openai-codex".to_string()].into();
        let wanted = vec![
            resolved("state", "0.22.9"),
            resolved("llm-router", "1.4.19"),
            resolved("provider-openai-codex", "0.4.9"),
        ];
        let mut selected_versions = BTreeMap::from([
            ("state".to_string(), "0.22.9".to_string()),
            ("llm-router".to_string(), "1.4.19".to_string()),
            ("provider-openai-codex".to_string(), "0.4.9".to_string()),
        ]);

        let kept = keep_declared_dependencies(wanted, &asked, &declared, &mut selected_versions);
        let keys: Vec<&str> = kept.iter().map(|worker| worker.key.as_str()).collect();

        assert_eq!(keys, vec!["llm-router", "provider-openai-codex"]);
        assert_eq!(
            selected_versions
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["llm-router", "provider-openai-codex"],
            "the lock must not be forced to the dropped dependency version"
        );
    }

    #[test]
    fn an_explicitly_asked_worker_is_always_written_even_when_declared() {
        let declared = crate::ComposeFile::parse(
            "namespace: default\ncontainers:\n  state:\n    worker: package://state\n    version: \"0.22.8\"\n",
            "/tmp/worker-compose.yaml",
        )
        .unwrap();
        let asked: BTreeSet<String> = ["state".to_string()].into();
        let mut selected_versions = BTreeMap::from([("state".to_string(), "0.22.9".to_string())]);

        let kept = keep_declared_dependencies(
            vec![resolved("state", "0.22.9")],
            &asked,
            &declared,
            &mut selected_versions,
        );

        assert_eq!(
            kept.len(),
            1,
            "compose::add worker=state is the operator moving the pin on purpose"
        );
        assert_eq!(
            selected_versions.get("state").map(String::as_str),
            Some("0.22.9")
        );
    }
}

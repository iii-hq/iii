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
    collections::BTreeMap,
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

/// Keeps one declaration for a dependency shared by several requested workers.
///
/// Registry graphs are resolved per requested root. Two roots may therefore
/// return the same container. Identical declarations are one shared
/// dependency; different declarations mean the roots resolved incompatible
/// versions, sources, or dependency edges. Reject that batch before the file
/// is read or edited, so argument order cannot select the winning declaration.
fn coalesce_expanded(
    expanded: Vec<crate::edit::NewContainer>,
) -> Result<Vec<crate::edit::NewContainer>> {
    let mut positions = BTreeMap::new();
    let mut unique: Vec<crate::edit::NewContainer> = Vec::with_capacity(expanded.len());

    for container in expanded {
        if let Some(&position) = positions.get(&container.key) {
            if unique[position] != container {
                return Err(ComposeError::InvalidWorkerSpec {
                    spec: container.key.clone(),
                    reason: format!(
                        "the requested workers resolve container '{}' to conflicting sources, \
                         versions, or dependencies",
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
            let compose = ComposeFile::load(file)?;
            self.engine_policy.validate_project(&compose)?;
            // Validate before announcing: a project that cannot start is better
            // refused here than half-started later.
            let namespace = self.project_namespace(&compose);
            crate::manifest::validate_offline(&compose, &namespace)?;

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
        let current = ComposeFile::load(file)?;
        self.engine_policy.validate_project(&current)?;
        let project = self.project(file).await?;
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
    ) -> Result<Option<OpResult>> {
        let file = self.resolve_file(file)?;
        let current = ComposeFile::load(file)?;
        self.engine_policy.validate_project(&current)?;
        let project = self.project(file).await?;
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
    /// [`crate::edit`]. A version the caller did not pin is resolved once and
    /// written out, because `compose::add` promising "the latest" and a later
    /// `up` silently getting a different one is the drift a compose file exists
    /// to prevent.
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
            .map(|worker| crate::edit::parse_worker(worker))
            .collect::<Result<Vec<_>>>()?;
        // A worker is not useful alone: its manifest names what it calls, and
        // the registry answers with that whole graph already pinned to versions
        // that satisfy each other. They are declared rather than started
        // behind the file, so what runs is still what the file says.
        //
        // Dependencies first and the worker last: with no `start_after`, start
        // order is declaration order, so this is what makes a worker start
        // after the things it calls.
        let mut expanded = futures::stream::iter(asked.clone().into_iter().enumerate().map(
            |(index, worker)| async move {
                let graph = self.expand(&worker).await;
                (index, graph)
            },
        ))
        .buffer_unordered(4)
        .collect::<Vec<_>>()
        .await;
        expanded.sort_by_key(|(index, _)| *index);
        let mut wanted = Vec::new();
        for (_, graph) in expanded {
            wanted.extend(graph?);
        }
        let wanted = coalesce_expanded(wanted)?;

        // Acquire registry artifacts before taking either the file mutation lock
        // or the project's runtime lock. `lifecycle::start_one` calls install
        // again, but that second call is a cheap verified cache hit.
        let package_cache = crate::state::StateStore::package_cache()?;
        let operation = crate::operation::active(&operation_id);
        let installs: Vec<(String, String, String)> = wanted
            .iter()
            .filter_map(|worker| match &worker.source {
                crate::edit::Source::Package {
                    reference,
                    version: Some(version),
                } => Some((worker.key.clone(), reference.clone(), version.clone())),
                _ => None,
            })
            .collect();
        let acquired: Vec<Result<()>> =
            futures::stream::iter(installs.into_iter().map(|(key, reference, version)| {
                let package_cache = package_cache.clone();
                let operation = operation.clone();
                async move {
                    if let Some(operation) = operation {
                        operation
                            .emit(
                                Some(&key),
                                "installing",
                                format!("acquiring {reference}@{version}"),
                            )
                            .await;
                    }
                    crate::registry::install(&key, &reference, &version, &package_cache)
                        .await
                        .map(|_| ())
                }
            }))
            .buffer_unordered(4)
            .collect()
            .await;
        for result in acquired {
            result?;
        }

        if operation
            .as_ref()
            .is_some_and(|operation| operation.is_cancelled())
        {
            return Err(ComposeError::OperationCancelled { operation_id });
        }

        let _mutation = self.lock_mutation(path).await;
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
        self.validate_engine_policy_text(path, &text)?;
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

        if added.is_empty() && replaced.is_empty() {
            return Ok(MutationOutcome::from_operations(
                OpStatus::Ok,
                false,
                Some(&container),
                Some(&requested),
                wanted
                    .iter()
                    .find(|worker| worker.key == container)
                    .and_then(|worker| match &worker.source {
                        crate::edit::Source::Package { version, .. } => version.clone(),
                        crate::edit::Source::Path { .. } => None,
                    }),
                std::iter::empty::<&OpResult>(),
            ));
        }
        let edited = &edited;

        // Parsed before it is written, so a splice that would not load leaves
        // the operator's file exactly as it was.
        crate::ComposeFile::parse(edited, path)?;
        write_atomically(path, edited)?;

        let current = ComposeFile::load(path)?;
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
        let version = wanted
            .iter()
            .find(|worker| worker.key == container)
            .and_then(|worker| match &worker.source {
                crate::edit::Source::Package { version, .. } => version.clone(),
                crate::edit::Source::Path { .. } => None,
            });
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

    /// The worker asked for, plus everything it needs, in start order.
    ///
    /// A `path://` worker is taken alone: its dependencies are declared in a
    /// manifest on disk, and resolving those means asking the registry per name
    /// rather than reading one answer. That is worth doing, and is not done
    /// here yet.
    ///
    /// `engine` workers are skipped. They are compiled into the engine and are
    /// already serving before compose starts anything; declaring one would
    /// produce a container with no artefact to install.
    async fn expand(
        &self,
        asked: &crate::edit::NewContainer,
    ) -> Result<Vec<crate::edit::NewContainer>> {
        let crate::edit::Source::Package { reference, version } = &asked.source else {
            return Ok(vec![asked.clone()]);
        };

        let range = version.clone().unwrap_or_else(|| "*".to_string());
        let graph = crate::registry::resolve_graph(&asked.key, reference, &range).await?;
        expand_graph(asked, reference, graph)
    }

    /// Moves one declared container to another version of the same package.
    ///
    /// `worker=state` takes whatever the registry calls latest; `worker=state@1.2.3`
    /// takes that one, which is how a downgrade is spelled. The container has to
    /// be declared already — this edits a line, it does not add one, and
    /// `compose::add` is the call that adds.
    ///
    /// The answer names both versions, because the operator asked for "latest"
    /// without knowing what that is and the interesting part of the reply is
    /// what it turned out to be.
    pub async fn update(
        &self,
        file: Option<&Path>,
        worker: Option<&str>,
        operation_id: String,
    ) -> Result<MutationOutcome> {
        let Some(worker) = worker else {
            return Err(ComposeError::InvalidWorkerSpec {
                spec: String::new(),
                reason: "no worker was named. Pass worker=<name> or worker=<name@version>"
                    .to_string(),
            });
        };

        let path = self.resolve_file(file)?;
        let asked = crate::edit::parse_worker(worker)?;
        let crate::edit::Source::Package { reference, version } = &asked.source else {
            return Err(ComposeError::NotAPackageContainer {
                container: asked.key.clone(),
                kind: "path".to_string(),
            });
        };

        let _mutation = self.lock_mutation(path).await;
        let text = std::fs::read_to_string(path).map_err(|source| ComposeError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let compose = crate::ComposeFile::parse(&text, path)?;
        self.engine_policy.validate_project(&compose)?;
        let Some(container) = compose.containers.get(&asked.key) else {
            return Err(ComposeError::UnknownContainer {
                container: asked.key.clone(),
            });
        };
        if !matches!(
            container.worker,
            crate::config::WorkerSource::Package { .. }
        ) {
            return Err(ComposeError::NotAPackageContainer {
                container: asked.key.clone(),
                kind: "path".to_string(),
            });
        }

        // Asked for by version, or whatever the registry calls latest today.
        let wanted = match version {
            Some(version) => version.clone(),
            None => crate::registry::latest_version(&asked.key, reference).await?,
        };

        // The declared dependencies come along unchanged. An update moves a
        // version; rewriting the graph on the way is `compose::add`'s job, and
        // doing it here would edit lines the operator did not ask about.
        let new = crate::edit::NewContainer {
            key: asked.key.clone(),
            source: crate::edit::Source::Package {
                reference: reference.clone(),
                version: Some(wanted.clone()),
            },
            start_after: container.start_after.clone(),
        };

        let edited = match crate::edit::upsert_container(&text, &new)? {
            crate::edit::Outcome::Unchanged => {
                return Ok(MutationOutcome::from_operations(
                    OpStatus::Ok,
                    false,
                    Some(&asked.key),
                    None,
                    Some(wanted),
                    std::iter::empty::<&OpResult>(),
                ));
            }
            crate::edit::Outcome::Replaced { text, .. } => text,
            // `upsert_container` only adds when the key is absent, and the key
            // was read out of this same file a moment ago.
            crate::edit::Outcome::Added(text) => text,
        };

        // Parsed before it is written, so a splice that would not load leaves
        // the operator's file exactly as it was.
        crate::ComposeFile::parse(&edited, path)?;
        write_atomically(path, &edited)?;

        // The whole project, not just this container, and deliberately so. A
        // cached project is the file as it was read, so the new version is only
        // picked up once the project is dropped and re-read — and dropping it
        // while its other children run would leave them supervised by nothing.
        // `compose::restart worker=` is the surgical one; this is the safe one.
        let (down, up) = self.restart_project(file, None, &operation_id).await?;
        Ok(MutationOutcome::from_operations(
            up.status,
            true,
            Some(&asked.key),
            None,
            Some(wanted),
            [&down, &up].into_iter(),
        ))
    }

    /// Removes one declared worker and reconciles the running project.
    ///
    /// Dependency edges pointing at the removed worker are deleted with it.
    /// The edited declaration is fully validated before the file or any
    /// process changes. Only the removed container stops; normal idempotent
    /// `up` then starts anything else that was already missing.
    pub async fn remove(
        &self,
        file: Option<&Path>,
        worker: Option<&str>,
        operation_id: String,
    ) -> Result<MutationOutcome> {
        let Some(worker) = worker.map(str::trim).filter(|worker| !worker.is_empty()) else {
            return Err(ComposeError::InvalidWorkerSpec {
                spec: String::new(),
                reason: "no worker was named. Pass worker=<name>".to_string(),
            });
        };

        let path = self.resolve_file(file)?;
        let _mutation = self.lock_mutation(path).await;
        let text = std::fs::read_to_string(path).map_err(|source| ComposeError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        self.validate_engine_policy_text(path, &text)?;
        let Some(edited) = crate::edit::remove_container(&text, worker)? else {
            return Err(ComposeError::UnknownContainer {
                container: worker.to_string(),
            });
        };

        let current = crate::ComposeFile::parse(&edited, path)?;
        self.engine_policy.validate_project(&current)?;
        let namespace = self.project_namespace(&current);
        crate::manifest::validate_offline(&current, &namespace)?;

        // Claim or load the old project before replacing the file: cleanup of
        // the removed container needs its old scripts and environment.
        let project = self.project(path).await?;
        write_atomically(path, &edited)?;

        let (down, up) = project
            .reconcile_removal(current, worker, operation_id)
            .await;
        Ok(MutationOutcome::from_operations(
            up.status,
            true,
            Some(worker),
            None,
            None,
            [&down, &up].into_iter(),
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
            let project = self.project(path).await?;
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

        let (down, up) = self.restart_project(file, None, &operation_id).await?;
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
    async fn restart_project(
        &self,
        file: Option<&Path>,
        container: Option<&str>,
        operation_id: &str,
    ) -> Result<(OpResult, OpResult)> {
        let path = self.resolve_file(file)?.to_path_buf();
        let down = self
            .down(file, container, format!("{operation_id}-down"))
            .await?;
        self.forget(&path).await;
        let up = self
            .up(file, container, format!("{operation_id}-up"))
            .await?;
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
        let mut error = None;
        let mut failed = Vec::new();

        for result in operations.flat_map(|operation| &operation.containers) {
            if result.changed && !requested.contains(result.container.as_str()) {
                affected_workers.insert(result.container.clone());
            }
            if result.error.is_some() {
                failed.push(result.container.clone());
            }
            if error.is_none() {
                error = result.error.as_ref().map(MutationError::from);
            }
        }

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

/// Turns one registry answer into the declarations Compose can own.
///
/// Engine-kind dependencies are omitted because the engine already provides
/// them. An engine-kind root is different: silently omitting the exact worker
/// the caller requested would make `compose::add` report success without
/// changing the project, so reject it with migration guidance instead.
fn expand_graph(
    asked: &crate::edit::NewContainer,
    reference: &str,
    graph: crate::registry::Graph,
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

    let mut declarable: std::collections::BTreeSet<String> = graph
        .nodes
        .iter()
        .filter(|node| node.kind != "engine")
        .map(|node| node.name.clone())
        .collect();
    declarable.insert(asked.key.clone());

    let needs = |name: &str| -> Vec<String> {
        let mut needed: Vec<String> = graph
            .edges
            .iter()
            .filter(|(from, to)| from == name && to != name && declarable.contains(to))
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
    };

    // Registry nodes are a set, not an ordered plan. Derive a deterministic
    // dependency-first order from the edges, preferring the requested root
    // only after other ready nodes so independent registry entries cannot put
    // it before a dependency that becomes ready in the same wave.
    let mut pending: BTreeMap<String, usize> = BTreeMap::new();
    let mut dependents: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for name in &declarable {
        let dependencies = needs(name);
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

    let nodes: BTreeMap<&str, &crate::registry::Node> = graph
        .nodes
        .iter()
        .filter(|node| node.kind != "engine")
        .map(|node| (node.name.as_str(), node))
        .collect();
    order
        .into_iter()
        .map(|name| {
            if name == asked.key {
                let mut root = asked.clone();
                root.start_after = needs(&asked.key);
                if let Some(node) = nodes.get(asked.key.as_str()) {
                    root.source = crate::edit::Source::Package {
                        reference: reference.to_string(),
                        version: Some(node.version.clone()),
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

    fn package(name: &str) -> crate::edit::NewContainer {
        crate::edit::NewContainer {
            key: name.to_string(),
            source: crate::edit::Source::Package {
                reference: name.to_string(),
                version: None,
            },
            start_after: Vec::new(),
        }
    }

    #[test]
    fn configurable_engine_root_points_to_engine_workers() {
        let graph = crate::registry::Graph {
            nodes: vec![crate::registry::Node {
                name: "configuration".to_string(),
                version: "0.23.0".to_string(),
                kind: "engine".to_string(),
            }],
            edges: Vec::new(),
        };

        let error = expand_graph(&package("configuration"), "configuration", graph)
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
            }],
            edges: Vec::new(),
        };

        let error = expand_graph(
            &package("iii-engine-functions"),
            "iii-engine-functions",
            graph,
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
                },
                crate::registry::Node {
                    name: "configuration".to_string(),
                    version: "0.23.0".to_string(),
                    kind: "engine".to_string(),
                },
                crate::registry::Node {
                    name: "state".to_string(),
                    version: "2.0.0".to_string(),
                    kind: "binary".to_string(),
                },
            ],
            edges: vec![
                ("api".to_string(), "configuration".to_string()),
                ("api".to_string(), "state".to_string()),
            ],
        };

        let expanded = expand_graph(&package("api"), "api", graph).expect("expand graph");
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
                },
                crate::registry::Node {
                    name: "db".to_string(),
                    version: "3.0.0".to_string(),
                    kind: "binary".to_string(),
                },
                crate::registry::Node {
                    name: "api".to_string(),
                    version: "1.0.0".to_string(),
                    kind: "binary".to_string(),
                },
            ],
            edges: vec![
                ("api".to_string(), "state".to_string()),
                ("state".to_string(), "db".to_string()),
            ],
        };

        let expanded = expand_graph(&package("api"), "api", graph).expect("expand graph");
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
                },
                crate::registry::Node {
                    name: "state".to_string(),
                    version: "2.0.0".to_string(),
                    kind: "binary".to_string(),
                },
            ],
            edges: vec![
                ("api".to_string(), "state".to_string()),
                ("state".to_string(), "api".to_string()),
            ],
        };

        let error = expand_graph(&package("api"), "api", graph)
            .expect_err("registry dependency cycles must be rejected");
        assert_eq!(error.code(), "DEPENDENCY_CYCLE");
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

        let merged = coalesce_expanded(vec![state.clone(), queue.clone(), state.clone()]).unwrap();

        assert_eq!(merged, vec![state, queue]);
    }

    #[test]
    fn conflicting_shared_dependencies_are_rejected_before_editing() {
        let first = crate::edit::parse_worker("state@1.0.0").unwrap();
        let second = crate::edit::parse_worker("state@2.0.0").unwrap();

        for expanded in [vec![first.clone(), second.clone()], vec![second, first]] {
            let error = coalesce_expanded(expanded)
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
                    error: Some(OpError {
                        code: "CHILD_EXITED_BEFORE_REGISTRATION".into(),
                        message: "container 'tailscale' exited with 1 before it registered. It last said:\nretry secret output".into(),
                    }),
                },
            ],
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
}

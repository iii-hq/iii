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

use serde_json::Value;
use tokio::sync::{Mutex, OnceCell};

use crate::{
    config::{ComposeFile, EngineSpec},
    engine::EngineClient,
    error::{ComposeError, Result},
    lifecycle::OpResult,
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
            engine,
            engine_policy,
            projects: Mutex::new(BTreeMap::new()),
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
        let compose = ComposeFile::load(file)?;
        self.project_from(compose).await
    }

    async fn project_from(&self, compose: ComposeFile) -> Result<Arc<Project>> {
        let key = compose.identity.clone();

        // The map lock is held only long enough to claim the cell. Loading under
        // it would make one slow project block `compose::list` and every other
        // project on the daemon.
        let cell = {
            let mut projects = self.projects.lock().await;
            Arc::clone(projects.entry(key.clone()).or_default())
        };

        cell.get_or_try_init(|| async {
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
        self.up_stack(file, None, container, operation_id).await
    }

    pub async fn up_stack(
        &self,
        file: Option<&Path>,
        stack: Option<&str>,
        container: Option<&str>,
        operation_id: String,
    ) -> Result<OpResult> {
        let file = self.resolve_file(file)?;
        let current = ComposeFile::load_stack(file, stack)?;
        self.engine_policy.validate_project(&current)?;
        let project = self.project_from(current).await?;
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
    ) -> Result<Value> {
        let path = self.resolve_file(file)?;
        self.validate_engine_policy_file(path)?;
        if let Some(key) = container {
            let project = self.project(path).await?;
            let result = project.restart_one(key, operation_id).await;
            return Ok(serde_json::json!({
                "status": result.status,
                "container": key,
                "changed": result.changed,
                "restarted": serde_json::to_value(&result).unwrap_or(Value::Null),
            }));
        }

        let (down, up) = self.restart_project(file, None, &operation_id).await?;
        Ok(serde_json::json!({
            "status": up.status,
            "changed": down.changed || up.changed,
            "down": serde_json::to_value(&down).unwrap_or(Value::Null),
            "up": serde_json::to_value(&up).unwrap_or(Value::Null),
        }))
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

    /// Takes a project down.
    pub async fn down(
        &self,
        file: Option<&Path>,
        container: Option<&str>,
        operation_id: String,
    ) -> Result<OpResult> {
        self.down_stack(file, None, container, operation_id).await
    }

    pub async fn down_stack(
        &self,
        file: Option<&Path>,
        stack: Option<&str>,
        container: Option<&str>,
        operation_id: String,
    ) -> Result<OpResult> {
        let path = self.resolve_file(file)?;
        self.validate_engine_policy_file(path)?;
        let current = ComposeFile::load_stack(path, stack)?;
        let project = self.project_from(current).await?;
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
        self.validate_stack(file, None).await
    }

    pub async fn validate_stack(
        &self,
        file: Option<&Path>,
        stack: Option<&str>,
    ) -> Result<crate::manifest::ValidationReport> {
        let file = self.resolve_file(file)?;
        if file.is_relative() && !file.exists() {
            return Err(ComposeError::RelativeFileMissing {
                path: file.to_path_buf(),
                cwd: std::env::current_dir().unwrap_or_default(),
            });
        }
        let compose = ComposeFile::load_stack(file, stack)?;
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

/// What every compose daemon registers as.
///
/// Fixed, and the exclusion depends on it: `(default, compose)` is a lease the
/// engine hands to one connection, so a second daemon is refused at
/// registration rather than left running unreachable.
pub const DAEMON_WORKER_NAME: &str = "compose";

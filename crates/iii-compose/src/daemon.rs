// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! The daemon: one engine connection, many projects.
//!
//! Two addresses, and they answer different questions. `--ns` is the daemon —
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

use tokio::sync::Mutex;

use crate::{
    config::ComposeFile,
    engine::EngineClient,
    error::{ComposeError, Result},
    lifecycle::OpResult,
    project::Project,
};

/// How often the supervisor checks whether a ready child is still alive.
/// Fast enough that a crash is reported while the operator is still watching,
/// slow enough that an idle daemon costs nothing.
const SUPERVISION_INTERVAL: Duration = Duration::from_millis(250);

pub struct Daemon {
    /// What this daemon registered as. Fixed, and the same on every machine:
    /// what tells two daemons apart is the namespace, not the name.
    pub worker_name: String,
    /// This machine's identity — the `--id`, and the namespace it answers
    /// `compose::*` in. `id=` on a call is checked against it.
    pub daemon_namespace: String,
    pub engine_url: String,
    engine: Arc<EngineClient>,
    /// Every project this daemon has been asked about, keyed by the canonical
    /// path of its compose file. Nothing else identifies a project: a name
    /// someone chose would be a second identity, and a second identity can be
    /// pointed at the wrong file.
    projects: Mutex<BTreeMap<PathBuf, Arc<Project>>>,
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
    pub fn start(engine_url: String, daemon_namespace: String) -> Arc<Self> {
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
            engine_url,
            engine,
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

        if let Some(existing) = self.projects.lock().await.get(&key).cloned() {
            return Ok(existing);
        }

        let compose = ComposeFile::load(file)?;
        // Validate before announcing: a project that cannot start is better
        // refused here than half-started later.
        let namespace = crate::namespace::project_namespace(None, compose.namespace.as_deref());
        crate::manifest::validate_offline(&compose, &namespace)?;

        let project = Project::open(
            &self.daemon_namespace,
            compose,
            Arc::clone(&self.engine),
            self.engine_url.clone(),
        )
        .await?;

        self.projects
            .lock()
            .await
            .insert(key.clone(), Arc::clone(&project));
        crate::report::daemon_line(
            &format!("project {} loaded into {namespace}", key.display()),
            false,
        );
        Ok(project)
    }

    /// Every project this daemon knows, for `compose::list`.
    pub async fn list(&self) -> Vec<serde_json::Value> {
        let projects: Vec<Arc<Project>> = self.projects.lock().await.values().cloned().collect();
        let mut listed = Vec::new();
        for project in projects {
            listed.push(serde_json::json!({
                "namespace": project.project_namespace,
                "file": project.file.path,
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
        let project = self.project(self.resolve_file(file)?).await?;
        Ok(project.up(container, operation_id).await)
    }

    /// Takes a project down.
    pub async fn down(
        &self,
        file: Option<&Path>,
        container: Option<&str>,
        operation_id: String,
    ) -> Result<OpResult> {
        let project = self.project(self.resolve_file(file)?).await?;
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
        let namespace = crate::namespace::project_namespace(None, compose.namespace.as_deref());
        crate::manifest::validate_offline(&compose, &namespace)
    }

    pub async fn status(&self, file: Option<&Path>) -> Result<Arc<Project>> {
        self.project(self.resolve_file(file)?).await
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
        let projects: Vec<Arc<Project>> = self.projects.lock().await.values().cloned().collect();
        for project in projects {
            project.shutdown().await;
        }
        self.engine.shutdown().await;
    }

    /// Leaves without touching what was not started here. Used when the engine
    /// refuses this daemon's registration.
    pub async fn abandon(&self) {
        let projects: Vec<Arc<Project>> = self.projects.lock().await.values().cloned().collect();
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

                let projects: Vec<Arc<Project>> =
                    daemon.projects.lock().await.values().cloned().collect();
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

// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! The daemon: one engine connection, many projects.
//!
//! `compose::*` is registered once, in `default`, and every call names the
//! project it means with `id=`. That is the whole addressing story: an operator
//! types an id they chose, not a namespace they have to look up, and the
//! namespace goes back to being what the engine uses to route workers.
//!
//! One daemon serves an engine, and its worker name says so: `compose`, fixed.
//!
//! A random name would let a second daemon register, but not be reached — both
//! would own `compose::up` in `default` and the engine routes a call to one of
//! them. The second would sit there holding projects nobody can address, which
//! is worse than being refused. The `(default, compose)` lease is the only
//! race-free way to say "this engine already has one", so the collision is the
//! feature: the second daemon is told, immediately and by name.

use std::{collections::BTreeMap, path::Path, sync::Arc, time::Duration};

use tokio::sync::Mutex;

use crate::{
    config::ComposeFile,
    engine::EngineClient,
    error::{ComposeError, Result},
    lifecycle::OpResult,
    project::{ContainerStatus, Project},
};

/// How often the supervisor checks whether a ready child is still alive.
/// Fast enough that a crash is reported while the operator is still watching,
/// slow enough that an idle daemon costs nothing.
const SUPERVISION_INTERVAL: Duration = Duration::from_millis(250);

pub struct Daemon {
    /// What this daemon registered as. Random, and of no interest to the
    /// operator: projects are addressed by `id=`, never by this.
    pub worker_name: String,
    pub engine_url: String,
    engine: Arc<EngineClient>,
    /// Every project this daemon has been asked about, by `id`.
    projects: Mutex<BTreeMap<String, Arc<Project>>>,
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
    pub fn start(engine_url: String) -> Arc<Self> {
        let engine = Arc::new(EngineClient::connect(
            &engine_url,
            DAEMON_WORKER_NAME,
            crate::namespace::DEFAULT_NAMESPACE,
        ));

        let daemon = Arc::new(Self {
            worker_name: DAEMON_WORKER_NAME.to_string(),
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

    /// The project `id` names, loading `file` if this is the first time.
    ///
    /// `file` is required to reach a project the daemon has not seen; after
    /// that it is optional, and giving a different one is an error rather than
    /// a silent rebind — the id already owns durable state pointing at a
    /// compose file, and pointing it elsewhere would adopt children it never
    /// started.
    pub async fn project(&self, id: &str, file: Option<&Path>) -> Result<Arc<Project>> {
        if let Some(existing) = self.projects.lock().await.get(id).cloned() {
            if let Some(file) = file {
                let requested = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
                if requested != existing.file.path {
                    return Err(ComposeError::StateBindingMismatch {
                        daemon_id: id.to_string(),
                        recorded: existing.file.path.clone(),
                        requested,
                    });
                }
            }
            return Ok(existing);
        }

        let Some(file) = file else {
            return Err(ComposeError::UnknownProject { id: id.to_string() });
        };

        if file.is_relative() && !file.exists() {
            return Err(ComposeError::RelativeFileMissing {
                path: file.to_path_buf(),
                cwd: std::env::current_dir().unwrap_or_default(),
            });
        }

        let compose = ComposeFile::load(file)?;
        // Validate before announcing: a project that cannot start is better
        // refused here than half-started later.
        let namespace = crate::namespace::project_namespace(None, compose.name.as_deref());
        crate::manifest::validate_offline(&compose, &namespace)?;

        let project = Project::open(
            id.to_string(),
            compose,
            Arc::clone(&self.engine),
            self.engine_url.clone(),
        )
        .await?;

        self.projects
            .lock()
            .await
            .insert(id.to_string(), Arc::clone(&project));
        crate::report::daemon_line(&format!("project '{id}' loaded into {namespace}"), false);
        Ok(project)
    }

    /// Every project this daemon knows, for `compose::list`.
    pub async fn list(&self) -> Vec<serde_json::Value> {
        let projects: Vec<Arc<Project>> = self.projects.lock().await.values().cloned().collect();
        let mut listed = Vec::new();
        for project in projects {
            listed.push(serde_json::json!({
                "id": project.id,
                "namespace": project.project_namespace,
                "file": project.file.path,
                "containers": project.status().await,
            }));
        }
        listed
    }

    /// Brings `id` up, loading `file` if the project is new.
    pub async fn up(
        &self,
        id: &str,
        file: Option<&Path>,
        container: Option<&str>,
        operation_id: String,
    ) -> Result<OpResult> {
        // Only `up` falls back to the compose file in the daemon's own
        // directory, and only for a project it has not seen: starting compose
        // inside a project and saying `up id=a` should be enough. `down` and
        // `status` keep erroring on an unknown id, because there the fallback
        // would turn a mistyped id into a new project instead of a question.
        let default_file = Path::new(crate::cli::DEFAULT_COMPOSE_FILE);
        let known = self.projects.lock().await.contains_key(id);
        let file = match file {
            Some(file) => Some(file),
            None if !known && default_file.exists() => Some(default_file),
            None => None,
        };

        let project = self.project(id, file).await?;
        Ok(project.up(container, operation_id).await)
    }

    /// Takes `id` down. The file is not needed: a project being stopped is one
    /// the daemon already knows.
    pub async fn down(
        &self,
        id: &str,
        container: Option<&str>,
        operation_id: String,
    ) -> Result<OpResult> {
        let project = self.project(id, None).await?;
        Ok(project.down(container, operation_id).await)
    }

    /// Checks a declaration without taking it on.
    ///
    /// With a `file`, the file is read and answered for and nothing is kept —
    /// this is the call a CI job makes, and it must not leave the daemon
    /// holding a project or bind an id to a path. With only an `id`, the
    /// question is about a project already held.
    pub async fn validate(
        &self,
        id: Option<&str>,
        file: Option<&Path>,
    ) -> Result<crate::manifest::ValidationReport> {
        if let Some(file) = file {
            if file.is_relative() && !file.exists() {
                return Err(ComposeError::RelativeFileMissing {
                    path: file.to_path_buf(),
                    cwd: std::env::current_dir().unwrap_or_default(),
                });
            }
            let compose = ComposeFile::load(file)?;
            let namespace = crate::namespace::project_namespace(None, compose.name.as_deref());
            return crate::manifest::validate_offline(&compose, &namespace);
        }

        let id = id.ok_or_else(|| ComposeError::UnknownProject {
            id: "<none>".to_string(),
        })?;
        let project = self.project(id, None).await?;
        crate::manifest::validate_offline(&project.file, &project.project_namespace)
    }

    pub async fn status(&self, id: &str) -> Result<Vec<ContainerStatus>> {
        Ok(self.project(id, None).await?.status().await)
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
            "stopping": projects.keys().collect::<Vec<_>>(),
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

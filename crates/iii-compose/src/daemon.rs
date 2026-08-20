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

        // The map lock is held only long enough to claim the cell. Loading under
        // it would make one slow project block `compose::list` and every other
        // project on the daemon.
        let cell = {
            let mut projects = self.projects.lock().await;
            Arc::clone(projects.entry(key.clone()).or_default())
        };

        cell.get_or_try_init(|| async {
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

    /// Adds a container to a project's file, then restarts the project.
    ///
    /// The file is the operator's, so it is edited rather than rewritten: see
    /// [`crate::edit`]. A version the caller did not pin is resolved once and
    /// written out, because `compose::add` promising "the latest" and a later
    /// `up` silently getting a different one is the drift a compose file exists
    /// to prevent.
    ///
    /// Restart is `down` then `up`, and deliberately unclever for now. A failed
    /// `up` leaves the file as edited and reports the failure: the edit is what
    /// was asked for, and undoing it would hide the reason the project will not
    /// start.
    pub async fn add(
        &self,
        file: Option<&Path>,
        worker: Option<&str>,
        operation_id: String,
    ) -> Result<Value> {
        let Some(worker) = worker else {
            return Err(ComposeError::InvalidWorkerSpec {
                spec: String::new(),
                reason: "no worker was named. Pass worker=<name|name@version|./path>".to_string(),
            });
        };

        let path = self.resolve_file(file)?;
        let asked = crate::edit::parse_worker(worker)?;
        // A worker is not useful alone: its manifest names what it calls, and
        // the registry answers with that whole graph already pinned to versions
        // that satisfy each other. They are declared rather than started
        // behind the file, so what runs is still what the file says.
        //
        // Dependencies first and the worker last: with no `depends_on`, start
        // order is declaration order, so this is what makes a worker start
        // after the things it calls.
        let wanted = self.expand(&asked).await?;

        let text = std::fs::read_to_string(path).map_err(|source| ComposeError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let mut edited = text.clone();
        let mut added: Vec<String> = Vec::new();
        let mut replaced: Vec<String> = Vec::new();
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
                }
            }
        }

        if added.is_empty() && replaced.is_empty() {
            return Ok(serde_json::json!({
                "status": "ok",
                "container": asked.key,
                "changed": false,
                "detail": "already declared at this version",
            }));
        }
        let action = match (added.is_empty(), replaced.is_empty()) {
            (false, true) => format!("added {}", added.join(", ")),
            (true, false) => format!("replaced {}", replaced.join(", ")),
            _ => format!(
                "added {}; replaced {}",
                added.join(", "),
                replaced.join(", ")
            ),
        };
        let edited = &edited;

        // Parsed before it is written, so a splice that would not load leaves
        // the operator's file exactly as it was.
        crate::ComposeFile::parse(edited, path)?;
        write_atomically(path, edited)?;

        let (down, up) = self.restart_project(file, None, &operation_id).await?;
        Ok(serde_json::json!({
            "status": up.status,
            "container": asked.key,
            "changed": true,
            "declared": added,
            "detail": action,
            "down": serde_json::to_value(&down).unwrap_or(Value::Null),
            "up": serde_json::to_value(&up).unwrap_or(Value::Null),
        }))
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
        let host = reference
            .rsplit_once('/')
            .map(|(host, _)| host)
            .unwrap_or("");

        // Only what compose can run becomes a container, and only those can be
        // depended on: an `engine` worker is already serving before compose
        // starts, so an edge to one names nothing this file declares.
        let declarable: std::collections::BTreeSet<&str> = graph
            .nodes
            .iter()
            .filter(|node| node.kind != "engine")
            .map(|node| node.name.as_str())
            .collect();

        let needs = |name: &str| -> Vec<String> {
            let mut needed: Vec<String> = graph
                .edges
                .iter()
                .filter(|(from, to)| from == name && to != name && declarable.contains(to.as_str()))
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
            depends_on: needs(&node.name),
        };

        let mut wanted: Vec<crate::edit::NewContainer> = graph
            .nodes
            .iter()
            .filter(|node| node.kind != "engine")
            .filter(|node| node.name != asked.key)
            .map(container)
            .collect();

        // The worker itself last, pinned to what the graph resolved and
        // depending on everything it calls.
        let mut root = asked.clone();
        root.depends_on = needs(&asked.key);
        if let Some(node) = graph.nodes.iter().find(|node| node.name == asked.key) {
            root.source = crate::edit::Source::Package {
                reference: reference.clone(),
                version: Some(node.version.clone()),
            };
        }
        wanted.push(root);
        Ok(wanted)
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
    ) -> Result<Value> {
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

        let text = std::fs::read_to_string(path).map_err(|source| ComposeError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let compose = crate::ComposeFile::parse(&text, path)?;
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
        let current = container.version.clone();

        // The declared dependencies come along unchanged. An update moves a
        // version; rewriting the graph on the way is `compose::add`'s job, and
        // doing it here would edit lines the operator did not ask about.
        let new = crate::edit::NewContainer {
            key: asked.key.clone(),
            source: crate::edit::Source::Package {
                reference: reference.clone(),
                version: Some(wanted.clone()),
            },
            depends_on: container.depends_on.clone(),
        };

        let edited = match crate::edit::upsert_container(&text, &new)? {
            crate::edit::Outcome::Unchanged => {
                return Ok(serde_json::json!({
                    "status": "ok",
                    "container": asked.key,
                    "changed": false,
                    "version": wanted,
                    "detail": format!("already at {wanted}"),
                }));
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
        let from = current.unwrap_or_else(|| "unpinned".to_string());
        Ok(serde_json::json!({
            "status": up.status,
            "container": asked.key,
            "changed": true,
            "from": from,
            "to": wanted,
            "detail": format!("{} from {from} to {wanted}", asked.key),
            "down": serde_json::to_value(&down).unwrap_or(Value::Null),
            "up": serde_json::to_value(&up).unwrap_or(Value::Null),
        }))
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
        if let Some(key) = container {
            let project = self.project(self.resolve_file(file)?).await?;
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

/// Writes through a temporary file in the same directory, then renames.
///
/// A compose file half-written is a project that will not start and an operator
/// with no copy of what it said before. The rename is atomic within a
/// filesystem, so a reader sees the old file or the new one.
fn write_atomically(path: &Path, text: &str) -> Result<()> {
    let temp = path.with_extension("compose-add-tmp");
    std::fs::write(&temp, text).map_err(|source| ComposeError::Io {
        path: temp.clone(),
        source,
    })?;
    std::fs::rename(&temp, path).map_err(|source| ComposeError::Io {
        path: path.to_path_buf(),
        source,
    })
}

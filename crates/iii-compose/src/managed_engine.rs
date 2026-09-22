// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Lifecycle of the engine process owned by `iii compose --up`.

use std::{
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::{ExitStatus, Stdio},
    time::Duration,
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;

use crate::{
    config::{CONFIGURABLE_ENGINE_WORKERS, EngineSpec},
    error::{ComposeError, Result},
    process::{ChildOutput, DEFAULT_STOP_GRACE, Supervised, spawn_supervised_piped},
    shutdown::ShutdownSignal,
    state::{ChildRecord, ChildStatus, Reconciliation, StateStore, reconcile},
};

/// Maximum size of the active engine log before it rolls into an archive.
const ENGINE_LOG_MAX_BYTES: u64 = 10 * 1024 * 1024;
/// Number of old engine log segments retained beside `engine.log`.
const ENGINE_LOG_ARCHIVES: usize = 3;
const ENGINE_LOCK_FILE: &str = "engine.lock";
const ENGINE_CONFIG_FILE: &str = "engine-config.yaml";
/// The managed engine's `ChildRecord`, kept apart from `state.json`: the daemon
/// rewrites that file whole for its containers, while the engine is started
/// before the daemon exists and outlives every project it serves.
const ENGINE_RECORD_FILE: &str = "engine.json";
const DEFAULT_WORKER_MANAGER_HOST: &str = "0.0.0.0";
const DEFAULT_WORKER_MANAGER_PORT: u16 = 49134;

/// The engine process owned by one foreground compose invocation.
pub struct ManagedEngine {
    process: Supervised,
    logs: LogCapture,
    config_path: PathBuf,
    log_path: PathBuf,
    remove_config_on_stop: bool,
    /// Where this engine's record lives, when `start` wrote one.
    record_dir: Option<PathBuf>,
    _namespace_lock: Option<NamespaceLock>,
}

impl ManagedEngine {
    /// Starts the current `iii` executable with output detached from compose's
    /// terminal and captured in the owning project's namespace directory.
    ///
    /// An engine that an earlier compose left running in this namespace gets
    /// what workers get on restart. When the record names this compose file,
    /// the recorded PID still carries the birth identity recorded for it, and
    /// the engine was started from the configuration and launch identity this
    /// spec produces, it is adopted rather than fought over its port. A
    /// verified engine of this file started from anything else is stopped and
    /// replaced. Another file's engine is refused unless that file stopped it
    /// on purpose. A PID that cannot be verified is never signalled: the
    /// listener probe decides, and names it if the port is taken.
    ///
    /// `compose_path` is the canonical path of the owning file, as
    /// `ComposeFile::load` resolves it: the record compares it byte for byte.
    ///
    /// Returns `None` when `shutdown` was requested before an engine was
    /// owned, or once the engine this call came to own has been stopped again
    /// because the request arrived meanwhile. The future is never left half
    /// way: every step that owns a process runs to completion, so the caller
    /// must await it rather than race it against the signal.
    pub async fn start(
        spec: &EngineSpec,
        daemon_namespace: &str,
        compose_path: &Path,
        shutdown: &ShutdownSignal,
    ) -> Result<Option<Self>> {
        let executable =
            std::env::current_exe().map_err(|err| ComposeError::EngineSpawnFailed {
                message: format!("could not locate the current iii executable: {err}"),
            })?;
        Self::start_with(
            &executable,
            spec,
            daemon_namespace,
            compose_path,
            shutdown,
            DEFAULT_STOP_GRACE,
        )
        .await
    }

    /// `start` with the executable, the shutdown signal and the stop grace
    /// injectable: what tests drive with a stand-in engine.
    async fn start_with(
        executable: &Path,
        spec: &EngineSpec,
        daemon_namespace: &str,
        compose_path: &Path,
        shutdown: &ShutdownSignal,
        stop_grace: Duration,
    ) -> Result<Option<Self>> {
        let store = StateStore::for_project(daemon_namespace, compose_path)?;
        let namespace_dir = store.dir();
        // The lock comes before anything else: it makes one compose the only
        // reader of the record and the only one deciding what happens to the
        // engine it names. Waiting for it is the one step a signal may cut
        // short; nothing is owned yet. A failed probe later leaves the
        // namespace directory and its lock file behind, which is harmless.
        let lock_dir = namespace_dir.to_path_buf();
        let namespace = daemon_namespace.to_string();
        let Some(acquired) = shutdown
            .run(tokio::task::spawn_blocking(move || {
                NamespaceLock::acquire(&lock_dir, &namespace)
            }))
            .await
        else {
            return Ok(None);
        };
        let namespace_lock = acquired.map_err(|source| ComposeError::EngineSpawnFailed {
            message: format!("could not claim the managed engine namespace: {source}"),
        })??;
        let rendered = render_engine_config(spec)?;
        // Validates the endpoint too: an explicit iii-worker-manager port has
        // to match engine.url. That used to happen only inside the listener
        // probe; it has to hold for an adopted engine as well, so it comes first.
        let launch = LaunchIdentity::current(executable, spec, &rendered)?;
        let log_path = engine_log_path(namespace_dir);
        if shutdown.requested() {
            return Ok(None);
        }

        let mut unverifiable = None;
        match reconcile_previous_engine(namespace_dir, compose_path, &launch, &rendered)? {
            PreviousEngine::Alive {
                process,
                interchangeable: true,
            } => {
                let mut engine =
                    Self::adopted(process, namespace_dir.join(ENGINE_CONFIG_FILE), log_path);
                engine.record_dir = Some(namespace_dir.to_path_buf());
                engine._namespace_lock = Some(namespace_lock);
                if shutdown.requested() {
                    // Owned for an instant, like a fresh spawn below.
                    engine.stop(stop_grace).await;
                    return Ok(None);
                }
                return Ok(Some(engine));
            }
            PreviousEngine::Alive {
                process,
                interchangeable: false,
            } => {
                // Verifiably this file's engine, but not one the current spec
                // would have started: replace it, the restart a changed engine
                // section always gets. Awaited in full, lock held throughout:
                // a signal arriving now ends the start after the stop, never
                // in the middle of it.
                process.stop(stop_grace).await;
                if shutdown.requested() {
                    return Ok(None);
                }
            }
            PreviousEngine::Unverifiable { pid } => unverifiable = Some(pid),
            PreviousEngine::None => {}
        }

        ensure_listener_available(spec).map_err(|error| match (error, unverifiable) {
            (
                ComposeError::ManagedEngineListenerUnavailable {
                    listener,
                    hint,
                    source,
                },
                Some(pid),
            ) => ComposeError::ManagedEngineListenerUnavailable {
                listener,
                hint: format!(
                    "{hint}. Compose recorded engine pid {pid} for this namespace, but that pid \
                     can no longer be verified as the engine it started, so it was not signalled"
                ),
                source,
            },
            (error, _) => error,
        })?;
        let config_path = write_engine_config(&rendered, namespace_dir)?;
        let mut engine = Self::spawn_with_materialized_config(
            executable,
            &config_path,
            &log_path,
            daemon_namespace,
        )
        .await?;
        engine._namespace_lock = Some(namespace_lock);
        engine.record_dir = Some(namespace_dir.to_path_buf());
        let record = EngineRecord {
            compose_path: compose_path.to_path_buf(),
            launch,
            process: ChildRecord::from_supervised(&engine.process, ChildStatus::Starting),
        };
        if let Err(error) = save_engine_record(namespace_dir, &record) {
            // Without a record the engine could only ever be found by its
            // port. Better to start clean next time than to leave one behind.
            engine.stop(stop_grace).await;
            return Err(error);
        }
        if shutdown.requested() {
            // Owned for an instant: torn down here, the record marked
            // stopped, before the caller ever sees it.
            engine.stop(stop_grace).await;
            return Ok(None);
        }
        Ok(Some(engine))
    }

    /// Re-attaches to an engine an earlier compose left running. Its output
    /// streams belonged to that compose, so nothing new reaches `engine.log`
    /// from here on; what was captured before adoption is still there.
    fn adopted(process: Supervised, config_path: PathBuf, log_path: PathBuf) -> Self {
        Self {
            process,
            logs: LogCapture::finished(),
            config_path,
            log_path,
            remove_config_on_stop: true,
            record_dir: None,
            _namespace_lock: None,
        }
    }

    /// Whether this engine was inherited from an earlier compose rather than
    /// spawned here.
    pub fn is_adopted(&self) -> bool {
        self.process.is_adopted()
    }

    async fn spawn_with_materialized_config(
        executable: &Path,
        config_path: &Path,
        log_path: &Path,
        namespace: &str,
    ) -> Result<Self> {
        // Cancellation before spawn must remove the materialized configuration.
        struct PendingConfig(Option<PathBuf>);
        impl Drop for PendingConfig {
            fn drop(&mut self) {
                if let Some(path) = &self.0 {
                    let _ = std::fs::remove_file(path);
                }
            }
        }
        let mut pending = PendingConfig(Some(config_path.to_path_buf()));
        match Self::spawn_with_paths(executable, config_path, log_path, namespace).await {
            Ok(mut engine) => {
                pending.0 = None;
                engine.remove_config_on_stop = true;
                Ok(engine)
            }
            Err(error) => {
                let _ = std::fs::remove_file(config_path);
                Err(error)
            }
        }
    }

    async fn spawn_with_paths(
        executable: &Path,
        config_path: &Path,
        log_path: &Path,
        namespace: &str,
    ) -> Result<Self> {
        let parent = log_path.parent().unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(parent).map_err(|source| ComposeError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
        owner_only(parent).map_err(|source| ComposeError::Io {
            path: parent.to_path_buf(),
            source,
        })?;

        let owned_log_path = log_path.to_path_buf();
        let log = tokio::task::spawn_blocking(move || {
            RotatingLog::open(&owned_log_path, ENGINE_LOG_MAX_BYTES, ENGINE_LOG_ARCHIVES)
        })
        .await
        .map_err(|source| ComposeError::EngineSpawnFailed {
            message: format!("could not prepare the engine log: {source}"),
        })?
        .map_err(|source| ComposeError::Io {
            path: log_path.to_path_buf(),
            source,
        })?;

        let mut command = tokio::process::Command::new(executable);
        command
            .arg("--config")
            .arg(config_path)
            .stdin(Stdio::null());
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::process::CommandExt;

            command
                .as_std_mut()
                .arg0(crate::process_title::command_name(
                    crate::process_title::Role::Engine,
                    namespace,
                ));
        }
        #[cfg(not(target_os = "linux"))]
        let _ = namespace;

        let (process, output) =
            spawn_supervised_piped(command).map_err(|err| ComposeError::EngineSpawnFailed {
                message: format!("could not start {}: {err}", executable.display()),
            })?;
        let logs = capture_output(output, log);

        Ok(Self {
            process,
            logs,
            config_path: config_path.to_path_buf(),
            log_path: log_path.to_path_buf(),
            remove_config_on_stop: false,
            record_dir: None,
            _namespace_lock: None,
        })
    }

    pub fn pid(&self) -> u32 {
        self.process.pid
    }

    pub fn config_path(&self) -> &Path {
        &self.config_path
    }

    pub fn log_path(&self) -> &Path {
        &self.log_path
    }

    pub fn follow_command(&self) -> String {
        follow_command(&self.log_path)
    }

    pub fn log_tail(&self) -> Option<String> {
        log_tail(&self.log_path)
    }

    #[cfg(test)]
    async fn wait(&self) -> ExitStatus {
        let status = self.process.wait().await;
        self.finish_logging().await;
        status
    }

    pub fn poll(&self) -> crate::process::Outcome {
        self.process.poll()
    }

    pub async fn stop(&self, grace: Duration) -> ExitStatus {
        let status = self.process.stop(grace).await;
        self.finish_logging().await;
        if self.remove_config_on_stop {
            let _ = std::fs::remove_file(&self.config_path);
        }
        if let Some(dir) = &self.record_dir {
            // Best effort: a record left as is names a PID the next start
            // finds gone, which resolves the same way.
            let _ = mark_engine_record_stopped(dir);
        }
        status
    }

    pub async fn stop_with_default_grace(&self) -> ExitStatus {
        self.stop(DEFAULT_STOP_GRACE).await
    }

    /// Lets the output readers flush the final bytes after the child exits.
    pub async fn finish_logging(&self) {
        let _ = tokio::time::timeout(Duration::from_secs(2), self.logs.wait()).await;
    }
}

#[derive(Serialize)]
struct MaterializedEngineConfig<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    registration_namespace_grace_ms: Option<u64>,
    workers: Vec<MaterializedWorker<'a>>,
}

#[derive(Serialize)]
struct MaterializedWorker<'a> {
    name: &'a str,
    config: &'a serde_yaml::Value,
}

#[cfg(test)]
fn materialize_engine_config(spec: &EngineSpec, namespace_dir: &Path) -> Result<PathBuf> {
    write_engine_config(&render_engine_config(spec)?, namespace_dir)
}

/// The document a managed engine is started with. Deterministic for a spec,
/// which is what lets a surviving engine's configuration be compared with the
/// one the current file would produce.
fn render_engine_config(spec: &EngineSpec) -> Result<String> {
    let inferred_worker_manager = (!spec.workers.contains_key("iii-worker-manager"))
        .then(|| worker_manager_config_from_url(&spec.url))
        .transpose()?;
    let mut workers = Vec::new();
    for worker_type in CONFIGURABLE_ENGINE_WORKERS {
        workers.extend(
            spec.workers
                .iter()
                .filter(|(name, _)| crate::config::engine_worker_type(name) == *worker_type)
                .map(|(name, config)| MaterializedWorker { name, config }),
        );
        if *worker_type == "iii-worker-manager"
            && let Some(config) = inferred_worker_manager.as_ref()
        {
            workers.push(MaterializedWorker {
                name: "iii-worker-manager",
                config,
            });
        }
    }
    let document = MaterializedEngineConfig {
        registration_namespace_grace_ms: spec.registration_namespace_grace_ms,
        workers,
    };
    serde_yaml::to_string(&document).map_err(|err| ComposeError::EngineSpawnFailed {
        message: format!("could not serialize managed engine configuration: {err}"),
    })
}

/// Publishes the rendered configuration as `engine-config.yaml` in the
/// namespace directory.
fn write_engine_config(text: &str, namespace_dir: &Path) -> Result<PathBuf> {
    write_owner_only_atomically(namespace_dir, ENGINE_CONFIG_FILE, text.as_bytes())
}

/// Writes `namespace_dir/file_name` through a temporary file, so no reader ever
/// sees a partial document, with owner-only permissions on file and directory.
fn write_owner_only_atomically(
    namespace_dir: &Path,
    file_name: &str,
    bytes: &[u8],
) -> Result<PathBuf> {
    std::fs::create_dir_all(namespace_dir).map_err(|source| ComposeError::Io {
        path: namespace_dir.to_path_buf(),
        source,
    })?;
    owner_only(namespace_dir).map_err(|source| ComposeError::Io {
        path: namespace_dir.to_path_buf(),
        source,
    })?;

    let path = namespace_dir.join(file_name);
    // A temporary of this write alone. The digest cache is shared between
    // projects with no lock over it, and two writers must never meet in one
    // file: each gets its own, and the rename is what publishes it. A
    // temporary that never gets renamed is removed on the way out.
    static WRITES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    struct Temporary(PathBuf, bool);
    impl Drop for Temporary {
        fn drop(&mut self) {
            if !self.1 {
                let _ = std::fs::remove_file(&self.0);
            }
        }
    }
    let mut temp = Temporary(
        namespace_dir.join(format!(
            "{file_name}.{}.{}.tmp",
            std::process::id(),
            WRITES.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        )),
        false,
    );
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temp.0).map_err(|source| ComposeError::Io {
        path: temp.0.clone(),
        source,
    })?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|source| ComposeError::Io {
            path: temp.0.clone(),
            source,
        })?;
    owner_only(&temp.0).map_err(|source| ComposeError::Io {
        path: temp.0.clone(),
        source,
    })?;
    std::fs::rename(&temp.0, &path).map_err(|source| ComposeError::Io {
        path: path.clone(),
        source,
    })?;
    temp.1 = true;
    owner_only(&path).map_err(|source| ComposeError::Io {
        path: path.clone(),
        source,
    })?;
    Ok(path)
}

#[derive(Debug)]
struct EngineEndpoint {
    worker_host: String,
    port: u16,
}

fn engine_endpoint(engine_url: &str) -> Result<EngineEndpoint> {
    let url = url::Url::parse(engine_url).map_err(|_| ComposeError::EngineSpawnFailed {
        message: "engine.url must be a valid ws:// or wss:// URL".to_string(),
    })?;
    if !matches!(url.scheme(), "ws" | "wss") {
        return Err(ComposeError::EngineSpawnFailed {
            message: "engine.url must be a valid ws:// or wss:// URL".to_string(),
        });
    }
    let worker_host = match url.host() {
        Some(url::Host::Ipv6(address)) => format!("[{address}]"),
        Some(host) => host.to_string(),
        None => {
            return Err(ComposeError::EngineSpawnFailed {
                message: "engine.url must include a host".to_string(),
            });
        }
    };
    let port = url
        .port_or_known_default()
        .ok_or_else(|| ComposeError::EngineSpawnFailed {
            message: "engine.url must include a port".to_string(),
        })?;
    Ok(EngineEndpoint { worker_host, port })
}

fn effective_listener_endpoint(spec: &EngineSpec) -> Result<EngineEndpoint> {
    let url_endpoint = engine_endpoint(&spec.url)?;
    let Some(config) = spec.workers.get("iii-worker-manager") else {
        return Ok(url_endpoint);
    };
    let mapping = config
        .as_mapping()
        .expect("engine worker mappings are validated while the compose file is parsed");
    let field = |name: &str| mapping.get(serde_yaml::Value::String(name.to_string()));
    let worker_host = match field("host") {
        Some(serde_yaml::Value::String(host)) => expand_engine_env_references(host)?,
        Some(_) => {
            return Err(ComposeError::EngineSpawnFailed {
                message: "iii-worker-manager host must be a string".to_string(),
            });
        }
        None => DEFAULT_WORKER_MANAGER_HOST.to_string(),
    };
    let port = match field("port") {
        Some(serde_yaml::Value::Number(port)) => {
            port.as_u64().and_then(|port| u16::try_from(port).ok())
        }
        Some(serde_yaml::Value::String(port)) => expand_engine_env_references(port)?.parse().ok(),
        Some(_) => None,
        None => Some(DEFAULT_WORKER_MANAGER_PORT),
    }
    .ok_or_else(|| ComposeError::EngineSpawnFailed {
        message: "iii-worker-manager port must resolve to an integer from 0 to 65535".to_string(),
    })?;

    if port != url_endpoint.port {
        return Err(ComposeError::ManagedEngineEndpointMismatch {
            url_port: url_endpoint.port,
            listener_port: port,
        });
    }

    Ok(EngineEndpoint { worker_host, port })
}

/// Expands the `${NAME:default}` syntax used by the engine config loader.
fn expand_engine_env_references(text: &str) -> Result<String> {
    let mut output = String::with_capacity(text.len());
    let mut rest = text;

    while let Some(start) = rest.find("${") {
        output.push_str(&rest[..start]);
        let reference = &rest[start + 2..];
        let Some(end) = reference.find('}') else {
            output.push_str(&rest[start..]);
            return Ok(output);
        };
        let body = &reference[..end];
        let (name, default) = body
            .split_once(':')
            .map_or((body, None), |(name, default)| (name, Some(default)));
        let value = std::env::var(name)
            .ok()
            .or_else(|| default.map(str::to_string))
            .ok_or_else(|| ComposeError::EngineSpawnFailed {
                message: format!(
                    "environment variable '{name}' is required by iii-worker-manager config"
                ),
            })?;
        output.push_str(&value);
        rest = &reference[end + 1..];
    }

    output.push_str(rest);
    Ok(output)
}

fn ensure_listener_available(spec: &EngineSpec) -> Result<()> {
    let endpoint = effective_listener_endpoint(spec)?;
    let listener = format!("{}:{}", endpoint.worker_host, endpoint.port);
    std::net::TcpListener::bind(&listener)
        .map(drop)
        .map_err(|source| ComposeError::ManagedEngineListenerUnavailable {
            hint: listener_hint(&endpoint, &source),
            listener,
            source,
        })
}

/// Why the engine's listener could not be bound, in terms an operator can act
/// on. Only `AddrInUse` means something else holds the port; the other
/// failures are about the address itself, and hunting for a process to stop
/// would not help.
fn listener_hint(endpoint: &EngineEndpoint, error: &std::io::Error) -> String {
    use std::io::ErrorKind;

    let port = endpoint.port;
    match error.kind() {
        ErrorKind::AddrInUse => {
            let inspect = if cfg!(windows) {
                format!("netstat -ano | findstr :{port}")
            } else {
                format!("lsof -nP -iTCP:{port} -sTCP:LISTEN")
            };
            format!(
                "Something else is listening on port {port}: an engine left behind by an earlier \
                 `iii compose --up`, another project whose engine.url uses the same port, or an \
                 unrelated program. Find it with `{inspect}` and stop it, or give this project \
                 its own port in engine.url"
            )
        }
        ErrorKind::PermissionDenied => format!(
            "This user may not bind port {port} (ports below 1024 usually need privileges). Use \
             another port in engine.url"
        ),
        ErrorKind::AddrNotAvailable => format!(
            "No local interface has the address {}. Check the host in engine.url or the \
             iii-worker-manager host setting",
            endpoint.worker_host
        ),
        _ => "Check the host and port in engine.url".to_string(),
    }
}

/// What the engine resolves on its own and a byte-identical
/// `engine-config.yaml` does not capture. Two runs that differ here are not
/// interchangeable: a surviving engine with another identity is replaced,
/// never adopted. The comparison is deliberately conservative; a spurious
/// difference costs a restart, a missed one keeps an engine the file no
/// longer describes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct LaunchIdentity {
    /// `engine.url` as written: what the daemon connects to.
    engine_url: String,
    /// The listener the engine binds, `host:port`, explicit or inferred.
    listener: String,
    /// The working directory the engine inherits. A relative path in
    /// `engine.workers`, such as `./config`, resolves against it.
    cwd: PathBuf,
    /// The `iii` binary the engine runs. An engine of another build is not
    /// the engine this file would start now, whether `iii update` replaced
    /// the file in place or another binary comes first on PATH.
    executable: ExecutableIdentity,
    /// Fingerprint of the environment the engine reads: the `${VAR}`
    /// references the materialized configuration leaves for it to expand, and
    /// the variables it consults directly. Names and values are hashed, so
    /// nothing is stored in clear; the record is still owner-only and should
    /// be treated as sensitive.
    env_fingerprint: String,
}

/// Path, size and mtime describe where the binary came from; the digest is
/// what proves it is the same build. An install that preserves timestamps,
/// or a rebuild of the same size within the same second, changes neither of
/// the first three.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ExecutableIdentity {
    path: PathBuf,
    len: u64,
    /// Modification time in whole seconds since the Unix epoch, when the
    /// filesystem reports one.
    modified: Option<u64>,
    /// SHA-256 of the file's contents.
    sha256: String,
}

impl ExecutableIdentity {
    /// The identity of the binary at `executable`, digest included.
    fn of(executable: &Path) -> Result<Self> {
        let (stamp, sha256) = executable_digest(executable, digest_cache_dir().as_deref())?;
        Ok(Self {
            path: executable.to_path_buf(),
            len: stamp.len,
            modified: stamp.modified.map(|(secs, _)| secs),
            sha256,
        })
    }
}

/// Files below this size are hashed every time; the cache is for the `iii`
/// binary, not for a test's shell script.
const DIGEST_CACHE_MIN_LEN: u64 = 1 << 20;
const DIGEST_CACHE_FILE: &str = "executable-digests.json";
const DIGEST_CACHE_ENTRIES: usize = 32;

/// Where digests are remembered between starts: the user's cache directory,
/// never the project. Only where the stamp carries a change identity the
/// kernel maintains (device, inode and change time on Unix); elsewhere, and
/// on a system without a cache directory, the digest is computed every time.
fn digest_cache_dir() -> Option<PathBuf> {
    if cfg!(unix) {
        dirs::cache_dir().map(|cache| cache.join("iii").join("compose"))
    } else {
        None
    }
}

/// SHA-256 of `path`, with the stamp of the file it was computed over. The
/// digest is the proof that two starts run the same build; the stamp is only
/// the cache key that spares hashing an unchanged binary at every start.
///
/// The cache is trusted as far as a local filesystem's change identity is: a
/// rewrite in place moves the change time, which no user tool preserves,
/// and a new file has a new inode. A filesystem that keeps neither (some
/// remote or virtual ones) is outside that model; the cache directory is the
/// user's own and is created owner-only. An entry is written only for a file
/// that stayed the same while it was read.
fn executable_digest(path: &Path, cache_dir: Option<&Path>) -> Result<(FileStamp, String)> {
    let io = |source| ComposeError::Io {
        path: path.to_path_buf(),
        source,
    };
    let mut file = std::fs::File::open(path).map_err(io)?;
    let mut before = FileStamp::of(&file.metadata().map_err(io)?);
    let cache_dir = cache_dir.filter(|_| before.len >= DIGEST_CACHE_MIN_LEN);
    let mut cache = cache_dir.map(DigestCache::load).unwrap_or_default();
    if let Some(hit) = cache.hit(path, &before) {
        return Ok((before, hit.to_string()));
    }
    for _ in 0..3 {
        use std::io::Seek;

        file.rewind().map_err(io)?;
        let mut hasher = Sha256::new();
        std::io::copy(&mut file, &mut hasher).map_err(io)?;
        let sha256 = hex::encode(hasher.finalize());
        let after = FileStamp::of(&file.metadata().map_err(io)?);
        if after != before {
            // Written while being read: neither the digest nor the stamp
            // describes one file. Read again.
            before = after;
            continue;
        }
        if let Some(cache_dir) = cache_dir {
            // Best effort: a cache that cannot be written only costs a rehash.
            cache.remember(path, before.clone(), &sha256);
            let _ = cache.store(cache_dir);
        }
        return Ok((before, sha256));
    }
    Err(ComposeError::EngineSpawnFailed {
        message: format!(
            "{} kept changing while its digest was being computed",
            path.display()
        ),
    })
}

/// What the kernel knows about a file without reading it. On Unix this
/// includes the change identity the cache relies on; elsewhere it describes
/// the file for the record and no cache is consulted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct FileStamp {
    len: u64,
    modified: Option<(u64, u32)>,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(unix)]
    changed: (i64, i64),
}

impl FileStamp {
    /// The stamp of a file, from metadata read through its open handle.
    fn of(metadata: &std::fs::Metadata) -> Self {
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;

        Self {
            len: metadata.len(),
            modified: metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|since| (since.as_secs(), since.subsec_nanos())),
            #[cfg(unix)]
            device: metadata.dev(),
            #[cfg(unix)]
            inode: metadata.ino(),
            #[cfg(unix)]
            changed: (metadata.ctime(), metadata.ctime_nsec()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct CachedDigest {
    stamp: FileStamp,
    sha256: String,
}

/// `path -> digest` for the few binaries a machine starts engines from.
/// Tolerant on read (a corrupt cache is an empty one) and bounded on write.
#[derive(Debug, Default, Serialize, Deserialize)]
struct DigestCache {
    entries: std::collections::BTreeMap<PathBuf, CachedDigest>,
}

impl DigestCache {
    /// The cache kept in `cache_dir`; empty when there is none or it cannot
    /// be read.
    fn load(cache_dir: &Path) -> Self {
        std::fs::read_to_string(cache_dir.join(DIGEST_CACHE_FILE))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// A cached digest for `path`, provided the stamp is the same and the
    /// entry looks like a digest at all: anything else is recomputed.
    fn hit(&self, path: &Path, stamp: &FileStamp) -> Option<&str> {
        self.entries
            .get(path)
            .filter(|cached| cached.stamp == *stamp)
            .map(|cached| cached.sha256.as_str())
            .filter(|sha256| sha256.len() == 64 && sha256.bytes().all(|b| b.is_ascii_hexdigit()))
    }

    /// Records `sha256` for `path`, forgetting files that no longer exist
    /// and starting over rather than growing past the bound.
    fn remember(&mut self, path: &Path, stamp: FileStamp, sha256: &str) {
        self.entries
            .retain(|known, _| known == path || known.exists());
        if self.entries.len() >= DIGEST_CACHE_ENTRIES {
            self.entries.clear();
        }
        self.entries.insert(
            path.to_path_buf(),
            CachedDigest {
                stamp,
                sha256: sha256.to_string(),
            },
        );
    }

    /// Publishes the cache in `cache_dir`, atomically and owner-only.
    fn store(&self, cache_dir: &Path) -> Result<()> {
        let bytes =
            serde_json::to_vec_pretty(self).map_err(|err| ComposeError::EngineSpawnFailed {
                message: format!("could not serialize the executable digest cache: {err}"),
            })?;
        write_owner_only_atomically(cache_dir, DIGEST_CACHE_FILE, &bytes).map(drop)
    }
}

impl LaunchIdentity {
    /// The identity a start of `spec` from `executable` would have right now.
    fn current(executable: &Path, spec: &EngineSpec, rendered_config: &str) -> Result<Self> {
        let endpoint = effective_listener_endpoint(spec)?;
        let cwd = std::env::current_dir().map_err(|err| ComposeError::EngineSpawnFailed {
            message: format!("could not read the current directory: {err}"),
        })?;
        Ok(Self {
            engine_url: spec.url.clone(),
            listener: format!("{}:{}", endpoint.worker_host, endpoint.port),
            cwd,
            executable: ExecutableIdentity::of(executable)?,
            env_fingerprint: env_fingerprint(rendered_config),
        })
    }
}

/// The variables the engine reads from its environment directly, beyond the
/// references in its configuration file: its own `III_*` switches, the
/// OpenTelemetry and PostHog settings, the CI markers and `HOME` that decide
/// telemetry opt-out and data directories, the service identifiers and the
/// logging filter. This is a selection with a bias towards restarting: an
/// `III_*` variable meant only for compose changes the identity too, at the
/// cost of a restart, never of a stale engine. Shell-shaped variables such
/// as `PATH` or `TZ` are left out on purpose.
fn engine_reads_directly(name: &str) -> bool {
    name.starts_with("III_")
        || name.starts_with("OTEL_")
        || name.starts_with("POSTHOG_")
        || matches!(
            name,
            "HOME"
                | "RUST_LOG"
                | "DEPLOYMENT_ENVIRONMENT"
                | "SERVICE_NAMESPACE"
                | "SERVICE_VERSION"
                | "CI"
                | "GITHUB_ACTIONS"
                | "GITLAB_CI"
                | "CIRCLECI"
                | "JENKINS_URL"
                | "TRAVIS"
                | "BUILDKITE"
                | "TF_BUILD"
                | "CODEBUILD_BUILD_ID"
                | "BITBUCKET_BUILD_NUMBER"
                | "DRONE"
                | "TEAMCITY_VERSION"
        )
}

/// The variables referenced as `${NAME}` or `${NAME:default}` in `rendered`,
/// found the way the engine's own expansion does (leftmost-first, a name of
/// one or more characters other than `}` and `:`, an optional default up to
/// the next `}`), in order of first appearance and without repeats.
fn referenced_variables(rendered: &str) -> Vec<&str> {
    let bytes = rendered.as_bytes();
    let mut names: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' && bytes.get(i + 1) == Some(&b'{') {
            let name_start = i + 2;
            let mut j = name_start;
            while j < bytes.len() && bytes[j] != b'}' && bytes[j] != b':' {
                j += 1;
            }
            if j > name_start && j < bytes.len() {
                let end = if bytes[j] == b'}' {
                    Some(j)
                } else {
                    bytes[j + 1..]
                        .iter()
                        .position(|b| *b == b'}')
                        .map(|k| j + 1 + k)
                };
                if let Some(end) = end {
                    let name = &rendered[name_start..j];
                    if !names.contains(&name) {
                        names.push(name);
                    }
                    i = end + 1;
                    continue;
                }
            }
        }
        i += 1;
    }
    names
}

/// Hashes the name and current value of every variable the engine would
/// read: the references in `rendered` and the ones it consults directly.
/// Read with `env::var`, as the engine does, so a value that is not Unicode
/// counts as unset there too.
fn env_fingerprint(rendered: &str) -> String {
    let mut names: std::collections::BTreeSet<String> = referenced_variables(rendered)
        .into_iter()
        .map(str::to_string)
        .collect();
    names.extend(
        std::env::vars_os()
            .filter_map(|(name, _)| name.into_string().ok())
            .filter(|name| engine_reads_directly(name)),
    );
    fingerprint_of(names.into_iter().map(|name| {
        let value = std::env::var(&name).ok();
        (name, value)
    }))
}

/// Length-prefixed, so no two sequences of pairs share an input to the hash:
/// a value containing a newline or an `=` cannot masquerade as another pair.
fn fingerprint_of(pairs: impl Iterator<Item = (String, Option<String>)>) -> String {
    /// Length-prefixed bytes: a field cannot run into the next.
    fn field(hasher: &mut Sha256, bytes: &[u8]) {
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(bytes);
    }

    let mut hasher = Sha256::new();
    for (name, value) in pairs {
        field(&mut hasher, name.as_bytes());
        match value {
            Some(value) => {
                hasher.update([1]);
                field(&mut hasher, value.as_bytes());
            }
            None => hasher.update([0]),
        }
    }
    hex::encode(hasher.finalize())
}

/// What `start` leaves behind for the next compose in this namespace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct EngineRecord {
    /// The compose file whose engine this is. Two files in one directory and
    /// namespace share this directory, and neither may adopt or stop the
    /// other's engine: the rule `DaemonState::check_binding` applies to
    /// workers.
    compose_path: PathBuf,
    launch: LaunchIdentity,
    process: ChildRecord,
}

/// What an earlier compose left running in this namespace.
enum PreviousEngine {
    /// No record, a record whose file stopped its engine on purpose, or a
    /// process that is gone.
    None,
    /// The recorded PID is still the engine that was recorded, and it belongs
    /// to this compose file. `interchangeable` is whether it was started from
    /// the configuration and launch identity the current spec produces.
    Alive {
        process: Supervised,
        interchangeable: bool,
    },
    /// A live PID that is not provably the recorded engine: recycled, or a
    /// platform that cannot fingerprint. Never signalled, only reported.
    Unverifiable { pid: u32 },
}

impl std::fmt::Debug for PreviousEngine {
    /// The decision and the pid: a `Supervised` handle has nothing readable.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => f.write_str("None"),
            Self::Alive {
                process,
                interchangeable,
            } => f
                .debug_struct("Alive")
                .field("pid", &process.pid)
                .field("interchangeable", interchangeable)
                .finish(),
            Self::Unverifiable { pid } => f.debug_struct("Unverifiable").field("pid", pid).finish(),
        }
    }
}

/// Read-only apart from the adoption handle: signals nothing, kills nothing.
/// Another file's record is refused before anything else is looked at,
/// unless that file stopped its engine on purpose.
fn reconcile_previous_engine(
    namespace_dir: &Path,
    compose_path: &Path,
    launch: &LaunchIdentity,
    expected_config: &str,
) -> Result<PreviousEngine> {
    let Some(record) = load_engine_record(namespace_dir)? else {
        return Ok(PreviousEngine::None);
    };
    let reconciliation = reconcile(&record.process);
    if record.compose_path != compose_path && reconciliation != Reconciliation::Stopped {
        return Err(ComposeError::InvalidState {
            path: engine_record_path(namespace_dir),
            message: format!(
                "it records {} instead. Use a different namespace for each compose file in this \
                 directory",
                record.compose_path.display()
            ),
        });
    }
    Ok(match reconciliation {
        Reconciliation::Stopped | Reconciliation::Gone => PreviousEngine::None,
        Reconciliation::Unverifiable => PreviousEngine::Unverifiable {
            pid: record.process.pid,
        },
        Reconciliation::Adopt => {
            match Supervised::adopt(record.process.pid, &record.process.birth) {
                Some(process) => {
                    let same_config =
                        std::fs::read_to_string(namespace_dir.join(ENGINE_CONFIG_FILE))
                            .map(|current| current == expected_config)
                            .unwrap_or(false);
                    PreviousEngine::Alive {
                        process,
                        interchangeable: same_config && record.launch == *launch,
                    }
                }
                // It exited between the two identity reads.
                None => PreviousEngine::None,
            }
        }
    })
}

/// Where the engine's record lives in the namespace directory.
fn engine_record_path(namespace_dir: &Path) -> PathBuf {
    namespace_dir.join(ENGINE_RECORD_FILE)
}

/// The record in `namespace_dir`; `None` when no compose has written one.
fn load_engine_record(namespace_dir: &Path) -> Result<Option<EngineRecord>> {
    let path = engine_record_path(namespace_dir);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(ComposeError::Io { path, source }),
    };
    // As with state.json, a corrupt record is an error and never a silent
    // reset: that is exactly the moment an engine may be running unaccounted for.
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|err| ComposeError::InvalidState {
            path,
            message: err.to_string(),
        })
}

/// Publishes the record in `namespace_dir`, atomically and owner-only.
fn save_engine_record(namespace_dir: &Path, record: &EngineRecord) -> Result<()> {
    let bytes =
        serde_json::to_vec_pretty(record).map_err(|err| ComposeError::EngineSpawnFailed {
            message: format!("could not serialize the managed engine record: {err}"),
        })?;
    write_owner_only_atomically(namespace_dir, ENGINE_RECORD_FILE, &bytes).map(drop)
}

/// Records that this compose stopped its engine on purpose, so the next start
/// neither adopts nor reports the PID even if it is recycled meanwhile.
fn mark_engine_record_stopped(namespace_dir: &Path) -> Result<()> {
    if let Some(mut record) = load_engine_record(namespace_dir)? {
        record.process.status = ChildStatus::Stopped;
        save_engine_record(namespace_dir, &record)?;
    }
    Ok(())
}

fn worker_manager_config_from_url(engine_url: &str) -> Result<serde_yaml::Value> {
    let endpoint = engine_endpoint(engine_url)?;
    let config = serde_yaml::Mapping::from_iter([
        (
            serde_yaml::Value::String("host".to_string()),
            serde_yaml::Value::String(endpoint.worker_host),
        ),
        (
            serde_yaml::Value::String("port".to_string()),
            serde_yaml::Value::Number(endpoint.port.into()),
        ),
    ]);

    Ok(serde_yaml::Value::Mapping(config))
}

/// Cross-process ownership of one managed engine in a project and namespace.
///
/// The lock file persists, but the kernel lock is released with this guard or
/// when the process exits, so a crash cannot strand the namespace.
struct NamespaceLock {
    _lock: fslock::LockFile,
}

impl NamespaceLock {
    fn acquire(namespace_dir: &Path, namespace: &str) -> Result<Self> {
        std::fs::create_dir_all(namespace_dir).map_err(|source| ComposeError::Io {
            path: namespace_dir.to_path_buf(),
            source,
        })?;
        owner_only(namespace_dir).map_err(|source| ComposeError::Io {
            path: namespace_dir.to_path_buf(),
            source,
        })?;

        let path = namespace_dir.join(ENGINE_LOCK_FILE);
        let mut options = std::fs::OpenOptions::new();
        options.create(true).read(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options.open(&path).map_err(|source| ComposeError::Io {
            path: path.clone(),
            source,
        })?;
        owner_only(&path).map_err(|source| ComposeError::Io {
            path: path.clone(),
            source,
        })?;

        let mut lock = fslock::LockFile::open(&path).map_err(|source| ComposeError::Io {
            path: path.clone(),
            source,
        })?;
        match lock.try_lock_with_pid() {
            Ok(true) => Ok(Self { _lock: lock }),
            Ok(false) => Err(ComposeError::DaemonNamespaceTaken {
                namespace: namespace.to_string(),
            }),
            Err(source) => Err(ComposeError::Io { path, source }),
        }
    }
}

struct LogCapture(tokio::sync::watch::Receiver<bool>);

impl LogCapture {
    /// A capture with nothing left to drain: an adopted engine's pipes died
    /// with the compose that spawned it.
    fn finished() -> Self {
        let (_sender, receiver) = tokio::sync::watch::channel(true);
        Self(receiver)
    }

    async fn wait(&self) {
        let mut done = self.0.clone();
        while !*done.borrow_and_update() {
            if done.changed().await.is_err() {
                break;
            }
        }
    }
}

/// One writer owns both child streams, so rotation cannot race stderr against
/// stdout. Readers keep draining even if disk writes fail, avoiding a full pipe
/// that would otherwise stall the engine itself.
fn capture_output(output: ChildOutput, log: RotatingLog) -> LogCapture {
    let (chunks_tx, mut chunks_rx) = tokio::sync::mpsc::channel::<(usize, Vec<u8>)>(64);
    for (stream_id, stream) in [
        output
            .stdout
            .map(|stream| Box::new(stream) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
        output
            .stderr
            .map(|stream| Box::new(stream) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
    ]
    .into_iter()
    .enumerate()
    .filter_map(|(stream_id, stream)| stream.map(|stream| (stream_id, stream)))
    {
        let chunks_tx = chunks_tx.clone();
        tokio::spawn(async move {
            let mut stream = stream;
            let mut buffer = vec![0_u8; 8 * 1024];
            while let Ok(read) = stream.read(&mut buffer).await {
                if read == 0
                    || chunks_tx
                        .send((stream_id, buffer[..read].to_vec()))
                        .await
                        .is_err()
                {
                    break;
                }
            }
        });
    }
    drop(chunks_tx);

    let (done_tx, done_rx) = tokio::sync::watch::channel(false);
    tokio::task::spawn_blocking(move || {
        let mut log = Some(log);
        let mut sanitizers = [TerminalSanitizer::default(), TerminalSanitizer::default()];
        while let Some((stream_id, chunk)) = chunks_rx.blocking_recv() {
            let chunk = sanitizers[stream_id].sanitize(&chunk);
            if let Some(sink) = log.as_mut()
                && !chunk.is_empty()
                && sink.write_bounded(&chunk).is_err()
            {
                log = None;
            }
        }
        if let Some(mut sink) = log {
            let _ = sink.file.flush();
        }
        let _ = done_tx.send(true);
    });

    LogCapture(done_rx)
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum TerminalState {
    #[default]
    Ground,
    Escape,
    Csi,
    Osc,
    OscEscape,
    ControlString,
    ControlStringEscape,
}

/// Removes terminal control sequences while preserving ordinary UTF-8 output.
///
/// State is retained across pipe reads because an ANSI/OSC sequence or a UTF-8
/// scalar can be split at any byte boundary. Unterminated control strings stay
/// suppressed, which fails closed instead of letting their payload reach a
/// terminal through `tail -f` or an error summary.
#[derive(Debug, Default)]
pub(crate) struct TerminalSanitizer {
    state: TerminalState,
    utf8: Vec<u8>,
    utf8_expected: usize,
}

impl TerminalSanitizer {
    pub(crate) fn sanitize(&mut self, bytes: &[u8]) -> Vec<u8> {
        let mut clean = Vec::with_capacity(bytes.len());

        for &input in bytes {
            let mut pending = Some(input);
            while let Some(byte) = pending.take() {
                if self.state == TerminalState::Ground && !self.utf8.is_empty() {
                    if !(0x80..=0xbf).contains(&byte) {
                        self.utf8.clear();
                        self.utf8_expected = 0;
                        pending = Some(byte);
                        continue;
                    }

                    self.utf8.push(byte);
                    if self.utf8.len() == self.utf8_expected {
                        if let Ok(text) = std::str::from_utf8(&self.utf8)
                            && text.chars().all(is_safe_log_character)
                        {
                            clean.extend_from_slice(&self.utf8);
                        }
                        self.utf8.clear();
                        self.utf8_expected = 0;
                    }
                    continue;
                }

                match self.state {
                    TerminalState::Ground => match byte {
                        0x1b => self.state = TerminalState::Escape,
                        b'\n' | b'\t' | 0x20..=0x7e => clean.push(byte),
                        0xc2..=0xdf => self.start_utf8(byte, 2),
                        0xe0..=0xef => self.start_utf8(byte, 3),
                        0xf0..=0xf4 => self.start_utf8(byte, 4),
                        _ => {}
                    },
                    TerminalState::Escape => {
                        self.state = match byte {
                            0x1b => TerminalState::Escape,
                            b'[' => TerminalState::Csi,
                            b']' => TerminalState::Osc,
                            b'P' | b'X' | b'^' | b'_' => TerminalState::ControlString,
                            _ => TerminalState::Ground,
                        };
                    }
                    TerminalState::Csi => {
                        if byte == 0x1b {
                            self.state = TerminalState::Escape;
                        } else if (0x40..=0x7e).contains(&byte) {
                            self.state = TerminalState::Ground;
                        }
                    }
                    TerminalState::Osc => match byte {
                        0x07 => self.state = TerminalState::Ground,
                        0x1b => self.state = TerminalState::OscEscape,
                        _ => {}
                    },
                    TerminalState::OscEscape => {
                        self.state = match byte {
                            b'\\' => TerminalState::Ground,
                            0x1b => TerminalState::OscEscape,
                            _ => TerminalState::Osc,
                        };
                    }
                    TerminalState::ControlString => {
                        if byte == 0x1b {
                            self.state = TerminalState::ControlStringEscape;
                        }
                    }
                    TerminalState::ControlStringEscape => {
                        self.state = match byte {
                            b'\\' => TerminalState::Ground,
                            0x1b => TerminalState::ControlStringEscape,
                            _ => TerminalState::ControlString,
                        };
                    }
                }
            }
        }

        clean
    }

    fn start_utf8(&mut self, byte: u8, expected: usize) {
        self.utf8.push(byte);
        self.utf8_expected = expected;
    }
}

fn is_safe_log_character(character: char) -> bool {
    !character.is_control()
        && !matches!(
            character,
            '\u{061c}'
                | '\u{200e}'
                | '\u{200f}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202a}'..='\u{202e}'
                | '\u{2066}'..='\u{2069}'
        )
}

fn sanitize_terminal_output(bytes: &[u8]) -> Vec<u8> {
    TerminalSanitizer::default().sanitize(bytes)
}

struct RotatingLog {
    path: PathBuf,
    file: std::fs::File,
    size: u64,
    max_bytes: u64,
    archives: usize,
}

impl RotatingLog {
    fn open(path: &Path, max_bytes: u64, archives: usize) -> std::io::Result<Self> {
        let max_bytes = max_bytes.max(1);
        let mut options = std::fs::OpenOptions::new();
        options.create(true).read(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(path)?;
        owner_only(path)?;
        let mut size = file.metadata()?.len();
        if size > 0 && size < max_bytes {
            file.seek(SeekFrom::Start(0))?;
            let mut existing = Vec::with_capacity(size as usize);
            file.read_to_end(&mut existing)?;
            let clean = sanitize_terminal_output(&existing);
            if clean != existing {
                file.set_len(0)?;
                file.seek(SeekFrom::Start(0))?;
                file.write_all(&clean)?;
                file.flush()?;
                size = clean.len() as u64;
            }
        }
        let mut log = Self {
            path: path.to_path_buf(),
            file,
            size,
            max_bytes,
            archives,
        };
        if log.size >= log.max_bytes {
            log.rotate()?;
        }
        Ok(log)
    }

    fn write_bounded(&mut self, mut bytes: &[u8]) -> std::io::Result<()> {
        while !bytes.is_empty() {
            if self.size >= self.max_bytes {
                self.rotate()?;
            }
            let available = (self.max_bytes - self.size) as usize;
            let count = available.min(bytes.len());
            self.file.write_all(&bytes[..count])?;
            self.size += count as u64;
            bytes = &bytes[count..];
        }
        Ok(())
    }

    fn rotate(&mut self) -> std::io::Result<()> {
        self.file.flush()?;

        if self.archives > 0 {
            let oldest = archive_path(&self.path, self.archives);
            if oldest.exists() {
                std::fs::remove_file(oldest)?;
            }
            for index in (1..self.archives).rev() {
                let source = archive_path(&self.path, index);
                if source.exists() {
                    std::fs::rename(source, archive_path(&self.path, index + 1))?;
                }
            }

            let archive = archive_path(&self.path, 1);
            let mut options = std::fs::OpenOptions::new();
            options.create(true).write(true).truncate(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut archive_file = options.open(&archive)?;
            owner_only(&archive)?;

            let start = self.size.saturating_sub(self.max_bytes);
            self.file.seek(SeekFrom::Start(start))?;
            let mut remaining = self.max_bytes.min(self.size);
            let mut buffer = [0_u8; 8 * 1024];
            let mut sanitizer = TerminalSanitizer::default();
            while remaining > 0 {
                let limit = remaining.min(buffer.len() as u64) as usize;
                let count = self.file.read(&mut buffer[..limit])?;
                if count == 0 {
                    break;
                }
                archive_file.write_all(&sanitizer.sanitize(&buffer[..count]))?;
                remaining -= count as u64;
            }
            archive_file.flush()?;
        }

        // Keep the active path and inode stable so `tail -f engine.log` keeps
        // following across rotations on Unix and PowerShell keeps its handle.
        self.file.set_len(0)?;
        self.file.seek(SeekFrom::Start(0))?;
        self.size = 0;
        Ok(())
    }
}

fn archive_path(path: &Path, index: usize) -> PathBuf {
    let mut archive = path.as_os_str().to_os_string();
    archive.push(format!(".{index}"));
    PathBuf::from(archive)
}

fn engine_log_path(namespace_dir: &Path) -> PathBuf {
    namespace_dir.join("engine.log")
}

#[cfg(unix)]
fn follow_command(path: &Path) -> String {
    let quoted = path.to_string_lossy().replace('\'', "'\"'\"'");
    format!("tail -f '{quoted}'")
}

#[cfg(windows)]
fn follow_command(path: &Path) -> String {
    let quoted = path.to_string_lossy().replace('\'', "''");
    format!("Get-Content -LiteralPath '{quoted}' -Wait")
}

fn log_tail(path: &Path) -> Option<String> {
    const LINES: usize = 5;
    const WIDTH: usize = 240;
    const MAX_TAIL_BYTES: u64 = 64 * 1024;

    let mut file = std::fs::File::open(path).ok()?;
    let length = file.metadata().ok()?.len();
    let start = length.saturating_sub(MAX_TAIL_BYTES);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::with_capacity((length - start) as usize);
    file.take(MAX_TAIL_BYTES).read_to_end(&mut bytes).ok()?;
    let clean = sanitize_terminal_output(&bytes);
    let text = String::from_utf8_lossy(&clean);
    let text = if start > 0 {
        text.split_once('\n')
            .map_or(text.as_ref(), |(_, rest)| rest)
    } else {
        text.as_ref()
    };
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
            .map(|line| format!("  {}", line.chars().take(WIDTH).collect::<String>()))
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

#[cfg(unix)]
fn owner_only(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(
        path,
        std::fs::Permissions::from_mode(if path.is_dir() { 0o700 } else { 0o600 }),
    )
}

#[cfg(not(unix))]
fn owner_only(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{path::Path, time::Duration};

    use super::*;

    fn engine_spec() -> crate::config::EngineSpec {
        crate::ComposeFile::parse(
            r#"
engine:
  url: ws://127.0.0.1:50123
  registration_namespace_grace_ms: 2500
  workers:
    iii-sandbox:
      auto_install: false
    configuration:
      adapter:
        name: fs
        config:
          directory: ./config
    iii-worker-manager:
      port: ${ENGINE_PORT:50123}
    iii-worker-manager#rbac:
      port: 50124
containers: {}
"#,
            "/srv/app/worker-compose.yaml",
        )
        .unwrap()
        .engine
        .unwrap()
    }

    #[test]
    fn materialized_config_contains_only_engine_fields_in_canonical_worker_order() {
        let dir = tempfile::tempdir().unwrap();
        let path = materialize_engine_config(&engine_spec(), dir.path()).unwrap();
        let document: serde_yaml::Value =
            serde_yaml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();

        assert_eq!(document["registration_namespace_grace_ms"], 2500);
        assert!(document.get("url").is_none());
        assert!(document.get("containers").is_none());
        let workers = document["workers"].as_sequence().unwrap();
        assert_eq!(
            workers
                .iter()
                .map(|entry| entry["name"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec![
                "configuration",
                "iii-worker-manager",
                "iii-worker-manager#rbac",
                "iii-sandbox"
            ]
        );
        assert_eq!(workers[1]["config"]["port"], "${ENGINE_PORT:50123}");
        assert_eq!(workers[3]["config"]["auto_install"], false);
    }

    #[test]
    fn materialized_config_infers_worker_manager_endpoint_from_engine_url() {
        let spec = crate::ComposeFile::parse(
            r#"
engine:
  url: ws://127.0.0.1:50123
  workers: {}
containers: {}
"#,
            "/srv/app/worker-compose.yaml",
        )
        .unwrap()
        .engine
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = materialize_engine_config(&spec, dir.path()).unwrap();
        let document: serde_yaml::Value =
            serde_yaml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();

        assert_eq!(
            document["workers"],
            serde_yaml::from_str::<serde_yaml::Value>(
                r#"
- name: iii-worker-manager
  config:
    host: 127.0.0.1
    port: 50123
"#,
            )
            .unwrap()
        );
    }

    #[test]
    fn occupied_managed_engine_listener_is_refused() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut spec = engine_spec();
        spec.url = format!("ws://127.0.0.1:{port}");
        spec.workers.remove("iii-worker-manager");

        let error = ensure_listener_available(&spec)
            .expect_err("an occupied managed listener must be rejected");

        assert_eq!(error.code(), "MANAGED_ENGINE_LISTENER_UNAVAILABLE");
    }

    #[test]
    fn explicit_worker_manager_port_must_match_the_engine_url() {
        let spec = crate::ComposeFile::parse(
            r#"
engine:
  url: ws://127.0.0.1:50123
  workers:
    iii-worker-manager:
      host: 0.0.0.0
      port: 60123
containers: {}
"#,
            "/srv/app/worker-compose.yaml",
        )
        .unwrap()
        .engine
        .unwrap();

        let error = effective_listener_endpoint(&spec)
            .expect_err("different URL and listener ports must be rejected");

        assert_eq!(error.code(), "MANAGED_ENGINE_ENDPOINT_MISMATCH");
    }

    #[test]
    fn materialized_config_preserves_explicit_worker_manager_config() {
        let spec = crate::ComposeFile::parse(
            r#"
engine:
  url: ws://127.0.0.1:50123
  workers:
    iii-worker-manager:
      host: 0.0.0.0
      port: 60123
containers: {}
"#,
            "/srv/app/worker-compose.yaml",
        )
        .unwrap()
        .engine
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = materialize_engine_config(&spec, dir.path()).unwrap();
        let document: serde_yaml::Value =
            serde_yaml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();

        assert_eq!(
            document["workers"][0]["config"],
            serde_yaml::from_str::<serde_yaml::Value>(
                r#"
host: 0.0.0.0
port: 60123
"#,
            )
            .unwrap()
        );
    }

    #[cfg(unix)]
    #[test]
    fn materialized_config_and_directory_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join("daemon");
        let path = materialize_engine_config(&engine_spec(), &state_dir).unwrap();

        assert_eq!(
            std::fs::metadata(&state_dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[cfg(unix)]
    fn write_executable(path: &Path, contents: &str) {
        use std::{io::Write as _, os::unix::fs::PermissionsExt};

        let staging = path.with_extension("tmp");
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&staging)
            .unwrap();
        file.write_all(contents.as_bytes()).unwrap();
        file.sync_all().unwrap();
        drop(file);
        std::fs::set_permissions(&staging, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::rename(staging, path).unwrap();
    }

    #[test]
    fn engine_log_lives_in_the_project_namespace_directory() {
        assert_eq!(
            engine_log_path(Path::new("/project/.iii/compose/blue-whale")),
            Path::new("/project/.iii/compose/blue-whale/engine.log")
        );
    }

    #[test]
    fn concurrent_managed_engines_cannot_claim_the_same_project_namespace() {
        let dir = tempfile::tempdir().unwrap();
        let namespace_dir = dir.path().join("orders");
        std::fs::create_dir(&namespace_dir).unwrap();
        let start = std::sync::Arc::new(std::sync::Barrier::new(3));
        let attempted = std::sync::Arc::new(std::sync::Barrier::new(3));
        let mut handles = Vec::new();

        for _ in 0..2 {
            let namespace_dir = namespace_dir.clone();
            let start = std::sync::Arc::clone(&start);
            let attempted = std::sync::Arc::clone(&attempted);
            handles.push(std::thread::spawn(move || {
                start.wait();
                let claim = NamespaceLock::acquire(&namespace_dir, "orders");
                let acquired = claim.is_ok();
                attempted.wait();
                acquired
            }));
        }

        start.wait();
        attempted.wait();
        let acquired = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .filter(|acquired| *acquired)
            .count();

        assert_eq!(acquired, 1);
        NamespaceLock::acquire(&namespace_dir, "orders")
            .expect("the namespace lock must be released with its owner");
    }

    #[cfg(unix)]
    #[test]
    fn follow_command_quotes_paths_for_a_shell() {
        assert_eq!(
            follow_command(Path::new("/tmp/my project's/engine.log")),
            "tail -f '/tmp/my project'\"'\"'s/engine.log'"
        );
    }

    #[cfg(windows)]
    #[test]
    fn follow_command_quotes_paths_for_powershell() {
        assert_eq!(
            follow_command(Path::new("C:\\my project's\\engine.log")),
            "Get-Content -LiteralPath 'C:\\my project''s\\engine.log' -Wait"
        );
    }

    #[test]
    fn log_tail_is_bounded_to_the_last_five_non_empty_lines() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("engine.log");
        std::fs::write(&log, "one\n\ntwo\nthree\nfour\nfive\nsix\n").unwrap();

        assert_eq!(
            log_tail(&log).as_deref(),
            Some("  two\n  three\n  four\n  five\n  six")
        );
    }

    #[test]
    fn log_tail_reads_a_bounded_suffix() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("engine.log");
        let mut contents = "x".repeat(70 * 1024);
        contents.push_str("\none\ntwo\nthree\nfour\nfive\nsix\n");
        std::fs::write(&log, contents).unwrap();

        assert_eq!(
            log_tail(&log).as_deref(),
            Some("  two\n  three\n  four\n  five\n  six")
        );
    }

    #[test]
    fn log_tail_keeps_a_bounded_fragment_of_one_long_line() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("engine.log");
        std::fs::write(&log, "x".repeat(70 * 1024)).unwrap();
        let expected = format!("  {}", "x".repeat(240));

        assert_eq!(log_tail(&log).as_deref(), Some(expected.as_str()));
    }

    #[test]
    fn log_tail_strips_terminal_escape_sequences_from_existing_logs() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("engine.log");
        std::fs::write(
            &log,
            "\u{1b}[31mred\u{1b}[0m\n\u{1b}]2;forged title\u{7}visible\n",
        )
        .unwrap();

        assert_eq!(log_tail(&log).as_deref(), Some("  red\n  visible"));
    }

    #[test]
    fn terminal_sanitizer_tracks_escape_and_utf8_state_across_chunks() {
        let mut sanitizer = TerminalSanitizer::default();
        let mut clean = Vec::new();
        for chunk in [
            b"\x1b[3".as_slice(),
            b"1mred\x1b".as_slice(),
            b"[0m \x1b]2;forged".as_slice(),
            b" title\x1b".as_slice(),
            b"\\visible caf\xc3".as_slice(),
            b"\xa9\n".as_slice(),
        ] {
            clean.extend(sanitizer.sanitize(chunk));
        }

        assert_eq!(String::from_utf8(clean).unwrap(), "red visible café\n");
    }

    #[test]
    fn terminal_sanitizer_reprocesses_invalid_utf8_and_strips_bidi_controls() {
        let clean = sanitize_terminal_output(
            b"\xc3\x1b[31mred\x1b[0m \xe2\x80\xaespoof\xe2\x80\xa8forged\xe2\x80\xa9line\n",
        );

        assert_eq!(String::from_utf8(clean).unwrap(), "red spoofforgedline\n");
    }

    #[test]
    fn rotating_log_clamps_a_zero_size_limit() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("engine.log");

        let log = RotatingLog::open(&log, 0, 0).unwrap();

        assert_eq!(log.max_bytes, 1);
    }

    #[cfg(unix)]
    #[test]
    fn opening_an_existing_log_hardens_its_permissions() {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("engine.log");
        std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .mode(0o644)
            .open(&log)
            .unwrap();
        std::fs::set_permissions(&log, std::fs::Permissions::from_mode(0o644)).unwrap();

        let _log = RotatingLog::open(&log, ENGINE_LOG_MAX_BYTES, ENGINE_LOG_ARCHIVES).unwrap();

        assert_eq!(
            std::fs::metadata(log).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn engine_receives_config_and_captures_both_output_streams() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("fake iii");
        let config = dir.path().join("project config.yaml");
        let log = dir.path().join("engine.log");
        write_executable(
            &script,
            "#!/bin/sh\nprintf 'args:%s\\n' \"$*\"\nprintf '\\033[31mengine stdout\\033[0m\\n'\nprintf '\\033]2;forged title\\007engine stderr\\n' >&2\nexit 7\n",
        );

        let engine = ManagedEngine::spawn_with_paths(&script, &config, &log, "test")
            .await
            .unwrap();
        let status = tokio::time::timeout(Duration::from_secs(5), engine.wait())
            .await
            .expect("fake engine should exit");

        assert_eq!(status.code(), Some(7));
        let output = std::fs::read_to_string(&log).unwrap();
        assert!(
            output.contains(&format!("args:--config {}", config.display())),
            "config argument missing from {output:?}"
        );
        assert!(
            output.contains("engine stderr"),
            "stderr missing from {output:?}"
        );
        assert!(
            output.contains("engine stdout"),
            "stdout missing from {output:?}"
        );
        assert!(
            !output.contains('\u{1b}') && !output.contains("forged title"),
            "terminal escape sequence was persisted in {output:?}"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn failed_start_removes_the_materialized_engine_config() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("fake-iii");
        let namespace_dir = dir.path().join("daemon");
        let config = materialize_engine_config(&engine_spec(), &namespace_dir).unwrap();
        let log = namespace_dir.join("engine.log");
        write_executable(&script, "#!/bin/sh\nexit 0\n");
        std::fs::create_dir(&log).unwrap();

        let error =
            match ManagedEngine::spawn_with_materialized_config(&script, &config, &log, "test")
                .await
            {
                Ok(_) => panic!("a directory cannot be opened as the engine log"),
                Err(error) => error,
            };

        assert_eq!(error.code(), "IO_ERROR");
        assert!(
            !config.exists(),
            "a failed managed-engine start left its generated config behind"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn engine_log_rotates_before_it_can_grow_without_bound() {
        const EXPECTED_LIMIT: u64 = 10 * 1024 * 1024;

        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("noisy-iii");
        let config = dir.path().join("config.yaml");
        let log = dir.path().join("engine.log");
        write_executable(
            &script,
            "#!/bin/sh\ndd if=/dev/zero bs=1048576 count=11 2>/dev/null | tr '\\000' x\n",
        );

        let engine = ManagedEngine::spawn_with_paths(&script, &config, &log, "test")
            .await
            .unwrap();
        let status = tokio::time::timeout(Duration::from_secs(10), engine.wait())
            .await
            .expect("noisy engine should exit");

        assert!(status.success());
        assert!(
            std::fs::metadata(&log).unwrap().len() <= EXPECTED_LIMIT,
            "current log exceeded its size limit"
        );
        let archive = log.with_file_name("engine.log.1");
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while !archive.exists() && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(
            archive.exists(),
            "the previous log segment was not archived"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn stopping_the_engine_stops_its_process_group() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("fake-iii");
        let config = dir.path().join("config.yaml");
        let log = dir.path().join("engine.log");
        write_executable(
            &script,
            "#!/bin/sh\ntrap 'exit 0' TERM INT\nwhile :; do sleep 1; done\n",
        );

        let engine = ManagedEngine::spawn_with_paths(&script, &config, &log, "test")
            .await
            .unwrap();
        let pid = engine.pid();
        assert!(crate::process::is_running(pid));

        let _status = engine.stop(Duration::from_secs(2)).await;

        assert!(!crate::process::is_running(pid));
    }

    #[test]
    fn env_fingerprint_depends_only_on_the_referenced_variables() {
        assert_eq!(env_fingerprint("nothing to expand"), env_fingerprint(""));
        assert_eq!(
            env_fingerprint("a: ${PATH}\nb: ${PATH:fallback}\nc: ${PATH}"),
            env_fingerprint("${PATH}")
        );
        assert_eq!(
            env_fingerprint("${PATH} ${III_SURELY_UNSET_A}"),
            env_fingerprint("${III_SURELY_UNSET_A} ${PATH}")
        );
        assert_ne!(env_fingerprint("${PATH}"), env_fingerprint(""));
        assert_ne!(
            env_fingerprint("${III_SURELY_UNSET_A}"),
            env_fingerprint("")
        );
        assert_ne!(
            env_fingerprint("${III_SURELY_UNSET_A}"),
            env_fingerprint("${III_SURELY_UNSET_B}")
        );
    }

    /// A small, stable file to stand in for the executable where a test does
    /// not care which binary the engine came from: the identity hashes the
    /// file, and the test binary itself is hundreds of megabytes in debug.
    fn stand_in_executable() -> PathBuf {
        PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
    }

    /// The identity a start of `spec` from `executable` would record now.
    fn launch_identity(executable: &Path, spec: &crate::config::EngineSpec) -> LaunchIdentity {
        LaunchIdentity::current(executable, spec, &render_engine_config(spec).unwrap()).unwrap()
    }

    /// A shutdown that is never requested.
    fn never_shutdown() -> ShutdownSignal {
        ShutdownSignal::from_receiver(tokio::sync::watch::channel(false).1)
    }

    /// Polls until `path` exists or `timeout` elapses.
    #[cfg(unix)]
    async fn wait_for_path(path: &Path, timeout: Duration) -> bool {
        let deadline = tokio::time::Instant::now() + timeout;
        while tokio::time::Instant::now() < deadline {
            if path.exists() {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        false
    }

    /// A record for a process this test spawned, as `start` would write it.
    fn record_for(
        process: &Supervised,
        compose_path: &Path,
        launch: LaunchIdentity,
        status: ChildStatus,
    ) -> EngineRecord {
        EngineRecord {
            compose_path: compose_path.to_path_buf(),
            launch,
            process: ChildRecord::from_supervised(process, status),
        }
    }

    /// A record for any pid and birth identity, with a stand-in launch identity.
    fn record_of_pid(
        pid: u32,
        birth: crate::process::BirthIdentity,
        compose_path: &Path,
        status: ChildStatus,
    ) -> EngineRecord {
        EngineRecord {
            compose_path: compose_path.to_path_buf(),
            launch: launch_identity(&stand_in_executable(), &engine_spec()),
            process: ChildRecord::new(pid, birth, status),
        }
    }

    /// A spec on a free port with an inferred worker manager, so the listener
    /// probe passes and nothing else has to be running.
    fn spec_on_a_free_port() -> (crate::config::EngineSpec, u16) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let mut spec = engine_spec();
        spec.url = format!("ws://127.0.0.1:{port}");
        spec.workers.remove("iii-worker-manager");
        (spec, port)
    }

    /// A project directory with a compose file, and the namespace directory
    /// `start` derives for it.
    fn project(dir: &Path, namespace: &str) -> (PathBuf, PathBuf) {
        let compose_path = dir.join("worker-compose.yaml");
        std::fs::write(&compose_path, "containers: {}\n").unwrap();
        let namespace_dir = StateStore::for_project(namespace, &compose_path)
            .unwrap()
            .dir()
            .to_path_buf();
        (compose_path, namespace_dir)
    }

    /// A stand-in engine that loops until signalled; with `ignores_term` it
    /// survives SIGTERM and only SIGKILL ends it.
    #[cfg(unix)]
    fn survivor_script(dir: &Path, name: &str, ignores_term: bool) -> PathBuf {
        let script = dir.join(name);
        let trap = if ignores_term {
            "trap '' TERM INT"
        } else {
            "trap 'exit 0' TERM INT"
        };
        write_executable(
            &script,
            &format!("#!/bin/sh\n{trap}\nwhile :; do sleep 1; done\n"),
        );
        script
    }

    /// An engine some earlier compose started and then left behind: the
    /// process is up and only the record and the materialized config point at
    /// it. Dropping the `ManagedEngine` kills nothing (there is no
    /// kill-on-drop), which is the part that matters here. Unlike a real
    /// crash this test process still reaps it; the e2e covers reparenting.
    #[cfg(unix)]
    async fn leave_survivor(
        script: &Path,
        namespace_dir: &Path,
        compose_path: &Path,
        config: &str,
        launch: LaunchIdentity,
        namespace: &str,
    ) -> u32 {
        let config_path = write_engine_config(config, namespace_dir).unwrap();
        let earlier = ManagedEngine::spawn_with_paths(
            script,
            &config_path,
            &engine_log_path(namespace_dir),
            namespace,
        )
        .await
        .unwrap();
        save_engine_record(
            namespace_dir,
            &record_for(
                &earlier.process,
                compose_path,
                launch,
                ChildStatus::Starting,
            ),
        )
        .unwrap();
        earlier.pid()
    }

    #[test]
    fn engine_record_round_trips_and_can_be_marked_stopped() {
        let dir = tempfile::tempdir().unwrap();
        let namespace_dir = dir.path().join("daemon");
        assert!(load_engine_record(&namespace_dir).unwrap().is_none());

        let record = record_of_pid(
            4242,
            crate::process::BirthIdentity::StartTime(7),
            Path::new("/srv/app/worker-compose.yaml"),
            ChildStatus::Starting,
        );
        save_engine_record(&namespace_dir, &record).unwrap();
        assert_eq!(
            load_engine_record(&namespace_dir).unwrap(),
            Some(record.clone())
        );

        mark_engine_record_stopped(&namespace_dir).unwrap();
        let stopped = load_engine_record(&namespace_dir).unwrap().unwrap();
        assert_eq!(stopped.process.status, ChildStatus::Stopped);
        assert_eq!(stopped.process.pid, 4242);
        assert_eq!(stopped.process.birth, record.process.birth);
        assert_eq!(stopped.compose_path, record.compose_path);
        assert_eq!(stopped.launch, record.launch);
    }

    #[test]
    fn a_corrupt_engine_record_is_an_error_not_a_silent_reset() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(ENGINE_RECORD_FILE), "{not json").unwrap();

        let error = load_engine_record(dir.path()).expect_err("a corrupt record must surface");

        assert!(
            matches!(error, ComposeError::InvalidState { .. }),
            "{error}"
        );
    }

    #[test]
    fn nothing_recorded_or_a_deliberately_stopped_engine_is_no_previous_engine() {
        let dir = tempfile::tempdir().unwrap();
        let namespace_dir = dir.path().join("daemon");
        let compose_path = Path::new("/srv/app/worker-compose.yaml");
        let spec = engine_spec();
        let launch = launch_identity(&stand_in_executable(), &spec);
        let rendered = render_engine_config(&spec).unwrap();
        assert!(matches!(
            reconcile_previous_engine(&namespace_dir, compose_path, &launch, &rendered).unwrap(),
            PreviousEngine::None
        ));

        // A live, verifiable pid (this test process) that a compose stopped on
        // purpose is neither adopted nor reported.
        let me = std::process::id();
        save_engine_record(
            &namespace_dir,
            &record_of_pid(
                me,
                crate::process::birth_identity(me),
                compose_path,
                ChildStatus::Stopped,
            ),
        )
        .unwrap();
        assert!(matches!(
            reconcile_previous_engine(&namespace_dir, compose_path, &launch, &rendered).unwrap(),
            PreviousEngine::None
        ));
    }

    #[test]
    fn an_unverifiable_record_is_reported_and_never_adopted() {
        let dir = tempfile::tempdir().unwrap();
        let namespace_dir = dir.path().join("daemon");
        let compose_path = Path::new("/srv/app/worker-compose.yaml");
        let spec = engine_spec();
        let launch = launch_identity(&stand_in_executable(), &spec);
        let rendered = render_engine_config(&spec).unwrap();
        // A live pid whose recorded identity does not match it: a recycled pid.
        let me = std::process::id();
        save_engine_record(
            &namespace_dir,
            &record_of_pid(
                me,
                crate::process::BirthIdentity::StartTime(1),
                compose_path,
                ChildStatus::Starting,
            ),
        )
        .unwrap();

        match reconcile_previous_engine(&namespace_dir, compose_path, &launch, &rendered).unwrap() {
            PreviousEngine::Unverifiable { pid } => assert_eq!(pid, me),
            _ => panic!("a pid with another birth identity must not be adopted"),
        }
    }

    #[test]
    fn another_files_record_is_refused_unless_that_file_stopped_its_engine() {
        let dir = tempfile::tempdir().unwrap();
        let namespace_dir = dir.path().join("daemon");
        let mine = Path::new("/srv/app/a.yaml");
        let theirs = Path::new("/srv/app/b.yaml");
        let spec = engine_spec();
        let launch = launch_identity(&stand_in_executable(), &spec);
        let rendered = render_engine_config(&spec).unwrap();
        let me = std::process::id();

        // Their engine is up (this process stands in for it) and verifiable.
        save_engine_record(
            &namespace_dir,
            &record_of_pid(
                me,
                crate::process::birth_identity(me),
                theirs,
                ChildStatus::Starting,
            ),
        )
        .unwrap();
        let error = reconcile_previous_engine(&namespace_dir, mine, &launch, &rendered)
            .expect_err("another file's live engine must not be adopted or stopped");
        assert!(
            matches!(error, ComposeError::InvalidState { .. }),
            "{error}"
        );
        assert!(error.to_string().contains("b.yaml"), "{error}");

        // Unverifiable but still theirs: refused as well.
        save_engine_record(
            &namespace_dir,
            &record_of_pid(
                me,
                crate::process::BirthIdentity::StartTime(1),
                theirs,
                ChildStatus::Starting,
            ),
        )
        .unwrap();
        reconcile_previous_engine(&namespace_dir, mine, &launch, &rendered)
            .expect_err("another file's unverifiable pid must not be claimed");

        // They stopped it on purpose: the namespace is free to take.
        save_engine_record(
            &namespace_dir,
            &record_of_pid(
                me,
                crate::process::birth_identity(me),
                theirs,
                ChildStatus::Stopped,
            ),
        )
        .unwrap();
        assert!(matches!(
            reconcile_previous_engine(&namespace_dir, mine, &launch, &rendered).unwrap(),
            PreviousEngine::None
        ));
    }

    #[test]
    fn occupied_listener_error_says_how_to_find_the_holder() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut spec = engine_spec();
        spec.url = format!("ws://127.0.0.1:{port}");
        spec.workers.remove("iii-worker-manager");

        let text = ensure_listener_available(&spec).unwrap_err().to_string();

        assert!(text.contains(&format!("127.0.0.1:{port}")), "{text}");
        assert!(text.contains("iii compose --up"), "{text}");
        assert!(text.contains(&format!(":{port}")), "{text}");
        assert!(text.contains("engine.url"), "{text}");
    }

    #[test]
    fn listener_hint_blames_a_holder_only_for_an_occupied_port() {
        use std::io::{Error, ErrorKind};

        let endpoint = EngineEndpoint {
            worker_host: "127.0.0.1".to_string(),
            port: 6102,
        };

        let busy = listener_hint(&endpoint, &Error::from(ErrorKind::AddrInUse));
        assert!(
            busy.contains("6102") && busy.contains("iii compose --up"),
            "{busy}"
        );

        let denied = listener_hint(&endpoint, &Error::from(ErrorKind::PermissionDenied));
        assert!(denied.contains("6102"), "{denied}");
        assert!(
            !denied.contains("lsof") && !denied.contains("netstat"),
            "{denied}"
        );

        let unavailable = listener_hint(&endpoint, &Error::from(ErrorKind::AddrNotAvailable));
        assert!(unavailable.contains("127.0.0.1"), "{unavailable}");
        assert!(!unavailable.contains("lsof"), "{unavailable}");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_record_of_an_engine_that_exited_is_no_previous_engine() {
        let dir = tempfile::tempdir().unwrap();
        let namespace_dir = dir.path().join("daemon");
        let compose_path = dir.path().join("worker-compose.yaml");
        let spec = engine_spec();
        let launch = launch_identity(&stand_in_executable(), &spec);
        let rendered = render_engine_config(&spec).unwrap();
        let script = dir.path().join("fake-iii");
        write_executable(&script, "#!/bin/sh\nexit 0\n");
        let config = dir.path().join("config.yaml");
        let log = dir.path().join("engine.log");

        let engine = ManagedEngine::spawn_with_paths(&script, &config, &log, "test")
            .await
            .unwrap();
        save_engine_record(
            &namespace_dir,
            &record_for(
                &engine.process,
                &compose_path,
                launch.clone(),
                ChildStatus::Starting,
            ),
        )
        .unwrap();
        engine.wait().await;

        assert!(matches!(
            reconcile_previous_engine(&namespace_dir, &compose_path, &launch, &rendered).unwrap(),
            PreviousEngine::None
        ));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn start_adopts_this_files_surviving_engine_when_it_is_interchangeable() {
        let dir = tempfile::tempdir().unwrap();
        let (compose_path, namespace_dir) = project(dir.path(), "adopt");
        let (spec, _port) = spec_on_a_free_port();
        let rendered = render_engine_config(&spec).unwrap();
        let script = survivor_script(dir.path(), "fake-iii", false);
        let pid = leave_survivor(
            &script,
            &namespace_dir,
            &compose_path,
            &rendered,
            launch_identity(&script, &spec),
            "adopt",
        )
        .await;
        assert!(crate::process::is_running(pid));

        let engine = ManagedEngine::start_with(
            &script,
            &spec,
            "adopt",
            &compose_path,
            &never_shutdown(),
            Duration::from_secs(2),
        )
        .await
        .unwrap()
        .expect("an engine is owned");

        assert!(engine.is_adopted());
        assert_eq!(engine.pid(), pid);
        assert_eq!(
            load_engine_record(&namespace_dir)
                .unwrap()
                .unwrap()
                .process
                .pid,
            pid
        );

        engine.stop(Duration::from_secs(2)).await;

        assert!(!crate::process::is_running(pid));
        assert!(
            !namespace_dir.join(ENGINE_CONFIG_FILE).exists(),
            "the adopting compose owns the materialized config"
        );
        assert_eq!(
            load_engine_record(&namespace_dir)
                .unwrap()
                .unwrap()
                .process
                .status,
            ChildStatus::Stopped
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn start_replaces_this_files_surviving_engine_when_its_config_differs() {
        let dir = tempfile::tempdir().unwrap();
        let (compose_path, namespace_dir) = project(dir.path(), "replace");
        let (spec, _port) = spec_on_a_free_port();
        let rendered = render_engine_config(&spec).unwrap();
        let script = survivor_script(dir.path(), "fake-iii", false);
        // Started from an engine section that has since changed.
        let old = leave_survivor(
            &script,
            &namespace_dir,
            &compose_path,
            "workers: []\n",
            launch_identity(&script, &spec),
            "replace",
        )
        .await;

        let engine = ManagedEngine::start_with(
            &script,
            &spec,
            "replace",
            &compose_path,
            &never_shutdown(),
            Duration::from_secs(2),
        )
        .await
        .unwrap()
        .expect("an engine is owned");

        assert!(!engine.is_adopted());
        assert_ne!(engine.pid(), old);
        assert!(
            !crate::process::is_running(old),
            "the outdated engine must be stopped"
        );
        assert!(crate::process::is_running(engine.pid()));
        assert_eq!(
            std::fs::read_to_string(namespace_dir.join(ENGINE_CONFIG_FILE)).unwrap(),
            rendered
        );
        assert_eq!(
            load_engine_record(&namespace_dir)
                .unwrap()
                .unwrap()
                .process
                .pid,
            engine.pid()
        );

        engine.stop(Duration::from_secs(2)).await;
        assert!(!crate::process::is_running(engine.pid()));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn start_replaces_this_files_surviving_engine_when_its_launch_identity_differs() {
        let dir = tempfile::tempdir().unwrap();
        let (compose_path, namespace_dir) = project(dir.path(), "relaunch");
        let (spec, _port) = spec_on_a_free_port();
        let rendered = render_engine_config(&spec).unwrap();
        let script = survivor_script(dir.path(), "fake-iii", false);
        // Same YAML, but started from another working directory: `./config`
        // in engine.workers does not mean the same place any more.
        let mut launch = launch_identity(&script, &spec);
        launch.cwd = PathBuf::from("/somewhere/else");
        let old = leave_survivor(
            &script,
            &namespace_dir,
            &compose_path,
            &rendered,
            launch,
            "relaunch",
        )
        .await;

        let engine = ManagedEngine::start_with(
            &script,
            &spec,
            "relaunch",
            &compose_path,
            &never_shutdown(),
            Duration::from_secs(2),
        )
        .await
        .unwrap()
        .expect("an engine is owned");

        assert!(!engine.is_adopted());
        assert_ne!(engine.pid(), old);
        assert!(!crate::process::is_running(old));

        engine.stop(Duration::from_secs(2)).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn start_refuses_another_files_surviving_engine() {
        let dir = tempfile::tempdir().unwrap();
        let (compose_path, namespace_dir) = project(dir.path(), "shared");
        let other = dir.path().join("other.yaml");
        let (spec, _port) = spec_on_a_free_port();
        let rendered = render_engine_config(&spec).unwrap();
        let script = survivor_script(dir.path(), "fake-iii", false);
        let pid = leave_survivor(
            &script,
            &namespace_dir,
            &other,
            &rendered,
            launch_identity(&script, &spec),
            "shared",
        )
        .await;

        let error = ManagedEngine::start_with(
            &script,
            &spec,
            "shared",
            &compose_path,
            &never_shutdown(),
            Duration::from_secs(2),
        )
        .await
        .err()
        .expect("another file's engine must be refused");

        assert!(
            matches!(error, ComposeError::InvalidState { .. }),
            "{error}"
        );
        assert!(error.to_string().contains("other.yaml"), "{error}");
        assert!(
            crate::process::is_running(pid),
            "the other file's engine must not be signalled"
        );

        Supervised::adopt(pid, &crate::process::birth_identity(pid))
            .unwrap()
            .stop(Duration::from_secs(2))
            .await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn start_names_an_unverifiable_pid_when_the_port_is_taken() {
        let dir = tempfile::tempdir().unwrap();
        let (compose_path, namespace_dir) = project(dir.path(), "busy");
        let (spec, port) = spec_on_a_free_port();
        let me = std::process::id();
        save_engine_record(
            &namespace_dir,
            &record_of_pid(
                me,
                crate::process::BirthIdentity::StartTime(1),
                &compose_path,
                ChildStatus::Starting,
            ),
        )
        .unwrap();
        let _holder = std::net::TcpListener::bind(("127.0.0.1", port)).unwrap();
        let script = survivor_script(dir.path(), "fake-iii", false);

        let error = ManagedEngine::start_with(
            &script,
            &spec,
            "busy",
            &compose_path,
            &never_shutdown(),
            Duration::from_secs(2),
        )
        .await
        .err()
        .expect("an occupied port must still fail");

        assert_eq!(error.code(), "MANAGED_ENGINE_LISTENER_UNAVAILABLE");
        let text = error.to_string();
        assert!(text.contains(&format!("pid {me}")), "{text}");
        assert!(text.contains("not signalled"), "{text}");
        assert!(text.contains("lsof") || text.contains("netstat"), "{text}");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn start_replaces_this_files_surviving_engine_when_the_executable_changed() {
        let dir = tempfile::tempdir().unwrap();
        let (compose_path, namespace_dir) = project(dir.path(), "rebuilt");
        let (spec, _port) = spec_on_a_free_port();
        let rendered = render_engine_config(&spec).unwrap();
        let script = survivor_script(dir.path(), "fake-iii", false);
        // Same YAML, same directory, but the binary that started it is not
        // the one that would start it now: `iii update` happened in between.
        let mut launch = launch_identity(&script, &spec);
        launch.executable.len += 1;
        let old = leave_survivor(
            &script,
            &namespace_dir,
            &compose_path,
            &rendered,
            launch,
            "rebuilt",
        )
        .await;

        let engine = ManagedEngine::start_with(
            &script,
            &spec,
            "rebuilt",
            &compose_path,
            &never_shutdown(),
            Duration::from_secs(2),
        )
        .await
        .unwrap()
        .expect("an engine is owned");

        assert!(!engine.is_adopted());
        assert_ne!(engine.pid(), old);
        assert!(!crate::process::is_running(old));

        engine.stop(Duration::from_secs(2)).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn start_replaces_this_files_surviving_engine_when_the_executable_was_rebuilt_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let (compose_path, namespace_dir) = project(dir.path(), "rebuilt-in-place");
        let (spec, _port) = spec_on_a_free_port();
        let rendered = render_engine_config(&spec).unwrap();
        let script = dir.path().join("fake-iii");
        // Two builds of the same size, installed at the same path with the
        // same mtime: only their contents tell them apart.
        let build = |tag: &str| {
            format!("#!/bin/sh\ntrap 'exit 0' TERM INT\n# build {tag}\nwhile :; do sleep 1; done\n")
        };
        write_executable(&script, &build("one"));
        let before = LaunchIdentity::current(&script, &spec, &rendered).unwrap();
        let modified = std::fs::metadata(&script).unwrap().modified().unwrap();
        write_executable(&script, &build("two"));
        std::fs::File::options()
            .write(true)
            .open(&script)
            .unwrap()
            .set_modified(modified)
            .unwrap();
        let after = LaunchIdentity::current(&script, &spec, &rendered).unwrap();
        assert_eq!(after.executable.path, before.executable.path);
        assert_eq!(after.executable.len, before.executable.len);
        assert_eq!(after.executable.modified, before.executable.modified);
        assert_ne!(after.executable.sha256, before.executable.sha256);

        // The record says the survivor came from build one; build two is what
        // would start now. (The stand-in itself runs whatever the file holds
        // at spawn; only the recorded identity matters to the decision.)
        let old = leave_survivor(
            &script,
            &namespace_dir,
            &compose_path,
            &rendered,
            before,
            "rebuilt-in-place",
        )
        .await;

        let engine = ManagedEngine::start_with(
            &script,
            &spec,
            "rebuilt-in-place",
            &compose_path,
            &never_shutdown(),
            Duration::from_secs(2),
        )
        .await
        .unwrap()
        .expect("an engine is owned");

        assert!(!engine.is_adopted(), "another build must not be adopted");
        assert_ne!(engine.pid(), old);
        assert!(!crate::process::is_running(old));

        engine.stop(Duration::from_secs(2)).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_shutdown_during_a_replacement_finishes_the_stop_and_holds_the_lock_until_then() {
        let dir = tempfile::tempdir().unwrap();
        let (compose_path, namespace_dir) = project(dir.path(), "cancel");
        let (spec, _port) = spec_on_a_free_port();
        // A survivor that ignores SIGTERM, and says when its trap is armed
        // and when it was signalled.
        let armed = dir.path().join("armed");
        let signalled = dir.path().join("signalled");
        let script = dir.path().join("stubborn-iii");
        write_executable(
            &script,
            &format!(
                "#!/bin/sh\ntrap 'touch {}' TERM INT\ntouch {}\nwhile :; do sleep 1; done\n",
                signalled.display(),
                armed.display()
            ),
        );
        let old = leave_survivor(
            &script,
            &namespace_dir,
            &compose_path,
            "workers: []\n",
            launch_identity(&script, &spec),
            "cancel",
        )
        .await;
        assert!(
            wait_for_path(&armed, Duration::from_secs(5)).await,
            "the survivor never armed its trap"
        );

        let (request, receiver) = tokio::sync::watch::channel(false);
        let shutdown = ShutdownSignal::from_receiver(receiver);
        let start = ManagedEngine::start_with(
            &script,
            &spec,
            "cancel",
            &compose_path,
            &shutdown,
            Duration::from_secs(2),
        );
        let driver = async {
            assert!(
                wait_for_path(&signalled, Duration::from_secs(5)).await,
                "the survivor was never asked to stop"
            );
            // Mid-stop the namespace is still this start's: nobody else can
            // claim it and read the record in the meantime.
            let lock_dir = namespace_dir.clone();
            let taken =
                tokio::task::spawn_blocking(move || NamespaceLock::acquire(&lock_dir, "cancel"))
                    .await
                    .unwrap();
            assert!(
                matches!(taken, Err(ComposeError::DaemonNamespaceTaken { .. })),
                "the lock must be held for the whole stop"
            );
            // The shutdown request lands while the survivor still ignores
            // SIGTERM: the start must escalate to SIGKILL before it returns.
            request.send(true).unwrap();
        };

        let (outcome, ()) = tokio::join!(start, driver);

        assert!(
            outcome.unwrap().is_none(),
            "a shutdown during the replacement ends the start without an engine"
        );
        assert!(
            !crate::process::is_running(old),
            "the replaced engine must be gone by the time start returns"
        );
        let lock_dir = namespace_dir.clone();
        tokio::task::spawn_blocking(move || NamespaceLock::acquire(&lock_dir, "cancel"))
            .await
            .unwrap()
            .expect("the lock is released once the start has returned");
    }

    #[test]
    fn the_digest_cache_exists_only_where_the_kernel_keeps_a_change_identity() {
        assert_eq!(
            digest_cache_dir().is_some(),
            cfg!(unix) && dirs::cache_dir().is_some()
        );
    }

    #[cfg(unix)]
    #[test]
    fn executable_digests_are_cached_by_file_identity_and_invalidated_by_a_rewrite() {
        let dir = tempfile::tempdir().unwrap();
        let cache_dir = dir.path().join("cache");
        let binary = dir.path().join("iii");
        let content = |byte: u8| vec![byte; DIGEST_CACHE_MIN_LEN as usize + 1];
        std::fs::write(&binary, content(1)).unwrap();
        let modified = std::fs::metadata(&binary).unwrap().modified().unwrap();

        let (_, first) = executable_digest(&binary, Some(&cache_dir)).unwrap();
        assert!(
            cache_dir.join(DIGEST_CACHE_FILE).exists(),
            "a file this size is worth remembering"
        );

        // Same file, same stamp: served from the cache. Tampering with the
        // cached value is how the test can tell.
        let planted = "f".repeat(64);
        let mut cache = DigestCache::load(&cache_dir);
        cache.entries.get_mut(&binary).unwrap().sha256 = planted.clone();
        cache.store(&cache_dir).unwrap();
        assert_eq!(
            executable_digest(&binary, Some(&cache_dir)).unwrap().1,
            planted
        );

        // A cached value that is not a digest is not trusted.
        cache.entries.get_mut(&binary).unwrap().sha256 = "not a digest".to_string();
        cache.store(&cache_dir).unwrap();
        assert_eq!(
            executable_digest(&binary, Some(&cache_dir)).unwrap().1,
            first
        );

        // Rewritten in place with the same size and the same mtime: the
        // change time still moves, and the entry no longer applies.
        std::fs::write(&binary, content(2)).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&binary)
            .unwrap()
            .set_modified(modified)
            .unwrap();
        let (_, second) = executable_digest(&binary, Some(&cache_dir)).unwrap();
        assert_ne!(second, first);
        assert_ne!(second, planted);

        // Small files never touch the cache.
        let script = dir.path().join("tiny");
        std::fs::write(&script, b"#!/bin/sh\n").unwrap();
        executable_digest(&script, Some(&cache_dir)).unwrap();
        assert!(!DigestCache::load(&cache_dir).entries.contains_key(&script));
    }

    #[test]
    fn referenced_variables_follow_the_engines_grammar() {
        assert_eq!(
            referenced_variables("a: ${A}\nb: ${B:def}\nc: ${A}"),
            vec!["A", "B"]
        );
        // The outer reference has an empty name and is no reference at all;
        // the inner one is what the engine expands.
        assert_eq!(referenced_variables("${:${TOKEN}}"), vec!["TOKEN"]);
        // A default runs to the first `}`; an unterminated reference is none.
        assert_eq!(referenced_variables("${A:x}y} ${B"), vec!["A"]);
        // `[^}:]+` admits anything else, `$` and `{` included: so does this.
        assert_eq!(referenced_variables("${A ${B}"), vec!["A ${B"]);
        assert!(referenced_variables("$A ${} ${:x} plain").is_empty());
    }

    #[test]
    fn fingerprint_distinguishes_pairs_a_flat_encoding_would_confuse() {
        let pairs = |list: &[(&str, Option<&str>)]| {
            fingerprint_of(
                list.iter()
                    .map(|(name, value)| (name.to_string(), value.map(str::to_string))),
            )
        };

        assert_ne!(
            pairs(&[("A", Some("x\nB=y")), ("B", Some("z"))]),
            pairs(&[("A", Some("x")), ("B", Some("y\nB=z"))])
        );
        assert_ne!(pairs(&[("A", None)]), pairs(&[("A", Some(""))]));
        assert_ne!(
            pairs(&[("A", Some("1")), ("B", Some("2"))]),
            pairs(&[("A", Some("12")), ("B", Some(""))])
        );
        assert_eq!(pairs(&[("A", Some("1"))]), pairs(&[("A", Some("1"))]));
    }

    #[test]
    fn the_engines_direct_environment_is_part_of_the_identity() {
        assert!(engine_reads_directly("III_NAMESPACE_GRACE_MS"));
        assert!(engine_reads_directly("III_TELEMETRY_ENABLED"));
        assert!(engine_reads_directly("OTEL_EXPORTER_OTLP_ENDPOINT"));
        assert!(engine_reads_directly("RUST_LOG"));
        assert!(engine_reads_directly("CI"));
        assert!(engine_reads_directly("HOME"));
        assert!(!engine_reads_directly("PATH"));
        assert!(!engine_reads_directly("TZ"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn stopping_a_recorded_engine_marks_its_record_stopped() {
        let dir = tempfile::tempdir().unwrap();
        let namespace_dir = dir.path().join("daemon");
        let compose_path = dir.path().join("worker-compose.yaml");
        let spec = engine_spec();
        let launch = launch_identity(&stand_in_executable(), &spec);
        let rendered = render_engine_config(&spec).unwrap();
        let script = survivor_script(dir.path(), "fake-iii", false);
        let config = namespace_dir.join(ENGINE_CONFIG_FILE);
        let log = engine_log_path(&namespace_dir);
        let mut engine = ManagedEngine::spawn_with_paths(&script, &config, &log, "test")
            .await
            .unwrap();
        engine.record_dir = Some(namespace_dir.clone());
        save_engine_record(
            &namespace_dir,
            &record_for(
                &engine.process,
                &compose_path,
                launch.clone(),
                ChildStatus::Starting,
            ),
        )
        .unwrap();

        engine.stop(Duration::from_secs(2)).await;

        assert_eq!(
            load_engine_record(&namespace_dir)
                .unwrap()
                .unwrap()
                .process
                .status,
            ChildStatus::Stopped
        );
        assert!(matches!(
            reconcile_previous_engine(&namespace_dir, &compose_path, &launch, &rendered).unwrap(),
            PreviousEngine::None
        ));
    }
}

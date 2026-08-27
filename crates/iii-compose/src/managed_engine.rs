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

use serde::Serialize;
use tokio::io::AsyncReadExt;

use crate::{
    config::{CONFIGURABLE_ENGINE_WORKERS, EngineSpec},
    error::{ComposeError, Result},
    process::{ChildOutput, DEFAULT_STOP_GRACE, Supervised, spawn_supervised_piped},
    state::StateStore,
};

/// Maximum size of the active engine log before it rolls into an archive.
const ENGINE_LOG_MAX_BYTES: u64 = 10 * 1024 * 1024;
/// Number of old engine log segments retained beside `engine.log`.
const ENGINE_LOG_ARCHIVES: usize = 3;
const ENGINE_LOCK_FILE: &str = "engine.lock";
const ENGINE_CONFIG_FILE: &str = "engine-config.yaml";
const DEFAULT_WORKER_MANAGER_HOST: &str = "0.0.0.0";
const DEFAULT_WORKER_MANAGER_PORT: u16 = 49134;

/// The engine process owned by one foreground compose invocation.
pub struct ManagedEngine {
    process: Supervised,
    logs: LogCapture,
    config_path: PathBuf,
    log_path: PathBuf,
    remove_config_on_stop: bool,
    _namespace_lock: Option<NamespaceLock>,
}

impl ManagedEngine {
    /// Starts the current `iii` executable with output detached from compose's
    /// terminal and captured below this daemon namespace's state directory.
    pub async fn start(spec: &EngineSpec, daemon_namespace: &str) -> Result<Self> {
        ensure_listener_available(spec)?;
        let executable =
            std::env::current_exe().map_err(|err| ComposeError::EngineSpawnFailed {
                message: format!("could not locate the current iii executable: {err}"),
            })?;
        let state_root = StateStore::root()?;
        let namespace_dir = state_root.join(daemon_namespace);
        let lock_dir = namespace_dir.clone();
        let namespace = daemon_namespace.to_string();
        let namespace_lock =
            tokio::task::spawn_blocking(move || NamespaceLock::acquire(&lock_dir, &namespace))
                .await
                .map_err(|source| ComposeError::EngineSpawnFailed {
                    message: format!("could not claim the managed engine namespace: {source}"),
                })??;
        let config_path = materialize_engine_config(spec, &namespace_dir)?;
        let log_path = engine_log_path(&state_root, daemon_namespace);
        let mut engine =
            Self::spawn_with_materialized_config(&executable, &config_path, &log_path).await?;
        engine._namespace_lock = Some(namespace_lock);
        Ok(engine)
    }

    async fn spawn_with_materialized_config(
        executable: &Path,
        config_path: &Path,
        log_path: &Path,
    ) -> Result<Self> {
        match Self::spawn_with_paths(executable, config_path, log_path).await {
            Ok(mut engine) => {
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

fn materialize_engine_config(spec: &EngineSpec, namespace_dir: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(namespace_dir).map_err(|source| ComposeError::Io {
        path: namespace_dir.to_path_buf(),
        source,
    })?;
    owner_only(namespace_dir).map_err(|source| ComposeError::Io {
        path: namespace_dir.to_path_buf(),
        source,
    })?;

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
    let text = serde_yaml::to_string(&document).map_err(|err| ComposeError::EngineSpawnFailed {
        message: format!("could not serialize managed engine configuration: {err}"),
    })?;

    let path = namespace_dir.join(ENGINE_CONFIG_FILE);
    let temp = namespace_dir.join(format!("{ENGINE_CONFIG_FILE}.tmp"));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temp).map_err(|source| ComposeError::Io {
        path: temp.clone(),
        source,
    })?;
    file.write_all(text.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|source| ComposeError::Io {
            path: temp.clone(),
            source,
        })?;
    owner_only(&temp).map_err(|source| ComposeError::Io {
        path: temp.clone(),
        source,
    })?;
    std::fs::rename(&temp, &path).map_err(|source| ComposeError::Io {
        path: path.clone(),
        source,
    })?;
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
        .map_err(|source| ComposeError::ManagedEngineListenerUnavailable { listener, source })
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

/// Cross-process ownership of one managed engine namespace.
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

fn engine_log_path(root: &Path, daemon_namespace: &str) -> PathBuf {
    root.join(daemon_namespace).join("engine.log")
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
    fn engine_log_lives_under_the_daemon_namespace() {
        assert_eq!(
            engine_log_path(Path::new("/state"), "blue-whale"),
            Path::new("/state/blue-whale/engine.log")
        );
    }

    #[test]
    fn concurrent_managed_engines_cannot_claim_the_same_namespace() {
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

        let engine = ManagedEngine::spawn_with_paths(&script, &config, &log)
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
            match ManagedEngine::spawn_with_materialized_config(&script, &config, &log).await {
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

        let engine = ManagedEngine::spawn_with_paths(&script, &config, &log)
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

        let engine = ManagedEngine::spawn_with_paths(&script, &config, &log)
            .await
            .unwrap();
        let pid = engine.pid();
        assert!(crate::process::is_running(pid));

        let _status = engine.stop(Duration::from_secs(2)).await;

        assert!(!crate::process::is_running(pid));
    }
}

// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! External worker support: spawns installed binaries from `iii_workers/` as child processes.
//!
//! When the engine encounters a worker class that isn't registered in the built-in
//! registry, it checks `iii.toml` for installed workers and spawns the corresponding
//! binary, passing its config via a temporary YAML file.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
};

use serde::Deserialize;

use serde_json::Value;
use tokio::{
    process::Child,
    sync::Mutex,
    time::{Duration, timeout},
};

use crate::{engine::Engine, workers::traits::Worker};

/// Resolves an external worker class to a binary path.
///
/// Convention: `workers::image_resize::ImageResizeModule`
///   -> extract middle segment `image_resize`
///   -> convert underscores to hyphens: `image-resize`
///   -> binary at `iii_workers/image-resize`
///
/// Also checks `iii.toml` to verify the worker is actually installed.

#[derive(Deserialize)]
struct ManifestFile {
    workers: Option<BTreeMap<String, String>>,
}

/// A built-in external worker: a worker name that resolves to a binary
/// on $PATH plus extra args, instead of the conventional
/// `iii_workers/<name>` lookup. Used for daemons shipped as subcommands
/// of other iii binaries.
struct KnownExternal {
    /// Worker class slug as it appears in config.yaml (e.g. "iii-sandbox").
    name: &'static str,
    /// Binary to resolve via $PATH (fallback: ~/.local/bin/<binary>).
    binary: &'static str,
    /// Extra args prepended to the child's argv, before `--config <path>`.
    args: &'static [&'static str],
}

/// Workers shipped as subcommands of a single binary on $PATH, not as
/// standalone binaries in iii_workers/. Resolved in
/// `resolve_external_module_in` before the iii.toml lookup.
const KNOWN_EXTERNAL: &[KnownExternal] = &[
    KnownExternal {
        name: "iii-sandbox",
        binary: "iii-worker",
        args: &["sandbox-daemon"],
    },
    // Slug is "iii-worker-ops" to avoid clashing with the built-in
    // `WorkerManager` already registered as "iii-worker-manager".
    KnownExternal {
        name: "iii-worker-ops",
        binary: "iii-worker",
        args: &["worker-manager-daemon"],
    },
];

pub fn resolve_external_module(class: &str) -> Option<ExternalWorkerInfo> {
    let base_dir = std::env::current_dir().ok()?;
    resolve_external_module_in(&base_dir, class)
}

pub fn resolve_external_module_in(base_dir: &Path, class: &str) -> Option<ExternalWorkerInfo> {
    // Normalize the class to a binary-name candidate for KNOWN_EXTERNAL
    // matching. The real caller (`WorkerRegistry::create_worker`) passes
    // the bare `name` from config.yaml — e.g. "iii-sandbox". Tests and
    // the legacy iii.toml path use the `workers::<slug>` form. Handle
    // both by taking the last `::` segment (or the whole string if no
    // separator) and normalizing `_` -> `-`.
    let binary_name_candidate = class.rsplit("::").next().unwrap_or(class).replace('_', "-");

    // Check well-known externals first. These are shipped as
    // subcommands of iii-binaries on $PATH, so we skip the
    // iii_workers/<name> directory convention entirely.
    if let Some(hit) = KNOWN_EXTERNAL
        .iter()
        .find(|k| k.name == binary_name_candidate)
    {
        let binary_path = which::which(hit.binary)
            .or_else(|_| {
                std::env::var("HOME")
                    .map(|h| PathBuf::from(h).join(".local/bin").join(hit.binary))
                    .map_err(|_| which::Error::CannotFindBinaryPath)
                    .and_then(|p| {
                        if p.exists() {
                            Ok(p)
                        } else {
                            Err(which::Error::CannotFindBinaryPath)
                        }
                    })
            })
            .ok()?;

        return Some(ExternalWorkerInfo {
            name: binary_name_candidate,
            binary_path,
            extra_args: hit.args.iter().map(|s| (*s).to_string()).collect(),
        });
    }

    // iii.toml lookup uses the `workers::<slug>` class-path form.
    let parts: Vec<&str> = class.split("::").collect();
    if parts.len() < 2 {
        return None;
    }

    let slug = parts.get(1)?;
    let binary_name = slug.replace('_', "-");

    // Parse iii.toml and check for exact worker key
    let manifest_path = base_dir.join("iii.toml");
    if manifest_path.exists() {
        let content = std::fs::read_to_string(&manifest_path).ok()?;
        let parsed: ManifestFile = toml::from_str(&content).ok()?;
        let workers = parsed.workers.unwrap_or_default();
        if !workers.contains_key(&binary_name) {
            return None;
        }
    } else {
        return None;
    }

    let file_name = if cfg!(target_os = "windows") {
        format!("{}.exe", binary_name)
    } else {
        binary_name.clone()
    };
    let binary_path = base_dir.join("iii_workers").join(&file_name);
    if !binary_path.exists() {
        tracing::warn!(
            "Worker '{}' is in iii.toml but binary not found at {}",
            binary_name,
            binary_path.display()
        );
        return None;
    }

    Some(ExternalWorkerInfo {
        name: binary_name,
        binary_path,
        extra_args: vec![],
    })
}

pub struct ExternalWorkerInfo {
    pub name: String,
    pub binary_path: PathBuf,
    /// Args prepended to the child's argv, before `--config <path>`.
    /// Empty for conventional iii_workers/<binary> resolution; populated
    /// when the worker was matched via KNOWN_EXTERNAL.
    pub extra_args: Vec<String>,
}

/// A worker implementation backed by an external binary from `iii_workers/`.
///
/// The binary is spawned as a child process during `start_background_tasks`
/// and killed during `destroy`. The worker config is serialized to a temporary
/// YAML file and passed via `--config <path>`.
#[derive(Clone)]
pub struct ExternalWorker {
    display_name: &'static str,
    name: String,
    binary_path: PathBuf,
    extra_args: Vec<String>,
    config: Option<Value>,
    child: Arc<Mutex<Option<Child>>>,
    config_file: Arc<Mutex<Option<PathBuf>>>,
}

impl ExternalWorker {
    pub fn new(info: ExternalWorkerInfo, config: Option<Value>) -> Self {
        let name = info.name.clone();
        let display_name = Box::leak(format!("ExternalWorker({})", &name).into_boxed_str());
        Self {
            display_name,
            name,
            binary_path: info.binary_path,
            extra_args: info.extra_args,
            config,
            child: Arc::new(Mutex::new(None)),
            config_file: Arc::new(Mutex::new(None)),
        }
    }
}

#[async_trait::async_trait]
impl Worker for ExternalWorker {
    fn name(&self) -> &'static str {
        self.display_name
    }

    async fn create(
        _engine: Arc<Engine>,
        _config: Option<Value>,
    ) -> anyhow::Result<Box<dyn Worker>> {
        Err(anyhow::anyhow!(
            "ExternalWorker::create should not be called directly"
        ))
    }

    async fn initialize(&self) -> anyhow::Result<()> {
        tracing::info!(
            "External worker '{}' initialized (binary: {})",
            self.name,
            self.binary_path.display()
        );
        Ok(())
    }

    async fn start_background_tasks(
        &self,
        mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
        _shutdown_tx: tokio::sync::watch::Sender<bool>,
    ) -> anyhow::Result<()> {
        let config_path = if let Some(ref config) = self.config {
            let path = crate::workers::secure_temp::write_engine_config_temp(&self.name, config)
                .map_err(|e| anyhow::anyhow!(e))?;
            tracing::debug!("Wrote external worker config to {}", path.display());
            *self.config_file.lock().await = Some(path.clone());
            Some(path)
        } else {
            None
        };

        let child_handle = self.child.clone();

        // Wait for the engine to finish binding its listener, but bail early
        // if shutdown fires during the delay.
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(2)) => {}
            _ = shutdown_rx.changed() => {
                tracing::info!(
                    "External worker '{}' received shutdown before spawn",
                    self.name
                );
                return Ok(());
            }
        }

        let mut cmd = tokio::process::Command::new(&self.binary_path);
        for arg in &self.extra_args {
            cmd.arg(arg);
        }
        if let Some(ref path) = config_path {
            cmd.arg("--config").arg(path);
        }
        // Pipe stdio instead of inheriting the engine's TTY fds. External
        // workers (notably iii-sandbox) load libkrun, which raws the host
        // terminal when attaching the guest serial console — termios is per
        // tty, so any tcsetattr by the child mutates the engine's terminal
        // and scrambles tracing output until the VM exits. Piping isolates
        // libkrun's tcsetattr to non-tty fds (calls become no-ops with
        // ENOTTY) and forwarders below copy bytes line-atomic to the
        // engine's stdout/stderr. stdin is /dev/null because workers don't
        // read host stdin during normal operation.
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Detach process group on Unix for clean termination
        #[cfg(unix)]
        // SAFETY: setsid() is async-signal-safe per POSIX and safe to call in
        // pre_exec (which runs in the forked-but-not-yet-execed child). We create
        // a new session so the child process group can be cleanly terminated via
        // killpg() in kill_child().
        unsafe {
            cmd.pre_exec(|| {
                nix::unistd::setsid()
                    .map_err(|e| std::io::Error::other(format!("setsid failed: {e}")))?;
                Ok(())
            });
        }

        let mut child = cmd.spawn().map_err(|e| {
            anyhow::anyhow!(
                "Failed to spawn external worker '{}' ({}): {}",
                self.name,
                self.binary_path.display(),
                e
            )
        })?;

        // Forward child stdout/stderr to engine stdout/stderr line-atomically.
        // BufReader::read_until('\n') hands us complete lines (or trailing EOF
        // bytes). Each forwarded line is one Stdout::write_all / Stderr::write_all
        // call, which holds the global StdoutLock/StderrLock for the duration —
        // so worker lines never interleave mid-line with engine tracing output.
        if let Some(stdout) = child.stdout.take() {
            tokio::spawn(forward_pipe(stdout, false));
        }
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(forward_pipe(stderr, true));
        }

        tracing::info!(
            "Spawned external worker '{}' (pid: {:?})",
            self.name,
            child.id()
        );
        *child_handle.lock().await = Some(child);

        // Watch for shutdown signal
        let child_for_shutdown = self.child.clone();
        let name_for_shutdown = self.name.clone();
        tokio::spawn(async move {
            let _ = shutdown_rx.changed().await;
            tracing::info!(
                "External worker '{}' received shutdown signal",
                name_for_shutdown
            );
            kill_child(&child_for_shutdown).await;
        });

        Ok(())
    }

    async fn destroy(&self) -> anyhow::Result<()> {
        tracing::info!("Destroying external worker '{}'", self.name);
        kill_child(&self.child).await;

        if let Some(path) = self.config_file.lock().await.take() {
            if let Err(e) = std::fs::remove_file(&path) {
                tracing::warn!("failed to remove temp config {}: {}", path.display(), e);
            }
        }

        Ok(())
    }
}

async fn kill_child(child: &Arc<Mutex<Option<Child>>>) {
    if let Some(mut proc) = child.lock().await.take() {
        #[cfg(unix)]
        {
            if let Some(id) = proc.id() {
                let pgid = nix::unistd::Pid::from_raw(id as i32);
                let _ = nix::sys::signal::killpg(pgid, nix::sys::signal::Signal::SIGTERM);
            }
        }

        #[cfg(not(unix))]
        {
            let _ = proc.kill().await;
        }

        // Wait briefly for graceful shutdown, then force kill
        let exited = timeout(Duration::from_secs(3), proc.wait()).await;
        if exited.is_err() {
            #[cfg(unix)]
            if let Some(id) = proc.id() {
                let pgid = nix::unistd::Pid::from_raw(id as i32);
                let _ = nix::sys::signal::killpg(pgid, nix::sys::signal::Signal::SIGKILL);
            }
            #[cfg(not(unix))]
            {
                let _ = proc.kill().await;
            }
            let _ = proc.wait().await;
        }
    }
}

/// Copy a child process pipe to the engine's stdout/stderr line-atomically.
///
/// `read_until(b'\n', ...)` returns each complete line (with the trailing
/// newline) plus any unterminated tail at EOF. We then issue one
/// `write_all` call to the engine's stdout/stderr lock, which holds the
/// process-global `StdoutLock`/`StderrLock` for the full write — so a
/// worker log line never lands in the middle of an engine tracing event
/// and the terminal sees coherent line boundaries.
///
/// The task ends when the child closes its end of the pipe (Ok(0)) or
/// on any read error.
async fn forward_pipe<R>(reader: R, to_stderr: bool)
where
    R: tokio::io::AsyncRead + Unpin,
{
    use std::io::Write;
    use tokio::io::AsyncBufReadExt;

    let mut buf = tokio::io::BufReader::new(reader);
    let mut line: Vec<u8> = Vec::with_capacity(512);
    loop {
        line.clear();
        match buf.read_until(b'\n', &mut line).await {
            Ok(0) => return,
            Ok(_) => {
                if to_stderr {
                    let _ = std::io::stderr().lock().write_all(&line);
                } else {
                    let _ = std::io::stdout().lock().write_all(&line);
                }
            }
            Err(_) => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Creates a temp dir with an `iii.toml` and optional binaries under `iii_workers/`.
    fn setup_manifest(workers: &[(&str, &str)], binaries: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::TempDir::new().unwrap();

        let mut toml = String::from("[workers]\n");
        for (name, version) in workers {
            toml.push_str(&format!("{} = \"{}\"\n", name, version));
        }
        std::fs::write(dir.path().join("iii.toml"), &toml).unwrap();

        let workers_dir = dir.path().join("iii_workers");
        std::fs::create_dir_all(&workers_dir).unwrap();
        for bin in binaries {
            std::fs::write(workers_dir.join(bin), b"fake-binary").unwrap();
        }

        dir
    }

    #[test]
    fn resolve_external_module_returns_none_for_builtin_name() {
        assert!(resolve_external_module("iii-stream").is_none());
    }

    #[test]
    fn resolve_external_module_returns_none_for_short_class() {
        assert!(resolve_external_module("SomeModule").is_none());
    }

    #[test]
    fn resolve_happy_path_three_segment_class() {
        let dir = setup_manifest(&[("image-resize", "1.0.0")], &["image-resize"]);

        let result =
            resolve_external_module_in(dir.path(), "workers::image_resize::ImageResizeModule");
        let info = result.expect("should resolve valid external module");
        assert_eq!(info.name, "image-resize");
        assert_eq!(
            info.binary_path,
            dir.path().join("iii_workers/image-resize")
        );
    }

    #[test]
    fn resolve_happy_path_two_segment_class() {
        let dir = setup_manifest(&[("my-worker", "2.0.0")], &["my-worker"]);

        let result = resolve_external_module_in(dir.path(), "workers::my_worker");
        let info = result.expect("two-segment class should resolve");
        assert_eq!(info.name, "my-worker");
    }

    #[test]
    fn resolve_underscore_to_hyphen_conversion() {
        let dir = setup_manifest(&[("data-transform", "0.1.0")], &["data-transform"]);

        let result =
            resolve_external_module_in(dir.path(), "workers::data_transform::DataTransformModule");
        let info = result.expect("underscores should convert to hyphens");
        assert_eq!(info.name, "data-transform");
    }

    #[test]
    fn resolve_selects_correct_worker_among_multiple() {
        let dir = setup_manifest(
            &[("alpha", "1.0.0"), ("beta", "2.0.0"), ("gamma", "3.0.0")],
            &["alpha", "beta", "gamma"],
        );

        let result = resolve_external_module_in(dir.path(), "workers::beta::BetaModule");
        let info = result.expect("should resolve 'beta' among multiple workers");
        assert_eq!(info.name, "beta");
    }

    #[test]
    fn resolve_slug_with_no_underscores_passes_through() {
        let dir = setup_manifest(&[("simple", "1.0.0")], &["simple"]);

        let result = resolve_external_module_in(dir.path(), "workers::simple::SimpleModule");
        let info = result.expect("slug without underscores should pass through unchanged");
        assert_eq!(info.name, "simple");
    }

    #[test]
    fn resolve_returns_none_for_empty_class() {
        let dir = setup_manifest(&[("x", "1.0.0")], &["x"]);
        assert!(resolve_external_module_in(dir.path(), "").is_none());
    }

    #[test]
    fn resolve_returns_none_for_single_segment_class() {
        let dir = setup_manifest(&[("x", "1.0.0")], &["x"]);
        assert!(resolve_external_module_in(dir.path(), "OnlyOneSegment").is_none());
    }

    #[test]
    fn resolve_returns_none_when_no_iii_toml() {
        let dir = tempfile::TempDir::new().unwrap();
        // No iii.toml created
        assert!(
            resolve_external_module_in(dir.path(), "workers::foo::FooModule").is_none(),
            "should return None when iii.toml does not exist"
        );
    }

    #[test]
    fn resolve_returns_none_when_worker_not_in_manifest() {
        let dir = setup_manifest(&[("other-worker", "1.0.0")], &["other-worker"]);

        assert!(
            resolve_external_module_in(dir.path(), "workers::missing::MissingModule").is_none(),
            "should return None when worker key is not in iii.toml"
        );
    }

    #[test]
    fn resolve_returns_none_when_binary_missing_on_disk() {
        // Worker is in iii.toml but binary file doesn't exist
        let dir = setup_manifest(&[("ghost", "1.0.0")], &[]); // no binaries created

        assert!(
            resolve_external_module_in(dir.path(), "workers::ghost::GhostModule").is_none(),
            "should return None when binary is not on disk"
        );
    }

    #[test]
    fn resolve_returns_none_for_empty_workers_section() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("iii.toml"), "[workers]\n").unwrap();
        std::fs::create_dir_all(dir.path().join("iii_workers")).unwrap();

        assert!(
            resolve_external_module_in(dir.path(), "workers::foo::FooModule").is_none(),
            "empty workers section should not match anything"
        );
    }

    #[test]
    fn resolve_returns_none_when_toml_has_no_workers_key() {
        let dir = tempfile::TempDir::new().unwrap();
        // Valid TOML but no [workers] section
        std::fs::write(dir.path().join("iii.toml"), "[package]\nname = \"test\"\n").unwrap();
        std::fs::create_dir_all(dir.path().join("iii_workers")).unwrap();

        assert!(
            resolve_external_module_in(dir.path(), "workers::foo::FooModule").is_none(),
            "should return None when iii.toml has no [workers] section"
        );
    }

    #[test]
    fn resolve_no_false_positive_on_substring() {
        let dir = setup_manifest(&[("image-resize", "1.0.0")], &["image-resize"]);

        // "workers::image::ImageModule" extracts slug "image" which should NOT match "image-resize"
        let result = resolve_external_module_in(dir.path(), "workers::image::ImageModule");
        assert!(
            result.is_none(),
            "Should not match 'image' when only 'image-resize' is installed"
        );
    }

    #[test]
    fn resolve_returns_none_for_malformed_toml() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("iii.toml"), "this is not valid toml {{{{").unwrap();
        std::fs::create_dir_all(dir.path().join("iii_workers")).unwrap();

        assert!(
            resolve_external_module_in(dir.path(), "workers::foo::FooModule").is_none(),
            "should return None for unparseable iii.toml"
        );
    }

    #[test]
    fn external_module_new_without_config() {
        let info = ExternalWorkerInfo {
            name: "test-worker".to_string(),
            binary_path: PathBuf::from("/tmp/test-worker"),
            extra_args: vec![],
        };
        let module = ExternalWorker::new(info, None);
        assert_eq!(module.name, "test-worker");
        assert_eq!(module.binary_path, PathBuf::from("/tmp/test-worker"));
        assert!(module.config.is_none());
    }

    #[test]
    fn external_module_new_with_config() {
        let config = serde_json::json!({"port": 8080, "debug": true});
        let info = ExternalWorkerInfo {
            name: "configured-worker".to_string(),
            binary_path: PathBuf::from("/tmp/configured-worker"),
            extra_args: vec![],
        };
        let module = ExternalWorker::new(info, Some(config.clone()));
        assert_eq!(module.config, Some(config));
    }

    #[test]
    fn external_module_display_name_format() {
        let info = ExternalWorkerInfo {
            name: "my-worker".to_string(),
            binary_path: PathBuf::from("/tmp/my-worker"),
            extra_args: vec![],
        };
        let module = ExternalWorker::new(info, None);
        assert_eq!(module.name(), "ExternalWorker(my-worker)");
    }

    #[test]
    fn external_module_name_returns_consistent_pointer() {
        let info = ExternalWorkerInfo {
            name: "test-worker".to_string(),
            binary_path: PathBuf::from("/tmp/test-worker"),
            extra_args: vec![],
        };
        let module = ExternalWorker::new(info, None);
        let name1 = module.name();
        let name2 = module.name();
        assert_eq!(
            name1 as *const str, name2 as *const str,
            "name() should return the same pointer on repeated calls"
        );
    }

    #[test]
    fn external_module_clone_shares_child_and_config_file() {
        let info = ExternalWorkerInfo {
            name: "clone-test".to_string(),
            binary_path: PathBuf::from("/tmp/clone-test"),
            extra_args: vec![],
        };
        let module = ExternalWorker::new(info, None);
        let cloned = module.clone();

        // Arc pointers should be the same (shared state)
        assert!(Arc::ptr_eq(&module.child, &cloned.child));
        assert!(Arc::ptr_eq(&module.config_file, &cloned.config_file));
        assert_eq!(module.name, cloned.name);
    }

    #[tokio::test]
    async fn external_module_initialize_succeeds() {
        let info = ExternalWorkerInfo {
            name: "init-test".to_string(),
            binary_path: PathBuf::from("/tmp/init-test"),
            extra_args: vec![],
        };
        let module = ExternalWorker::new(info, None);
        assert!(module.initialize().await.is_ok());
    }

    #[tokio::test]
    async fn external_module_destroy_succeeds_with_no_child() {
        let info = ExternalWorkerInfo {
            name: "destroy-test".to_string(),
            binary_path: PathBuf::from("/tmp/destroy-test"),
            extra_args: vec![],
        };
        let module = ExternalWorker::new(info, None);
        // destroy on a fresh module (no spawned child) should succeed
        assert!(module.destroy().await.is_ok());
    }

    #[tokio::test]
    async fn external_module_destroy_cleans_up_config_file() {
        let info = ExternalWorkerInfo {
            name: "cleanup-test".to_string(),
            binary_path: PathBuf::from("/tmp/cleanup-test"),
            extra_args: vec![],
        };
        let module = ExternalWorker::new(info, None);

        // Simulate a config file being written
        let temp_config = std::env::temp_dir().join("iii-cleanup-test-config.yaml");
        std::fs::write(&temp_config, "test: true").unwrap();
        *module.config_file.lock().await = Some(temp_config.clone());

        assert!(
            temp_config.exists(),
            "config file should exist before destroy"
        );
        module.destroy().await.unwrap();
        assert!(
            !temp_config.exists(),
            "config file should be cleaned up after destroy"
        );
    }

    #[tokio::test]
    async fn kill_child_noop_when_no_child() {
        let child_handle: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));
        // Should not panic or error
        kill_child(&child_handle).await;
        assert!(child_handle.lock().await.is_none());
    }

    #[test]
    fn external_worker_info_default_extra_args_is_empty() {
        let info = ExternalWorkerInfo {
            name: "unit-test".into(),
            binary_path: PathBuf::from("/tmp/fake"),
            extra_args: vec![],
        };
        assert!(info.extra_args.is_empty());
    }

    #[test]
    fn known_external_iii_sandbox_dispatches_to_iii_worker() {
        let hit = KNOWN_EXTERNAL
            .iter()
            .find(|k| k.name == "iii-sandbox")
            .expect("iii-sandbox must be in KNOWN_EXTERNAL");
        assert_eq!(hit.binary, "iii-worker");
        assert_eq!(hit.args, &["sandbox-daemon"]);
    }

    #[test]
    fn known_external_iii_worker_ops_dispatches_to_iii_worker() {
        let hit = KNOWN_EXTERNAL
            .iter()
            .find(|k| k.name == "iii-worker-ops")
            .expect("iii-worker-ops must be in KNOWN_EXTERNAL");
        assert_eq!(hit.binary, "iii-worker");
        assert_eq!(hit.args, &["worker-manager-daemon"]);
    }

    #[test]
    #[serial_test::serial]
    fn resolves_iii_worker_ops_to_subcommand() {
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("iii-worker");
        std::fs::write(&fake, "#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let orig_path = std::env::var_os("PATH");
        // SAFETY: test is marked #[serial] so no other thread mutates env concurrently.
        unsafe {
            std::env::set_var("PATH", dir.path());
        }

        let result = resolve_external_module_in(dir.path(), "iii-worker-ops");

        // SAFETY: test is marked #[serial]; restoring original.
        unsafe {
            if let Some(v) = orig_path {
                std::env::set_var("PATH", v);
            } else {
                std::env::remove_var("PATH");
            }
        }

        let info = result.expect("bare 'iii-worker-ops' must resolve via KNOWN_EXTERNAL");
        assert_eq!(info.name, "iii-worker-ops");
        assert_eq!(info.binary_path, fake);
        assert_eq!(info.extra_args, vec!["worker-manager-daemon".to_string()]);
    }

    #[test]
    #[serial_test::serial]
    fn resolve_external_module_in_hits_known_external_when_binary_on_path() {
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("iii-worker");
        std::fs::write(&fake, "#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let orig_path = std::env::var_os("PATH");
        // SAFETY: test is marked #[serial] so no other thread mutates env concurrently.
        unsafe {
            std::env::set_var("PATH", dir.path());
        }

        let result = resolve_external_module_in(dir.path(), "workers::iii_sandbox");

        // SAFETY: test is marked #[serial] so no other thread mutates env concurrently.
        unsafe {
            if let Some(v) = orig_path {
                std::env::set_var("PATH", v);
            } else {
                std::env::remove_var("PATH");
            }
        }

        let info = result.expect("iii-sandbox should resolve via KNOWN_EXTERNAL");
        assert_eq!(info.name, "iii-sandbox");
        assert_eq!(info.binary_path, fake);
        assert_eq!(info.extra_args, vec!["sandbox-daemon".to_string()]);
    }

    // Regression: `WorkerRegistry::create_worker` calls
    // `resolve_external_module(name)` with the bare `name` from
    // config.yaml — e.g. "iii-sandbox" — not the `workers::iii_sandbox`
    // class-path form. The KNOWN_EXTERNAL lookup must handle both.
    #[test]
    #[serial_test::serial]
    fn resolve_external_module_in_hits_known_external_with_bare_name() {
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("iii-worker");
        std::fs::write(&fake, "#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let orig_path = std::env::var_os("PATH");
        // SAFETY: #[serial].
        unsafe {
            std::env::set_var("PATH", dir.path());
        }

        let result = resolve_external_module_in(dir.path(), "iii-sandbox");

        // SAFETY: #[serial]; restoring original.
        unsafe {
            if let Some(v) = orig_path {
                std::env::set_var("PATH", v);
            } else {
                std::env::remove_var("PATH");
            }
        }

        let info = result.expect("bare 'iii-sandbox' must resolve via KNOWN_EXTERNAL");
        assert_eq!(info.name, "iii-sandbox");
        assert_eq!(info.binary_path, fake);
        assert_eq!(info.extra_args, vec!["sandbox-daemon".to_string()]);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn external_worker_spawns_child_with_extra_args_before_config() {
        let dir = tempfile::tempdir().unwrap();
        let argv_log = dir.path().join("argv.txt");

        let fixture =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/argv_probe.sh");

        let info = ExternalWorkerInfo {
            name: "probe".into(),
            binary_path: fixture,
            extra_args: vec!["first-arg".into(), "second-arg".into()],
        };

        let worker = ExternalWorker::new(info, Some(serde_json::json!({"key": "val"})));

        // The probe reads ARGV_LOG from env. Set it on the parent so
        // the spawned child inherits.
        // SAFETY: edition 2024 requires unsafe wrap; test is #[serial].
        unsafe {
            std::env::set_var("ARGV_LOG", &argv_log);
        }

        let (tx, rx) = tokio::sync::watch::channel(false);
        worker.start_background_tasks(rx, tx.clone()).await.unwrap();

        // Poll for the argv file (probe writes immediately; worker may
        // sleep briefly before spawning).
        let mut got = None;
        for _ in 0..50 {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            if let Ok(s) = std::fs::read_to_string(&argv_log) {
                got = Some(s.trim().to_string());
                break;
            }
        }
        let _ = tx.send(true);
        worker.destroy().await.unwrap();
        // SAFETY: test is #[serial].
        unsafe {
            std::env::remove_var("ARGV_LOG");
        }

        let argv = got.expect("probe fixture never wrote argv");
        assert!(
            argv.starts_with("first-arg second-arg --config "),
            "expected extra args before --config, got: {argv:?}"
        );
    }
}

// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

//! External module support: spawns installed binaries from `iii_workers/` as child processes.
//!
//! When the engine encounters a module class that isn't registered in the built-in
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

use crate::{engine::Engine, modules::module::Module};

/// Resolves an external module class to a binary path.
///
/// Convention: `modules::image_resize::ImageResizeModule`
///   -> extract middle segment `image_resize`
///   -> convert underscores to hyphens: `image-resize`
///   -> binary at `iii_workers/image-resize`
///
/// Also checks `iii.toml` to verify the module is actually installed.

#[derive(Deserialize)]
struct ManifestFile {
    workers: Option<BTreeMap<String, String>>,
}

pub fn resolve_external_module(class: &str) -> Option<ExternalModuleInfo> {
    let base_dir = std::env::current_dir().ok()?;
    resolve_external_module_in(&base_dir, class)
}

pub fn resolve_external_module_in(base_dir: &Path, class: &str) -> Option<ExternalModuleInfo> {
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

    Some(ExternalModuleInfo {
        name: binary_name,
        binary_path,
    })
}

pub struct ExternalModuleInfo {
    pub name: String,
    pub binary_path: PathBuf,
}

/// A module backed by an external binary from `iii_workers/`.
///
/// The binary is spawned as a child process during `start_background_tasks`
/// and killed during `destroy`. The module config is serialized to a temporary
/// YAML file and passed via `--config <path>`.
#[derive(Clone)]
pub struct ExternalModule {
    display_name: &'static str,
    name: String,
    binary_path: PathBuf,
    config: Option<Value>,
    child: Arc<Mutex<Option<Child>>>,
    config_file: Arc<Mutex<Option<PathBuf>>>,
}

impl ExternalModule {
    pub fn new(info: ExternalModuleInfo, config: Option<Value>) -> Self {
        let name = info.name.clone();
        let display_name = Box::leak(format!("ExternalModule({})", &name).into_boxed_str());
        Self {
            display_name,
            name,
            binary_path: info.binary_path,
            config,
            child: Arc::new(Mutex::new(None)),
            config_file: Arc::new(Mutex::new(None)),
        }
    }
}

#[async_trait::async_trait]
impl Module for ExternalModule {
    fn name(&self) -> &'static str {
        self.display_name
    }

    async fn create(
        _engine: Arc<Engine>,
        _config: Option<Value>,
    ) -> anyhow::Result<Box<dyn Module>> {
        Err(anyhow::anyhow!(
            "ExternalModule::create should not be called directly"
        ))
    }

    async fn initialize(&self) -> anyhow::Result<()> {
        tracing::info!(
            "External module '{}' initialized (binary: {})",
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
        // Write config to a temp file if we have config
        let config_path = if let Some(ref config) = self.config {
            let yaml = serde_yaml::to_string(config)?;
            let temp_dir = std::env::temp_dir();
            let config_path = temp_dir.join(format!("iii-{}-config.yaml", self.name));
            std::fs::write(&config_path, &yaml)?;
            tracing::debug!("Wrote external module config to {}", config_path.display());
            *self.config_file.lock().await = Some(config_path.clone());
            Some(config_path)
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
                    "External module '{}' received shutdown before spawn",
                    self.name
                );
                return Ok(());
            }
        }

        let mut cmd = tokio::process::Command::new(&self.binary_path);
        if let Some(ref path) = config_path {
            cmd.arg("--config").arg(path);
        }
        cmd.stdout(Stdio::inherit()).stderr(Stdio::inherit());

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

        let child = cmd.spawn().map_err(|e| {
            anyhow::anyhow!(
                "Failed to spawn external module '{}' ({}): {}",
                self.name,
                self.binary_path.display(),
                e
            )
        })?;

        tracing::info!(
            "Spawned external module '{}' (pid: {:?})",
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
                "External module '{}' received shutdown signal",
                name_for_shutdown
            );
            kill_child(&child_for_shutdown).await;
        });

        Ok(())
    }

    async fn destroy(&self) -> anyhow::Result<()> {
        tracing::info!("Destroying external module '{}'", self.name);
        kill_child(&self.child).await;

        // Clean up temp config file
        if let Some(path) = self.config_file.lock().await.take() {
            let _ = std::fs::remove_file(&path);
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── helpers ──────────────────────────────────────────────────────────

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

    // ── resolve_external_module (uses cwd, no iii.toml) ─────────────────

    #[test]
    fn resolve_external_module_returns_none_for_builtin_class() {
        // No iii.toml in test environment
        assert!(resolve_external_module("modules::stream::StreamModule").is_none());
    }

    #[test]
    fn resolve_external_module_returns_none_for_short_class() {
        assert!(resolve_external_module("SomeModule").is_none());
    }

    // ── resolve_external_module_in: happy paths ─────────────────────────

    #[test]
    fn resolve_happy_path_three_segment_class() {
        let dir = setup_manifest(&[("image-resize", "1.0.0")], &["image-resize"]);

        let result =
            resolve_external_module_in(dir.path(), "modules::image_resize::ImageResizeModule");
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

        let result = resolve_external_module_in(dir.path(), "modules::my_worker");
        let info = result.expect("two-segment class should resolve");
        assert_eq!(info.name, "my-worker");
    }

    #[test]
    fn resolve_underscore_to_hyphen_conversion() {
        let dir = setup_manifest(&[("data-transform", "0.1.0")], &["data-transform"]);

        let result =
            resolve_external_module_in(dir.path(), "modules::data_transform::DataTransformModule");
        let info = result.expect("underscores should convert to hyphens");
        assert_eq!(info.name, "data-transform");
    }

    #[test]
    fn resolve_selects_correct_worker_among_multiple() {
        let dir = setup_manifest(
            &[("alpha", "1.0.0"), ("beta", "2.0.0"), ("gamma", "3.0.0")],
            &["alpha", "beta", "gamma"],
        );

        let result = resolve_external_module_in(dir.path(), "modules::beta::BetaModule");
        let info = result.expect("should resolve 'beta' among multiple workers");
        assert_eq!(info.name, "beta");
    }

    #[test]
    fn resolve_slug_with_no_underscores_passes_through() {
        let dir = setup_manifest(&[("simple", "1.0.0")], &["simple"]);

        let result = resolve_external_module_in(dir.path(), "modules::simple::SimpleModule");
        let info = result.expect("slug without underscores should pass through unchanged");
        assert_eq!(info.name, "simple");
    }

    // ── resolve_external_module_in: failure paths ───────────────────────

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
            resolve_external_module_in(dir.path(), "modules::foo::FooModule").is_none(),
            "should return None when iii.toml does not exist"
        );
    }

    #[test]
    fn resolve_returns_none_when_worker_not_in_manifest() {
        let dir = setup_manifest(&[("other-worker", "1.0.0")], &["other-worker"]);

        assert!(
            resolve_external_module_in(dir.path(), "modules::missing::MissingModule").is_none(),
            "should return None when worker key is not in iii.toml"
        );
    }

    #[test]
    fn resolve_returns_none_when_binary_missing_on_disk() {
        // Worker is in iii.toml but binary file doesn't exist
        let dir = setup_manifest(&[("ghost", "1.0.0")], &[]); // no binaries created

        assert!(
            resolve_external_module_in(dir.path(), "modules::ghost::GhostModule").is_none(),
            "should return None when binary is not on disk"
        );
    }

    #[test]
    fn resolve_returns_none_for_empty_workers_section() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("iii.toml"), "[workers]\n").unwrap();
        std::fs::create_dir_all(dir.path().join("iii_workers")).unwrap();

        assert!(
            resolve_external_module_in(dir.path(), "modules::foo::FooModule").is_none(),
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
            resolve_external_module_in(dir.path(), "modules::foo::FooModule").is_none(),
            "should return None when iii.toml has no [workers] section"
        );
    }

    #[test]
    fn resolve_no_false_positive_on_substring() {
        let dir = setup_manifest(&[("image-resize", "1.0.0")], &["image-resize"]);

        // "modules::image::ImageModule" extracts slug "image" which should NOT match "image-resize"
        let result = resolve_external_module_in(dir.path(), "modules::image::ImageModule");
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
            resolve_external_module_in(dir.path(), "modules::foo::FooModule").is_none(),
            "should return None for unparseable iii.toml"
        );
    }

    // ── ExternalModule construction ─────────────────────────────────────

    #[test]
    fn external_module_new_without_config() {
        let info = ExternalModuleInfo {
            name: "test-worker".to_string(),
            binary_path: PathBuf::from("/tmp/test-worker"),
        };
        let module = ExternalModule::new(info, None);
        assert_eq!(module.name, "test-worker");
        assert_eq!(module.binary_path, PathBuf::from("/tmp/test-worker"));
        assert!(module.config.is_none());
    }

    #[test]
    fn external_module_new_with_config() {
        let config = serde_json::json!({"port": 8080, "debug": true});
        let info = ExternalModuleInfo {
            name: "configured-worker".to_string(),
            binary_path: PathBuf::from("/tmp/configured-worker"),
        };
        let module = ExternalModule::new(info, Some(config.clone()));
        assert_eq!(module.config, Some(config));
    }

    #[test]
    fn external_module_display_name_format() {
        let info = ExternalModuleInfo {
            name: "my-worker".to_string(),
            binary_path: PathBuf::from("/tmp/my-worker"),
        };
        let module = ExternalModule::new(info, None);
        assert_eq!(module.name(), "ExternalModule(my-worker)");
    }

    #[test]
    fn external_module_name_returns_consistent_pointer() {
        let info = ExternalModuleInfo {
            name: "test-worker".to_string(),
            binary_path: PathBuf::from("/tmp/test-worker"),
        };
        let module = ExternalModule::new(info, None);
        let name1 = module.name();
        let name2 = module.name();
        assert_eq!(
            name1 as *const str, name2 as *const str,
            "name() should return the same pointer on repeated calls"
        );
    }

    #[test]
    fn external_module_clone_shares_child_and_config_file() {
        let info = ExternalModuleInfo {
            name: "clone-test".to_string(),
            binary_path: PathBuf::from("/tmp/clone-test"),
        };
        let module = ExternalModule::new(info, None);
        let cloned = module.clone();

        // Arc pointers should be the same (shared state)
        assert!(Arc::ptr_eq(&module.child, &cloned.child));
        assert!(Arc::ptr_eq(&module.config_file, &cloned.config_file));
        assert_eq!(module.name, cloned.name);
    }

    // ── async Module trait methods ──────────────────────────────────────

    #[tokio::test]
    async fn external_module_initialize_succeeds() {
        let info = ExternalModuleInfo {
            name: "init-test".to_string(),
            binary_path: PathBuf::from("/tmp/init-test"),
        };
        let module = ExternalModule::new(info, None);
        assert!(module.initialize().await.is_ok());
    }

    #[tokio::test]
    async fn external_module_destroy_succeeds_with_no_child() {
        let info = ExternalModuleInfo {
            name: "destroy-test".to_string(),
            binary_path: PathBuf::from("/tmp/destroy-test"),
        };
        let module = ExternalModule::new(info, None);
        // destroy on a fresh module (no spawned child) should succeed
        assert!(module.destroy().await.is_ok());
    }

    #[tokio::test]
    async fn external_module_destroy_cleans_up_config_file() {
        let info = ExternalModuleInfo {
            name: "cleanup-test".to_string(),
            binary_path: PathBuf::from("/tmp/cleanup-test"),
        };
        let module = ExternalModule::new(info, None);

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
}

// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::{
    collections::{HashMap, HashSet},
    env,
    future::Future,
    pin::Pin,
    sync::{Arc, LazyLock, RwLock},
};

use regex::Regex;
use serde::Deserialize;
use serde_json::Value;

use notify::Watcher;

use super::{registry::WorkerRegistration, traits::Worker};
use crate::engine::Engine;

// =============================================================================
// EngineConfig (YAML structure)
// =============================================================================

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngineConfig {
    #[serde(default)]
    pub modules: Vec<WorkerEntry>,
    #[serde(default)]
    pub workers: Vec<WorkerEntry>,
}

impl EngineConfig {
    pub fn default_modules(self) -> Self {
        let modules = default_worker_entries();

        Self {
            modules,
            workers: Vec::new(),
        }
    }

    pub(crate) fn expand_env_vars(yaml_content: &str) -> String {
        static RE: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"\$\{([^}:]+)(?::([^}]*))?\}").unwrap());
        let re = &*RE;

        re.replace_all(yaml_content, |caps: &regex::Captures| {
            let var_name = &caps[1];
            let default_value = caps.get(2).map(|m| m.as_str());

            match env::var(var_name) {
                Ok(value) => value,
                Err(_) => match default_value {
                    Some(default) => default.to_string(),
                    None => {
                        tracing::error!(
                            "Environment variable '{}' not set and no
    default provided",
                            var_name
                        );
                        panic!(
                            "Environment variable '{}' not set and no default provided",
                            var_name
                        );
                    }
                },
            }
        })
        .to_string()
    }

    /// Loads config strictly from the given file path.
    /// Returns a clear error if the file does not exist or cannot be parsed.
    pub fn config_file(path: &str) -> anyhow::Result<Self> {
        let yaml_content = std::fs::read_to_string(path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                anyhow::anyhow!(
                    "Config file not found: '{}'.\n\
                     Hint: create a config.yaml or pass --use-default-config to run with defaults.",
                    path
                )
            } else {
                anyhow::anyhow!("Failed to read config file '{}': {}", path, e)
            }
        })?;
        let yaml_content = Self::expand_env_vars(&yaml_content);
        serde_yaml::from_str(&yaml_content)
            .map_err(|e| anyhow::anyhow!("Failed to parse config file '{}': {}", path, e))
    }

    /// Returns a config with default port and default modules (from inventory).
    /// Use this when explicitly opting in to run without a config file.
    pub fn default_config() -> Self {
        tracing::info!("Using default config (no config file)");
        Self {
            modules: default_worker_entries(),
            workers: Vec::new(),
        }
    }
}

fn default_worker_entries() -> Vec<WorkerEntry> {
    inventory::iter::<WorkerRegistration>
        .into_iter()
        .filter(|registration| registration.is_default)
        .map(|registration| WorkerEntry {
            name: registration.name.to_string(),
            image: None,
            config: None,
        })
        .collect()
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct WorkerEntry {
    pub name: String,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub config: Option<Value>,
}

// =============================================================================
// Type Aliases for Factories
// =============================================================================

/// Factory function type for creating Modules (async)
type WorkerFactory = Arc<
    dyn Fn(
            Arc<Engine>,
            Option<Value>,
        ) -> Pin<Box<dyn Future<Output = anyhow::Result<Box<dyn Worker>>> + Send>>
        + Send
        + Sync,
>;

/// Info about a registered module
struct WorkerInfo {
    factory: WorkerFactory,
}

// =============================================================================
// WorkerRegistry (unified registry for modules and adapters)
// =============================================================================

pub struct WorkerRegistry {
    worker_factories: RwLock<HashMap<String, WorkerInfo>>,
}

impl WorkerRegistry {
    pub fn new() -> Self {
        Self {
            worker_factories: RwLock::new(HashMap::new()),
        }
    }

    fn register_from_inventory(&self) {
        for registration in inventory::iter::<WorkerRegistration> {
            let factory = registration.factory;
            let info = WorkerInfo {
                factory: Arc::new(move |engine, config| (factory)(engine, config)),
            };
            self.worker_factories
                .write()
                .expect("RwLock poisoned")
                .insert(registration.name.to_string(), info);
        }
    }

    // =========================================================================
    // Module Registration
    // =========================================================================

    /// Registers a module by type
    ///
    /// The module must implement `Module`. The registry uses `M::create()` to create instances.
    pub fn register<M: Worker + 'static>(&self, name: &str) {
        let info = WorkerInfo {
            factory: Arc::new(|engine, config| Box::pin(M::create(engine, config))),
        };

        self.worker_factories
            .write()
            .expect("RwLock poisoned")
            .insert(name.to_string(), info);
    }

    /// Creates a module instance using the resolution chain:
    /// 1. Validates that built-in workers cannot have an `image` field.
    /// 2. Tries the built-in registry.
    /// 3. Falls back to legacy external worker resolution via `iii.toml`.
    /// 4. Delegates to `iii-worker start` (handles registry lookup, binary
    ///    download, and OCI spawning autonomously).
    pub async fn create_worker(
        self: &Arc<Self>,
        name: &str,
        image: Option<&str>,
        engine: Arc<Engine>,
        config: Option<Value>,
    ) -> anyhow::Result<Box<dyn Worker>> {
        // 1. Validate: image + built-in = error
        if image.is_some() {
            let is_builtin = self
                .worker_factories
                .read()
                .expect("RwLock poisoned")
                .contains_key(name);
            if is_builtin {
                return Err(anyhow::anyhow!(
                    "Worker '{}' is a built-in worker and cannot have an 'image' field. \
                     Remove 'image' or use a different name.",
                    name
                ));
            }
        }

        // 2. Try built-in registry (skip if image is set — that's always external)
        if image.is_none() {
            let factory = {
                let factories = self.worker_factories.read().expect("RwLock poisoned");
                factories.get(name).map(|info| info.factory.clone())
            };
            if let Some(factory) = factory {
                return factory(engine, config).await;
            }
        }

        // 3. Legacy: external worker (iii.toml + iii_workers/)
        if image.is_none()
            && let Some(info) = super::external::resolve_external_module(name)
        {
            tracing::info!(
                "Resolved '{}' as external worker '{}' ({})",
                name,
                info.name,
                info.binary_path.display()
            );
            let module = super::external::ExternalWorker::new(info, config);
            return Ok(Box::new(module));
        }

        // 4. Delegate to iii-worker start (handles registry lookup, binary
        //    download, OCI pull, and spawning autonomously)
        tracing::info!(worker = %name, "Starting external worker via iii-worker");
        let port = crate::workers::worker::DEFAULT_PORT;
        let process = super::registry_worker::ExternalWorkerProcess::spawn(name, port)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to start worker '{}': {}", name, e))?;
        Ok(Box::new(
            super::registry_worker::ExternalWorkerWrapper::new(process),
        ))
    }

    // =========================================================================
    // Default Registration
    // =========================================================================

    pub fn with_inventory() -> Self {
        let registry = Self::new();
        registry.register_from_inventory();
        registry
    }
}

impl Default for WorkerRegistry {
    fn default() -> Self {
        Self::with_inventory()
    }
}

impl WorkerEntry {
    /// Returns the worker type name used for factory lookup. For entries with
    /// instance suffixes like `iii-http#1`, this strips the `#N` and returns
    /// the base name `iii-http`.
    pub fn worker_type(&self) -> &str {
        self.name.split('#').next().unwrap_or(&self.name)
    }

    /// Creates a module instance from this entry
    pub async fn create_worker(
        &self,
        engine: Arc<Engine>,
        registry: &Arc<WorkerRegistry>,
    ) -> anyhow::Result<Box<dyn Worker>> {
        registry
            .create_worker(
                self.worker_type(),
                self.image.as_deref(),
                engine,
                self.config.clone(),
            )
            .await
            .map_err(|e| anyhow::anyhow!("Failed to create {}: {}", self.name, e))
    }
}

/// Assigns unique instance IDs to entries with duplicate names. The first
/// occurrence keeps its original name; subsequent occurrences get `#1`,
/// `#2`, etc. appended. This lets the diff and running-worker tracking
/// treat each entry independently.
pub fn assign_instance_ids(entries: &mut Vec<WorkerEntry>) {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for entry in entries.iter_mut() {
        let base = entry.name.clone();
        let count = counts.entry(base.clone()).or_insert(0);
        if *count > 0 {
            entry.name = format!("{}#{}", base, count);
        }
        *count += 1;
    }
}

// =============================================================================
// EngineBuilder
// =============================================================================

/// Builder pattern for configuring and starting the Engine.
///
/// # Examples
///
/// Load from a config file (fails if missing):
/// ```ignore
/// EngineBuilder::new()
///     .config_file("config.yaml")?
///     .build().await?
///     .serve().await?;
/// ```
///
/// Run with built-in defaults (no config file):
/// ```ignore
/// EngineBuilder::new()
///     .default_config()
///     .build().await?
///     .serve().await?;
/// ```
///
/// Register custom module:
/// ```ignore
/// EngineBuilder::new()
///     .register_worker::<MyCustomModule>("my::CustomModule")
///     .add_worker("my::CustomModule", Some(json!({"key": "value"})))
///     .build().await?
///     .serve().await?;
/// ```
pub struct EngineBuilder {
    config: Option<EngineConfig>,
    config_path: Option<String>,
    engine: Arc<Engine>,
    registry: Arc<WorkerRegistry>,
    running: Vec<super::reload::RunningWorker>,
}

impl EngineBuilder {
    /// Creates a new EngineBuilder with default registry
    pub fn new() -> Self {
        Self {
            config: None,
            config_path: None,
            engine: Arc::new(Engine::new()),
            registry: Arc::new(WorkerRegistry::with_inventory()),
            running: Vec::new(),
        }
    }

    pub fn engine(&self) -> &Arc<Engine> {
        &self.engine
    }

    /// Returns the currently-tracked running workers.
    pub fn running(&self) -> &[super::reload::RunningWorker] {
        &self.running
    }

    /// Mutable access to the running worker set. Intended for reload machinery
    /// that needs to swap entries in place; avoid calling from other code paths.
    pub fn running_mut(&mut self) -> &mut Vec<super::reload::RunningWorker> {
        &mut self.running
    }

    /// Returns an `Arc` handle to the shared `Engine`. Used by reload plumbing
    /// that must create workers against the live engine without consuming the
    /// builder.
    pub fn engine_handle(&self) -> Arc<Engine> {
        self.engine.clone()
    }

    /// Returns an `Arc` handle to the shared worker factory registry.
    pub fn registry_handle(&self) -> Arc<WorkerRegistry> {
        self.registry.clone()
    }

    /// Loads config strictly from file. Fails if file is missing or unparseable.
    pub fn with_config(mut self, config: EngineConfig) -> Self {
        self.config = Some(config);
        self
    }

    /// Records the path of the config file this engine was built from so that
    /// reload-time code can re-read and re-apply it. When set, `serve()` watches
    /// this file for changes and reloads automatically. When unset (e.g. running
    /// with `--use-default-config`), file watching is disabled.
    pub fn with_config_path(mut self, path: impl Into<String>) -> Self {
        self.config_path = Some(path.into());
        self
    }

    /// Returns the config file path set via [`Self::with_config_path`], or
    /// `None` if the engine is running without a file-backed config.
    pub fn config_path(&self) -> Option<&str> {
        self.config_path.as_deref()
    }

    /// Registers a custom module type in the registry
    ///
    /// This allows you to register a module implementation that can then be used
    /// via `add_worker` or in the config file.
    pub fn register_worker<M: Worker + 'static>(self, name: &str) -> Self {
        self.registry.register::<M>(name);
        self
    }

    /// Adds a worker entry
    pub fn add_worker(mut self, name: &str, config: Option<Value>) -> Self {
        if self.config.is_none() {
            self.config = Some(EngineConfig {
                modules: Vec::new(),
                workers: Vec::new(),
            });
        }

        if let Some(ref mut cfg) = self.config {
            cfg.workers.push(WorkerEntry {
                name: name.to_string(),
                image: None,
                config,
            });
        }
        self
    }

    /// Builds and initializes all modules
    pub async fn build(mut self) -> anyhow::Result<Self> {
        let config = self.config.take().expect("No worker configs found");

        crate::workers::observability::metrics::ensure_default_meter();

        let mut workers = config.workers;
        workers.extend(config.modules);

        tracing::info!("Building engine with {} workers", workers.len());
        let worker_names = workers
            .iter()
            .map(|entry| entry.name.clone())
            .collect::<HashSet<String>>();

        for registration in inventory::iter::<WorkerRegistration> {
            if registration.mandatory && !worker_names.contains(registration.name) {
                workers.push(WorkerEntry {
                    name: registration.name.to_string(),
                    image: None,
                    config: None,
                });
            }
        }

        assign_instance_ids(&mut workers);

        for entry in workers {
            tracing::debug!("Creating worker: {}", entry.name);
            let worker = entry
                .create_worker(self.engine.clone(), &self.registry)
                .await
                .map_err(|err| {
                    anyhow::anyhow!("failed to create worker '{}': {}", entry.name, err)
                })?;
            tracing::debug!("Initializing worker: {}", entry.name);
            worker.initialize().await.map_err(|err| {
                anyhow::anyhow!("failed to initialize worker '{}': {}", entry.name, err)
            })?;

            self.engine.begin_worker_scope(&entry.name);
            worker.register_functions(self.engine.clone());
            let registrations = self.engine.end_worker_scope();

            let (shutdown_tx, _) = tokio::sync::watch::channel(false);
            let worker_arc: Arc<dyn Worker> = Arc::from(worker);

            self.running.push(super::reload::RunningWorker {
                entry,
                worker: worker_arc,
                shutdown_tx,
                registrations,
            });
        }

        Ok(self)
    }

    pub async fn destroy(self) -> anyhow::Result<()> {
        tracing::warn!("Shutting down engine and destroying workers");
        for rw in self.running.iter() {
            tracing::debug!("Destroying worker: {}", rw.worker.name());
            let _ = rw.shutdown_tx.send(true);
            rw.worker.destroy().await?;
            self.engine.remove_worker_registrations(&rw.registrations);
        }
        tracing::warn!("Engine shutdown complete");
        Ok(())
    }

    /// Starts the engine server
    pub async fn serve(mut self) -> anyhow::Result<()> {
        let engine = self.engine.clone();
        let registry = self.registry.clone();
        let config_path = self.config_path.clone();

        // Lift the running workers out of `self` so we can mutably borrow them
        // inside the select loop. `self.running` is now empty; the teardown
        // at the end operates on the local `running` Vec.
        let mut running: Vec<super::reload::RunningWorker> = std::mem::take(&mut self.running);

        let (global_shutdown_tx, mut global_shutdown_rx) = tokio::sync::watch::channel(false);

        // Start background tasks for each worker. The per-worker `shutdown_rx`
        // lets the engine stop ONE worker (used by reload). The `shutdown_tx`
        // passed in is the GLOBAL tx, so when a worker like `WorkerManager`
        // catches SIGTERM/SIGINT/Ctrl+C and fires it, serve() itself unwinds.
        for rw in running.iter() {
            let shutdown_rx = rw.shutdown_tx.subscribe();
            let shutdown_tx = global_shutdown_tx.clone();
            if let Err(e) = rw
                .worker
                .start_background_tasks(shutdown_rx, shutdown_tx)
                .await
            {
                tracing::warn!(
                    worker = rw.worker.name(),
                    error = %e,
                    "Failed to start background tasks for worker"
                );
            }
        }

        // Relay global shutdown into each per-worker shutdown channel so a
        // global Ctrl+C terminates every worker (including those added via
        // reload -- those get subscribed to the relay the next time around).
        let initial_worker_shutdowns: Vec<_> =
            running.iter().map(|rw| rw.shutdown_tx.clone()).collect();
        let mut global_rx_for_relay = global_shutdown_rx.clone();
        tokio::spawn(async move {
            if global_rx_for_relay.changed().await.is_ok() && *global_rx_for_relay.borrow() {
                for tx in initial_worker_shutdowns {
                    let _ = tx.send(true);
                }
            }
        });

        // Start channel TTL sweep task
        engine
            .channel_manager
            .start_sweep_task(global_shutdown_rx.clone());

        // Set up config file watcher. When a config path is set, we watch it
        // for modifications and trigger a reload automatically. Editors often
        // write a temp file then rename, so we debounce events by 500ms to
        // coalesce rapid writes into a single reload.
        let (config_change_tx, mut config_change_rx) = tokio::sync::mpsc::channel::<()>(1);

        // Keep the watcher alive for the duration of serve().
        let _watcher = if let Some(ref path) = config_path {
            let tx = config_change_tx.clone();
            let watched_path = std::path::PathBuf::from(path);

            let mut watcher = notify::RecommendedWatcher::new(
                move |res: Result<notify::Event, notify::Error>| {
                    if let Ok(event) = res {
                        use notify::EventKind;
                        match event.kind {
                            EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_) => {
                                let _ = tx.try_send(());
                            }
                            _ => {}
                        }
                    }
                },
                notify::Config::default(),
            )?;

            // Watch the parent directory so we catch rename-based writes
            // (editors like vim write a temp file then rename it).
            // For bare filenames like "config.yaml", parent() returns ""
            // which is not a valid path — fall back to ".".
            let watch_target = watched_path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(std::path::Path::new("."));
            watcher.watch(watch_target, notify::RecursiveMode::NonRecursive)?;

            tracing::info!("reload: watching {} for changes", path);
            Some(watcher)
        } else {
            tracing::info!("reload: no config file to watch (--use-default-config)");
            None
        };

        // Drop the sender clone so config_change_rx completes when the
        // watcher's sender is the only one left (and it's dropped on exit).
        drop(config_change_tx);

        // Track fatal reload errors so we can exit with non-zero after teardown.
        let mut reload_error: Option<anyhow::Error> = None;

        loop {
            tokio::select! {
                _ = global_shutdown_rx.changed() => {
                    if *global_shutdown_rx.borrow() { break; }
                }
                Some(()) = config_change_rx.recv() => {
                    // Debounce: drain any queued events and wait 500ms for
                    // writes to settle before reloading.
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    while config_change_rx.try_recv().is_ok() {}

                    if let Err(e) = super::reload::ReloadManager::reload(
                        config_path.as_deref(),
                        engine.clone(),
                        registry.clone(),
                        &mut running,
                        global_shutdown_tx.clone(),
                    ).await {
                        reload_error = Some(e);
                        break;
                    }
                }
            }
        }

        // Fire global shutdown so the relay task stops all worker background
        // tasks (needed when the loop broke due to a reload error rather than
        // a signal-triggered shutdown).
        let _ = global_shutdown_tx.send(true);

        // Teardown -- inline version of the old `destroy()`. Operates on the
        // local `running` Vec directly so we don't have to reconstruct `self`.
        tracing::warn!("Shutting down engine and destroying workers");
        for rw in running.iter() {
            tracing::debug!("Destroying worker: {}", rw.worker.name());
            let _ = rw.shutdown_tx.send(true);
            rw.worker.destroy().await?;
            engine.remove_worker_registrations(&rw.registrations);
        }
        tracing::warn!("Engine shutdown complete");

        // Drop `global_shutdown_tx` last so the relay task unblocks.
        drop(global_shutdown_tx);

        // If the shutdown was caused by a reload failure, propagate the error
        // so the process exits with a non-zero status code.
        if let Some(e) = reload_error {
            return Err(e);
        }

        Ok(())
    }
}

impl Default for EngineBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(deprecated)]
mod tests {
    use super::*;

    #[test]
    fn test_env_var_expansion() {
        unsafe {
            env::set_var("TEST_VAR", "value1");
        }
        let input = "This is a ${TEST_VAR} and ${UNSET_VAR:default_value}";
        let expected = "This is a value1 and default_value";
        let output = EngineConfig::expand_env_vars(input);
        assert_eq!(output, expected);
    }

    #[test]
    fn test_expand_env_vars_with_default_when_var_missing() {
        unsafe {
            env::remove_var("MISSING_VAR");
        }
        let input = "Value is ${MISSING_VAR:default}";
        let expected = "Value is default";
        let output = EngineConfig::expand_env_vars(input);
        assert_eq!(output, expected);
    }

    #[test]
    fn test_expand_env_vars_existing_var_ignores_default() {
        // When var exists, default should be ignored
        unsafe {
            env::set_var("TEST_VAR_WITH_DEFAULT", "real_value");
        }
        let input = "url: ${TEST_VAR_WITH_DEFAULT:ignored_default}";
        let expected = "url: real_value";
        let output = EngineConfig::expand_env_vars(input);
        assert_eq!(output, expected);
    }

    #[test]
    fn test_expand_env_vars_no_variables_unchanged() {
        // Text without variables should remain unchanged
        let input = "plain text without any variables";
        let output = EngineConfig::expand_env_vars(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_expand_env_vars_empty_default() {
        // Explicit empty default ${VAR:} should return empty string
        unsafe {
            env::remove_var("TEST_EMPTY_DEFAULT");
        }
        let input = "value: ${TEST_EMPTY_DEFAULT:}";
        let expected = "value: ";
        let output = EngineConfig::expand_env_vars(input);
        assert_eq!(output, expected);
    }

    #[test]
    fn test_expand_env_vars_default_with_special_chars() {
        // Default containing special chars like URLs with colons
        unsafe {
            env::remove_var("TEST_REDIS_URL");
        }
        let input = "redis: ${TEST_REDIS_URL:redis://localhost:6379/0}";
        let expected = "redis: redis://localhost:6379/0";
        let output = EngineConfig::expand_env_vars(input);
        assert_eq!(output, expected);
    }

    #[test]
    fn test_expand_env_vars_multiple_same_var() {
        // Same variable used multiple times
        unsafe {
            env::set_var("TEST_REPEATED", "abc");
        }
        let input = "${TEST_REPEATED}-${TEST_REPEATED}-${TEST_REPEATED}";
        let expected = "abc-abc-abc";
        let output = EngineConfig::expand_env_vars(input);
        assert_eq!(output, expected);
    }

    #[test]
    fn test_expand_env_vars_adjacent_variables() {
        // Variables directly adjacent to each other
        unsafe {
            env::set_var("TEST_FIRST", "hello");
            env::set_var("TEST_SECOND", "world");
        }
        let input = "${TEST_FIRST}${TEST_SECOND}";
        let expected = "helloworld";
        let output = EngineConfig::expand_env_vars(input);
        assert_eq!(output, expected);
    }

    #[test]
    #[should_panic(expected = "not set and no default provided")]
    fn test_expand_env_vars_missing_var_no_default_panics() {
        // Missing var without default should panic
        unsafe {
            env::remove_var("TEST_MUST_PANIC");
        }
        let input = "key: ${TEST_MUST_PANIC}";
        EngineConfig::expand_env_vars(input);
    }

    #[test]
    fn test_expand_env_vars_var_with_underscore_and_numbers() {
        // Variable names with underscores and numbers
        unsafe {
            env::set_var("MY_VAR_123", "test_value");
        }
        let input = "value: ${MY_VAR_123}";
        let expected = "value: test_value";
        let output = EngineConfig::expand_env_vars(input);
        assert_eq!(output, expected);
    }

    #[test]
    fn test_expand_env_vars_multiline_yaml() {
        // Realistic YAML config with multiple lines
        unsafe {
            env::set_var("TEST_HOST", "localhost");
            env::set_var("TEST_PORT", "8080");
        }
        let input = r#"server:
  host: ${TEST_HOST}
  port: ${TEST_PORT}
  timeout: ${TEST_TIMEOUT:30}"#;
        let expected = r#"server:
  host: localhost
  port: 8080
  timeout: 30"#;
        let output = EngineConfig::expand_env_vars(input);
        assert_eq!(output, expected);
    }

    #[test]
    fn test_config_file_returns_error_when_file_missing() {
        let result = EngineConfig::config_file("/tmp/iii_nonexistent_config_12345.yaml");
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("Config file not found"),
            "Error should mention 'Config file not found', got: {}",
            err_msg
        );
    }

    #[test]
    fn test_config_file_loads_valid_yaml() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test_config.yaml");
        let mut file = std::fs::File::create(&path).unwrap();
        writeln!(file, "modules: []").unwrap();

        let config = EngineConfig::config_file(path.to_str().unwrap()).unwrap();
        assert!(config.modules.is_empty());
    }

    #[test]
    fn test_config_file_error_message_includes_path() {
        let path = "/tmp/iii_this_does_not_exist_67890.yaml";
        let result = EngineConfig::config_file(path);
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains(path),
            "Error should include the path '{}', got: {}",
            path,
            err_msg
        );
    }

    // =========================================================================
    // 1. expand_env_vars tests
    // =========================================================================

    #[test]
    fn test_expand_env_vars_simple() {
        // Expand a simple env var like ${HOME}
        unsafe {
            env::set_var("TEST_SIMPLE_HOME", "/home/user");
        }
        let input = "path: ${TEST_SIMPLE_HOME}";
        let output = EngineConfig::expand_env_vars(input);
        assert_eq!(output, "path: /home/user");
    }

    #[test]
    fn test_expand_env_vars_with_default() {
        // Expand ${NONEXISTENT:-default_value} should use default
        // The regex uses `:` as separator, so `:-default_value` means default = `-default_value`
        // Actually, re-examining the regex: r"\$\{([^}:]+)(?::([^}]*))?\}"
        // Group 1 = var name (everything up to : or })
        // Group 2 = everything after : up to }
        // So ${NONEXISTENT:-default_value} => var_name="NONEXISTENT", default="-default_value"
        unsafe {
            env::remove_var("TEST_EXPAND_NONEXISTENT_DEFAULT");
        }
        let input = "value: ${TEST_EXPAND_NONEXISTENT_DEFAULT:default_value}";
        let output = EngineConfig::expand_env_vars(input);
        assert_eq!(output, "value: default_value");
    }

    #[test]
    #[should_panic(expected = "not set and no default provided")]
    fn test_expand_env_vars_missing_no_default() {
        // Expand ${NONEXISTENT} without default panics
        unsafe {
            env::remove_var("TEST_EXPAND_MISSING_NODEF");
        }
        let input = "key: ${TEST_EXPAND_MISSING_NODEF}";
        EngineConfig::expand_env_vars(input);
    }

    #[test]
    fn test_expand_env_vars_multiple() {
        // Expand multiple different vars in one string
        unsafe {
            env::set_var("TEST_MULTI_A", "alpha");
            env::set_var("TEST_MULTI_B", "beta");
            env::set_var("TEST_MULTI_C", "gamma");
        }
        let input = "${TEST_MULTI_A}/${TEST_MULTI_B}/${TEST_MULTI_C}";
        let output = EngineConfig::expand_env_vars(input);
        assert_eq!(output, "alpha/beta/gamma");
    }

    #[test]
    fn test_expand_env_vars_no_vars() {
        // String without vars returns unchanged
        let input = "just a plain string with no variables at all";
        let output = EngineConfig::expand_env_vars(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_expand_env_vars_nested_in_yaml() {
        // Expand env vars in a YAML value string
        unsafe {
            env::set_var("TEST_YAML_DB_HOST", "db.example.com");
            env::set_var("TEST_YAML_DB_PORT", "5432");
        }
        let yaml_input = r#"database:
  host: ${TEST_YAML_DB_HOST}
  port: ${TEST_YAML_DB_PORT}
  name: ${TEST_YAML_DB_NAME:mydb}
  pool_size: 10"#;
        let output = EngineConfig::expand_env_vars(yaml_input);
        let expected = r#"database:
  host: db.example.com
  port: 5432
  name: mydb
  pool_size: 10"#;
        assert_eq!(output, expected);

        // Also verify the expanded YAML is actually parseable
        let parsed: serde_yaml::Value = serde_yaml::from_str(&output).unwrap();
        let db = &parsed["database"];
        assert_eq!(db["host"].as_str().unwrap(), "db.example.com");
        assert_eq!(db["port"].as_u64().unwrap(), 5432);
        assert_eq!(db["name"].as_str().unwrap(), "mydb");
        assert_eq!(db["pool_size"].as_u64().unwrap(), 10);
    }

    // =========================================================================
    // 2. default_modules tests
    // =========================================================================

    #[test]
    fn test_default_modules_returns_entries() {
        // Verify default_worker_entries returns a Vec of WorkerEntry
        let entries = default_worker_entries();
        // Each entry should have a non-empty worker name
        for entry in &entries {
            assert!(
                !entry.name.is_empty(),
                "Worker entry name should not be empty"
            );
        }
    }

    #[test]
    fn test_default_modules_keys() {
        // Verify the worker type keys are present (collected from inventory)
        let entries = default_worker_entries();
        let worker_names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();

        // We cannot know exact workers at compile time since they come from inventory,
        // but we can verify the structure is sound: no duplicates in worker names
        let unique_names: HashSet<&str> = worker_names.iter().copied().collect();
        assert_eq!(
            worker_names.len(),
            unique_names.len(),
            "Default worker entries should have unique worker names"
        );
    }

    #[test]
    fn test_default_config_includes_otel_module() {
        let config = EngineConfig::default_config();

        assert!(
            config
                .modules
                .iter()
                .any(|entry| entry.name == "iii-observability"),
            "default config should include ObservabilityWorker (registered as mandatory)"
        );
    }

    // =========================================================================
    // 3. Config parsing tests
    // =========================================================================

    #[test]
    fn test_config_yaml_parsing() {
        // Parse a minimal valid YAML config string
        let yaml = r#"
modules: []
"#;
        let config: EngineConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(config.modules.is_empty());
    }

    #[test]
    fn test_config_yaml_with_modules() {
        // Parse config listing workers under the modules key
        let yaml = r#"
modules:
  - name: "my::TestModule"
    config:
      key: "value"
      count: 42
  - name: "my::OtherModule"
"#;
        let config: EngineConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.modules.len(), 2);

        // First worker has name and config
        assert_eq!(config.modules[0].name, "my::TestModule");
        let cfg = config.modules[0].config.as_ref().unwrap();
        assert_eq!(cfg["key"], "value");
        assert_eq!(cfg["count"], 42);

        // Second worker has name but no config
        assert_eq!(config.modules[1].name, "my::OtherModule");
        assert!(config.modules[1].config.is_none());
    }

    #[test]
    fn test_config_yaml_empty() {
        // Parse empty/minimal YAML -- should use defaults
        let yaml = "{}";
        let config: EngineConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(config.modules.is_empty());
    }

    #[test]
    fn test_config_yaml_only_modules() {
        // Parse YAML with only the modules list (one worker)
        let yaml = r#"
modules:
  - name: "test::Module"
"#;
        let config: EngineConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.modules.len(), 1);
        assert_eq!(config.modules[0].name, "test::Module");
    }

    // =========================================================================
    // 4. WorkerRegistry tests
    // =========================================================================

    #[test]
    fn test_module_registry_new_is_empty() {
        // A freshly created registry (without inventory) should be empty
        let registry = WorkerRegistry::new();
        let factories = registry.worker_factories.read().expect("RwLock poisoned");
        assert!(
            factories.is_empty(),
            "New WorkerRegistry should have no registered workers"
        );
    }

    #[test]
    fn test_module_registry_register() {
        // Register a worker type and verify it exists in the registry
        use async_trait::async_trait;

        struct DummyModule;

        #[async_trait]
        impl Worker for DummyModule {
            fn name(&self) -> &'static str {
                "dummy"
            }

            async fn create(
                _engine: Arc<Engine>,
                _config: Option<Value>,
            ) -> anyhow::Result<Box<dyn Worker>> {
                Ok(Box::new(DummyModule))
            }

            async fn initialize(&self) -> anyhow::Result<()> {
                Ok(())
            }
        }

        let registry = WorkerRegistry::new();
        registry.register::<DummyModule>("test::DummyModule");

        let factories = registry.worker_factories.read().expect("RwLock poisoned");
        assert!(
            factories.contains_key("test::DummyModule"),
            "Registry should contain the registered worker"
        );
    }

    #[test]
    fn test_module_registry_contains() {
        // Check if a registered worker exists and an unregistered one does not
        use async_trait::async_trait;

        struct AnotherDummy;

        #[async_trait]
        impl Worker for AnotherDummy {
            fn name(&self) -> &'static str {
                "another_dummy"
            }

            async fn create(
                _engine: Arc<Engine>,
                _config: Option<Value>,
            ) -> anyhow::Result<Box<dyn Worker>> {
                Ok(Box::new(AnotherDummy))
            }

            async fn initialize(&self) -> anyhow::Result<()> {
                Ok(())
            }
        }

        let registry = WorkerRegistry::new();
        registry.register::<AnotherDummy>("test::AnotherDummy");

        let factories = registry.worker_factories.read().expect("RwLock poisoned");
        assert!(
            factories.contains_key("test::AnotherDummy"),
            "Registry should contain 'test::AnotherDummy'"
        );
        assert!(
            !factories.contains_key("test::NonExistent"),
            "Registry should not contain unregistered worker"
        );
    }

    #[test]
    fn test_module_registry_register_multiple() {
        // Register multiple workers and verify all are present
        use async_trait::async_trait;

        struct ModA;
        struct ModB;

        #[async_trait]
        impl Worker for ModA {
            fn name(&self) -> &'static str {
                "mod_a"
            }
            async fn create(
                _engine: Arc<Engine>,
                _config: Option<Value>,
            ) -> anyhow::Result<Box<dyn Worker>> {
                Ok(Box::new(ModA))
            }
            async fn initialize(&self) -> anyhow::Result<()> {
                Ok(())
            }
        }

        #[async_trait]
        impl Worker for ModB {
            fn name(&self) -> &'static str {
                "mod_b"
            }
            async fn create(
                _engine: Arc<Engine>,
                _config: Option<Value>,
            ) -> anyhow::Result<Box<dyn Worker>> {
                Ok(Box::new(ModB))
            }
            async fn initialize(&self) -> anyhow::Result<()> {
                Ok(())
            }
        }

        let registry = WorkerRegistry::new();
        registry.register::<ModA>("test::ModA");
        registry.register::<ModB>("test::ModB");

        let factories = registry.worker_factories.read().expect("RwLock poisoned");
        assert_eq!(factories.len(), 2);
        assert!(factories.contains_key("test::ModA"));
        assert!(factories.contains_key("test::ModB"));
    }

    // =========================================================================
    // WorkerEntry (YAML)
    // =========================================================================

    #[test]
    fn test_module_entry_deserialize() {
        let yaml = r#"
name: "my::Module"
config:
  key: "value"
"#;
        let entry: WorkerEntry = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(entry.name, "my::Module");
        assert!(entry.config.is_some());
    }

    #[test]
    fn test_module_entry_deserialize_no_config() {
        let yaml = r#"name: "my::Module""#;
        let entry: WorkerEntry = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(entry.name, "my::Module");
        assert!(entry.config.is_none());
    }

    // =========================================================================
    // EngineBuilder
    // =========================================================================

    #[test]
    fn test_engine_builder_default() {
        let builder = EngineBuilder::default();
        assert!(builder.config.is_none());
        assert!(builder.running().is_empty());
    }

    #[test]
    fn test_engine_builder_add_worker_without_config() {
        let builder = EngineBuilder::new().add_worker("test::Module", None);
        assert!(builder.config.is_some());
        let config = builder.config.unwrap();
        assert_eq!(config.workers.len(), 1);
        assert_eq!(config.workers[0].name, "test::Module");
        assert!(config.workers[0].config.is_none());
    }

    #[test]
    fn test_engine_builder_add_worker_with_config() {
        let builder = EngineBuilder::new()
            .add_worker("test::Module", Some(serde_json::json!({"key": "value"})));
        let config = builder.config.unwrap();
        assert_eq!(config.workers[0].config.as_ref().unwrap()["key"], "value");
    }

    #[test]
    fn test_engine_builder_add_multiple_modules() {
        let builder = EngineBuilder::new()
            .add_worker("test::ModA", None)
            .add_worker("test::ModB", Some(serde_json::json!({"port": 3000})));
        let config = builder.config.unwrap();
        assert_eq!(config.workers.len(), 2);
        assert_eq!(config.workers[0].name, "test::ModA");
        assert_eq!(config.workers[1].name, "test::ModB");
    }

    // =========================================================================
    // create_worker with unknown worker name
    // =========================================================================

    #[tokio::test]
    async fn test_create_worker_unknown_worker_delegates() {
        // Unknown workers are now delegated to `iii-worker start` rather than
        // returning an immediate error, so the result is Ok (an external worker
        // process wrapper) or an Err from the spawn itself — not an "Unknown
        // worker" error.
        let registry = Arc::new(WorkerRegistry::new());
        let engine = Arc::new(Engine::new());
        let result = registry
            .create_worker("nonexistent::Module", None, engine, None)
            .await;
        // If spawn succeeds we get Ok; if iii-worker binary is absent we may
        // get an Err, but it must NOT contain "Unknown worker".
        if let Err(e) = &result {
            assert!(
                !e.to_string().contains("Unknown worker"),
                "should not report 'Unknown worker'; got: {e}"
            );
        }
    }

    #[tokio::test]
    async fn test_create_worker_registered_name() {
        use async_trait::async_trait;

        struct TestMod;

        #[async_trait]
        impl Worker for TestMod {
            fn name(&self) -> &'static str {
                "test_mod"
            }
            async fn create(
                _engine: Arc<Engine>,
                _config: Option<Value>,
            ) -> anyhow::Result<Box<dyn Worker>> {
                Ok(Box::new(TestMod))
            }
            async fn initialize(&self) -> anyhow::Result<()> {
                Ok(())
            }
        }

        let registry = Arc::new(WorkerRegistry::new());
        registry.register::<TestMod>("test::TestMod");

        let engine = Arc::new(Engine::new());
        let result = registry
            .create_worker("test::TestMod", None, engine, None)
            .await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().name(), "test_mod");
    }

    // =========================================================================
    // WorkerEntry::create_worker
    // =========================================================================

    #[tokio::test]
    async fn test_module_entry_create_unknown_delegates() {
        // Unknown workers are now delegated to `iii-worker start`.  If spawn
        // fails (e.g. binary absent in CI) the error is wrapped with the
        // worker name, but it is no longer an immediate "Unknown worker" error.
        let entry = WorkerEntry {
            name: "unknown::Module".to_string(),
            image: None,
            config: None,
        };
        let registry = Arc::new(WorkerRegistry::new());
        let engine = Arc::new(Engine::new());
        let result = entry.create_worker(engine, &registry).await;
        if let Err(e) = &result {
            let msg = e.to_string();
            assert!(
                msg.contains("unknown::Module") || msg.contains("Failed to start"),
                "unexpected error message: {msg}"
            );
        }
    }

    // =========================================================================
    // EngineConfig YAML parsing edge cases
    // =========================================================================

    #[test]
    fn test_config_yaml_module_with_complex_config() {
        // Worker entry under modules with nested JSON-style config values
        let yaml = r#"
modules:
  - name: "my::Module"
    config:
      nested:
        deep: true
        items:
          - "a"
          - "b"
      number: 42
"#;
        let config: EngineConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.modules.len(), 1);
        let cfg = config.modules[0].config.as_ref().unwrap();
        assert_eq!(cfg["nested"]["deep"], true);
        assert_eq!(cfg["nested"]["items"][0], "a");
        assert_eq!(cfg["number"], 42);
    }

    // =========================================================================
    // expand_env_vars edge cases
    // =========================================================================

    #[test]
    fn test_expand_env_vars_empty_string() {
        let output = EngineConfig::expand_env_vars("");
        assert_eq!(output, "");
    }

    #[test]
    fn test_expand_env_vars_dollar_sign_without_brace() {
        let input = "price is $100";
        let output = EngineConfig::expand_env_vars(input);
        assert_eq!(output, "price is $100");
    }

    #[test]
    fn test_expand_env_vars_incomplete_syntax() {
        // ${unclosed should not match the regex
        let input = "value: ${UNCLOSED";
        let output = EngineConfig::expand_env_vars(input);
        assert_eq!(output, "value: ${UNCLOSED");
    }

    #[test]
    fn test_expand_env_vars_special_characters_in_value() {
        unsafe {
            env::set_var("TEST_SPECIAL_CHARS_VAL", "hello world!@#$%^&*()");
        }
        let input = "val: ${TEST_SPECIAL_CHARS_VAL}";
        let output = EngineConfig::expand_env_vars(input);
        assert_eq!(output, "val: hello world!@#$%^&*()");
    }

    // =========================================================================
    // WorkerRegistry register overwrites
    // =========================================================================

    #[test]
    fn test_module_registry_register_overwrite() {
        use async_trait::async_trait;

        struct ModV1;
        struct ModV2;

        #[async_trait]
        impl Worker for ModV1 {
            fn name(&self) -> &'static str {
                "v1"
            }
            async fn create(_: Arc<Engine>, _: Option<Value>) -> anyhow::Result<Box<dyn Worker>> {
                Ok(Box::new(ModV1))
            }
            async fn initialize(&self) -> anyhow::Result<()> {
                Ok(())
            }
        }

        #[async_trait]
        impl Worker for ModV2 {
            fn name(&self) -> &'static str {
                "v2"
            }
            async fn create(_: Arc<Engine>, _: Option<Value>) -> anyhow::Result<Box<dyn Worker>> {
                Ok(Box::new(ModV2))
            }
            async fn initialize(&self) -> anyhow::Result<()> {
                Ok(())
            }
        }

        let registry = WorkerRegistry::new();
        registry.register::<ModV1>("test::Overwrite");
        registry.register::<ModV2>("test::Overwrite");

        let factories = registry.worker_factories.read().expect("RwLock poisoned");
        assert_eq!(factories.len(), 1);
        assert!(factories.contains_key("test::Overwrite"));
    }

    #[tokio::test]
    async fn test_engine_builder_build_and_destroy_run_module_lifecycle() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        use async_trait::async_trait;

        static INITIALIZED: AtomicUsize = AtomicUsize::new(0);
        static REGISTERED: AtomicUsize = AtomicUsize::new(0);
        static DESTROYED: AtomicUsize = AtomicUsize::new(0);

        struct LifecycleModule;

        #[async_trait]
        impl Worker for LifecycleModule {
            fn name(&self) -> &'static str {
                "LifecycleModule"
            }

            async fn create(
                _engine: Arc<Engine>,
                _config: Option<Value>,
            ) -> anyhow::Result<Box<dyn Worker>> {
                Ok(Box::new(LifecycleModule))
            }

            async fn initialize(&self) -> anyhow::Result<()> {
                INITIALIZED.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }

            async fn destroy(&self) -> anyhow::Result<()> {
                DESTROYED.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }

            fn register_functions(&self, _engine: Arc<Engine>) {
                REGISTERED.fetch_add(1, Ordering::SeqCst);
            }
        }

        INITIALIZED.store(0, Ordering::SeqCst);
        REGISTERED.store(0, Ordering::SeqCst);
        DESTROYED.store(0, Ordering::SeqCst);

        let builder = EngineBuilder::new()
            .register_worker::<LifecycleModule>("test::Lifecycle")
            .add_worker(
                "test::Lifecycle",
                Some(serde_json::json!({"enabled": true})),
            )
            .build()
            .await
            .expect("build engine");

        assert_eq!(INITIALIZED.load(Ordering::SeqCst), 1);
        assert_eq!(REGISTERED.load(Ordering::SeqCst), 1);
        assert!(!builder.running().is_empty());

        builder.destroy().await.expect("destroy engine");
        assert_eq!(DESTROYED.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn engine_builder_reports_worker_name_on_stream_bind_failure() {
        let occupied = std::net::TcpListener::bind("127.0.0.1:0").expect("reserve port");
        let port = occupied.local_addr().expect("local addr").port();

        let err = EngineBuilder::new()
            .add_worker(
                "iii-stream",
                Some(serde_json::json!({
                    "host": "127.0.0.1",
                    "port": port,
                    "adapter": {
                        "name": "kv"
                    }
                })),
            )
            .build()
            .await
            .err()
            .expect("build should fail when the stream port is occupied");

        let message = err.to_string();
        assert!(
            message.contains("iii-stream"),
            "unexpected error message: {message}"
        );
        assert!(
            message.contains(&format!("127.0.0.1:{port}")),
            "unexpected error message: {message}"
        );
        assert!(
            message.contains("already in use"),
            "unexpected error message: {message}"
        );
    }

    #[test]
    fn test_worker_entry_with_image_field() {
        let yaml = r#"
workers:
  - name: my-worker
    image: docker.io/org/worker:latest
    config:
      port: 8080
"#;
        let config: EngineConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.workers.len(), 1);
        assert_eq!(config.workers[0].name, "my-worker");
        assert_eq!(
            config.workers[0].image.as_deref(),
            Some("docker.io/org/worker:latest")
        );
    }

    #[test]
    fn test_worker_entry_without_image_field() {
        let yaml = r#"
workers:
  - name: iii-stream
    config:
      port: 3112
"#;
        let config: EngineConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.workers[0].image, None);
    }

    #[test]
    fn test_engine_config_deserialize_workers_with_image() {
        let yaml = r#"
workers:
  - name: pdfkit
    image: ghcr.io/iii-hq/pdfkit:1.0
    config:
      timeout: 30
"#;
        let config: EngineConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.workers.len(), 1);
        assert_eq!(config.workers[0].name, "pdfkit");
        assert_eq!(
            config.workers[0].image.as_deref(),
            Some("ghcr.io/iii-hq/pdfkit:1.0")
        );
        assert!(config.workers[0].config.is_some());
    }

    #[test]
    fn test_engine_config_deserialize_legacy_modules_key() {
        let yaml = r#"
modules:
  - name: legacy-worker
"#;
        let config: EngineConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.modules.len(), 1);
        assert_eq!(config.modules[0].name, "legacy-worker");
        assert!(config.workers.is_empty());
    }

    #[test]
    fn test_engine_config_deserialize_both_modules_and_workers() {
        let yaml = r#"
modules:
  - name: builtin
workers:
  - name: external
    image: ghcr.io/org/ext:latest
"#;
        let config: EngineConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.modules.len(), 1);
        assert_eq!(config.workers.len(), 1);
    }

    #[test]
    fn test_engine_config_deserialize_empty() {
        let yaml = "modules: []\nworkers: []\n";
        let config: EngineConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(config.modules.is_empty());
        assert!(config.workers.is_empty());
    }

    #[test]
    fn test_worker_entry_allows_unknown_fields() {
        // WorkerEntry intentionally omits deny_unknown_fields so that future
        // CLI-written fields (e.g. `type: binary`) do not break older engine
        // versions.  EngineConfig itself remains strict.
        let yaml = r#"
workers:
  - name: test
    unknown_field: ignored
"#;
        let result: Result<EngineConfig, _> = serde_yaml::from_str(yaml);
        assert!(
            result.is_ok(),
            "WorkerEntry should accept unknown fields for forward compatibility"
        );
        assert_eq!(result.unwrap().workers[0].name, "test");
    }

    #[test]
    fn test_engine_config_worker_without_image() {
        let yaml = r#"
workers:
  - name: binary-worker
"#;
        let config: EngineConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.workers.len(), 1);
        assert!(config.workers[0].image.is_none());
        assert!(config.workers[0].config.is_none());
    }

    #[test]
    fn test_engine_config_with_env_var_expansion() {
        unsafe {
            env::set_var("TEST_IMAGE_TAG", "2.0.0");
        }
        let yaml = "workers:\n  - name: w\n    image: ghcr.io/org/w:${TEST_IMAGE_TAG}\n";
        let expanded = EngineConfig::expand_env_vars(yaml);
        let config: EngineConfig = serde_yaml::from_str(&expanded).unwrap();
        assert_eq!(
            config.workers[0].image.as_deref(),
            Some("ghcr.io/org/w:2.0.0")
        );
    }
}

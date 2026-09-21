// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! File-system adapter — one YAML file per configuration id.
//!
//! Layout: `<directory>/<id>.yaml` holding the entry's id/name/description,
//! `value`, and optional `metadata`. The JSON Schema is deliberately NOT
//! persisted on ordinary writes. Migration retains any available schema for
//! restart; workers re-register it on every boot. Normally the
//! disk file stays focused on the value a human edits. The adapter watches the
//! directory with `notify`
//! and surfaces external edits through the `ExternalChange` channel so the
//! worker can fire `configuration` triggers without depending on the source
//! of the change.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde_json::Value;
use tokio::sync::{Mutex as TokioMutex, RwLock};

use crate::engine::Engine;
use crate::workers::configuration::adapters::{
    ConfigurationAdapter, EnsureSupport, ExternalChange, ExternalChangeSender, RegisterKind,
    RegisterOutcome, SetOutcome,
};
use crate::workers::configuration::registry::{
    ConfigurationAdapterFuture, ConfigurationAdapterRegistration,
};
use crate::workers::configuration::structs::{
    ConfigurationEntry, ConfigurationMigrateResult, MigrateAction,
};

/// Registered name of the file-backed configuration adapter, and the default
/// adapter the configuration worker selects when none is configured. Exposed so
/// the boot-time persisted-config read (`crate::logging`) resolves the same
/// adapter/dir/extension this adapter actually uses, instead of duplicating the
/// literals and risking silent drift.
pub(crate) const ADAPTER_NAME: &str = "fs";
// `DEFAULT_DIRECTORY` / `FILE_EXTENSION` are `pub(crate)` so boot-time
// persisted-config readers (e.g. the `iii-state` boot-read) resolve the same
// on-disk location the configuration worker persists entries under.
pub(crate) const DEFAULT_DIRECTORY: &str = "./config";
pub(crate) const FILE_EXTENSION: &str = "yaml";
/// The previous default store location. When the resolved directory is the
/// current default and this legacy folder still holds entries, `new` migrates
/// them across once (a soft transition) so upgrading doesn't silently start the
/// store from empty. An explicit `directory:` override is never touched.
const LEGACY_DEFAULT_DIRECTORY: &str = "./data/configuration";

pub struct FsAdapter {
    directory: PathBuf,
    /// In-adapter cache, used by the watcher loop to diff disk state.
    /// The store holds its own cache too — they are intentionally redundant
    /// so the watcher can detect "what changed" without locking the worker.
    cache: Arc<RwLock<HashMap<String, ConfigurationEntry>>>,
    watcher: TokioMutex<Option<RecommendedWatcher>>,
    #[cfg(test)]
    migration_failure: std::sync::Mutex<Option<&'static str>>,
}

impl FsAdapter {
    pub async fn new(config: Option<Value>) -> anyhow::Result<Self> {
        let directory = config
            .as_ref()
            .and_then(|c| c.get("directory"))
            .and_then(|v| v.as_str())
            .unwrap_or(DEFAULT_DIRECTORY)
            .to_string();
        let directory = PathBuf::from(directory);
        tokio::fs::create_dir_all(&directory).await.map_err(|e| {
            anyhow::anyhow!(
                "failed to create configuration directory '{}': {}",
                directory.display(),
                e
            )
        })?;

        // Soft transition: when running on the current default location, move any
        // entries left in the legacy default folder across once. Guarded on the
        // default dir, so an explicit `directory:` override is never disturbed.
        if directory.as_path() == Path::new(DEFAULT_DIRECTORY) {
            Self::migrate_legacy_default(&directory).await;
        }

        let cache = Self::load_directory(&directory).await?;
        tracing::info!(
            directory = %directory.display(),
            entries = cache.len(),
            "FsAdapter initialised"
        );

        Ok(Self {
            directory,
            cache: Arc::new(RwLock::new(cache)),
            watcher: TokioMutex::new(None),
            #[cfg(test)]
            migration_failure: std::sync::Mutex::new(None),
        })
    }

    #[cfg(test)]
    fn fail_migration_at(&self, stage: &'static str) -> anyhow::Result<()> {
        anyhow::ensure!(
            *self.migration_failure.lock().unwrap() != Some(stage),
            "injected migration {stage} failure"
        );
        Ok(())
    }

    fn entry_path(&self, id: &str) -> PathBuf {
        self.directory.join(format!("{}.{}", id, FILE_EXTENSION))
    }

    /// One-time soft migration of `*.yaml` entries from the legacy default
    /// store location ([`LEGACY_DEFAULT_DIRECTORY`]) into `target`.
    async fn migrate_legacy_default(target: &Path) {
        Self::migrate_dir(Path::new(LEGACY_DEFAULT_DIRECTORY), target).await;
    }

    /// Move every `*.yaml` entry from `legacy` into `target`. Best-effort: a
    /// file already present in `target` is left in place (WARNING, the legacy
    /// copy is kept so the operator can reconcile); any I/O error is logged and
    /// the file is skipped. Never fails the adapter.
    async fn migrate_dir(legacy: &Path, target: &Path) {
        if legacy == target {
            return;
        }
        let mut read_dir = match tokio::fs::read_dir(legacy).await {
            Ok(rd) => rd,
            // Legacy folder absent (a fresh install) — nothing to migrate.
            Err(_) => return,
        };

        let mut moved = 0usize;
        while let Ok(Some(entry)) = read_dir.next_entry().await {
            let path = entry.path();
            if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some(FILE_EXTENSION)
            {
                continue;
            }
            let Some(name) = path.file_name() else {
                continue;
            };
            let dest = target.join(name);
            if tokio::fs::try_exists(&dest).await.unwrap_or(false) {
                tracing::warn!(
                    file = %name.to_string_lossy(),
                    location = %target.display(),
                    "Skipped migrating configuration file from the legacy '{}' folder: a file with that name already exists in the new location; the legacy copy was left untouched",
                    LEGACY_DEFAULT_DIRECTORY,
                );
                continue;
            }
            match tokio::fs::rename(&path, &dest).await {
                Ok(()) => moved += 1,
                Err(err) => tracing::warn!(
                    file = %name.to_string_lossy(),
                    error = %err,
                    "Failed to migrate configuration file from the legacy '{}' folder; leaving it in place",
                    LEGACY_DEFAULT_DIRECTORY,
                ),
            }
        }

        if moved > 0 {
            tracing::info!(
                from = %legacy.display(),
                to = %target.display(),
                count = moved,
                "The configuration store now defaults to '{}'; migrated existing entries out of the legacy '{}' folder",
                DEFAULT_DIRECTORY,
                LEGACY_DEFAULT_DIRECTORY,
            );
        }
    }

    async fn load_directory(dir: &Path) -> anyhow::Result<HashMap<String, ConfigurationEntry>> {
        let mut entries = HashMap::new();
        let mut read_dir = tokio::fs::read_dir(dir).await?;
        while let Some(dir_entry) = read_dir.next_entry().await? {
            let path = dir_entry.path();
            if !path.is_file() {
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some(FILE_EXTENSION) {
                continue;
            }
            match Self::read_entry(&path).await {
                Ok(entry) => {
                    entries.insert(entry.id.clone(), entry);
                }
                Err(err) => {
                    tracing::warn!(
                        path = %path.display(),
                        error = %err,
                        "Skipping configuration file with invalid YAML"
                    );
                }
            }
        }
        Ok(entries)
    }

    async fn read_entry(path: &Path) -> anyhow::Result<ConfigurationEntry> {
        let bytes = tokio::fs::read(path).await?;
        let entry: ConfigurationEntry = serde_yaml::from_slice(&bytes)
            .map_err(|e| anyhow::anyhow!("failed to parse {}: {}", path.display(), e))?;
        Ok(entry)
    }

    async fn write_entry(&self, entry: &ConfigurationEntry) -> anyhow::Result<()> {
        // schema omitted — see module doc; rebuilt from re-registration.
        let path = self.entry_path(&entry.id);
        let mut doc = serde_yaml::to_value(entry)
            .map_err(|e| anyhow::anyhow!("failed to serialise entry: {}", e))?;
        if let Some(map) = doc.as_mapping_mut() {
            map.remove("schema");
        }
        let yaml = serde_yaml::to_string(&doc)
            .map_err(|e| anyhow::anyhow!("failed to serialise entry: {}", e))?;
        let tmp = path.with_extension(format!("{}.tmp", FILE_EXTENSION));
        tokio::fs::write(&tmp, yaml.as_bytes()).await?;
        tokio::fs::rename(&tmp, &path).await?;
        Ok(())
    }

    /// Test helper — extract the configuration id from a file path with the
    /// adapter's `.yaml` extension. Returns `None` for paths that don't match
    /// the expected layout.
    #[cfg(test)]
    fn id_from_path(path: &Path) -> Option<String> {
        let file = path.file_name()?.to_str()?;
        let stripped = file.strip_suffix(&format!(".{}", FILE_EXTENSION))?;
        if stripped.is_empty() {
            return None;
        }
        Some(stripped.to_string())
    }
}

impl FsAdapter {
    async fn migrate_entry(
        &self,
        from_id: &str,
        to_id: &str,
        replace: bool,
    ) -> anyhow::Result<ConfigurationMigrateResult> {
        use std::io::Write;
        let mut cache = self.cache.write().await;
        let source = self.entry_path(from_id);
        let target = self.entry_path(to_id);
        // No await after taking the lock: cancellation cannot interrupt the
        // short disk/cache commit sequence. The watcher takes the same lock.
        let read = |path: &Path, id: &str| -> anyhow::Result<Option<ConfigurationEntry>> {
            let bytes = match std::fs::read(path) {
                Ok(bytes) => bytes,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(e) => return Err(e.into()),
            };
            let mut entry: ConfigurationEntry = serde_yaml::from_slice(&bytes)?;
            anyhow::ensure!(
                entry.id == id,
                "configuration filename/internal id mismatch for '{id}'"
            );
            if entry.schema.is_null()
                && let Some(cached) = cache.get(id)
            {
                entry.schema = cached.schema.clone();
            }
            Ok(Some(entry))
        };
        let prior = read(&target, to_id)?;
        if let Some(entry) = &prior
            && (!replace || from_id == to_id)
        {
            cache.insert(to_id.to_string(), entry.clone());
            return Ok(ConfigurationMigrateResult {
                action: MigrateAction::Preserved,
                entry: Some(entry.clone()),
            });
        }
        anyhow::ensure!(
            prior.is_some() || !cache.contains_key(to_id),
            "configuration destination disappeared from disk; retry after reconciliation"
        );
        let original = match std::fs::read(&source) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                anyhow::ensure!(
                    !cache.contains_key(from_id),
                    "configuration source disappeared from disk; refusing to discard cached state"
                );
                if let Some(entry) = &prior {
                    cache.insert(to_id.to_string(), entry.clone());
                }
                return Ok(ConfigurationMigrateResult {
                    action: if prior.is_some() {
                        MigrateAction::Preserved
                    } else {
                        MigrateAction::Missing
                    },
                    entry: prior,
                });
            }
            Err(e) => return Err(e.into()),
        };
        let mut entry: ConfigurationEntry = serde_yaml::from_slice(&original)?;
        anyhow::ensure!(
            entry.id == from_id,
            "configuration filename/internal id mismatch for '{from_id}'"
        );
        if entry.schema.is_null()
            && let Some(cached) = cache.get(from_id)
        {
            entry.schema = cached.schema.clone();
        }
        cache.insert(from_id.to_string(), entry.clone());
        let mut document: serde_yaml::Value = serde_yaml::from_slice(&original)?;
        let map = document
            .as_mapping_mut()
            .ok_or_else(|| anyhow::anyhow!("configuration must be a mapping"))?;
        map.insert("id".into(), to_id.into());
        // Keep unknown document fields and persist the live schema for restart.
        if !entry.schema.is_null() {
            map.insert("schema".into(), serde_yaml::to_value(&entry.schema)?);
        }
        entry.id = to_id.to_string();
        let yaml = serde_yaml::to_string(&document)?;
        let temporary = self
            .directory
            .join(format!(".migration-{}.tmp", uuid::Uuid::new_v4()));
        let persist = (|| -> anyhow::Result<()> {
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            #[cfg(test)]
            self.fail_migration_at("write")?;
            let mut file = options.open(&temporary)?;
            file.write_all(yaml.as_bytes())?;
            file.sync_all()?;
            // Unlike rename, hard_link atomically refuses an existing target.
            // Unsupported filesystems fail closed, leaving the source intact.
            #[cfg(test)]
            self.fail_migration_at("publish")?;
            if replace && prior.is_some() {
                // Keep the original target under a non-YAML extension so the
                // watcher/reload never mistakes the backup for another entry.
                let backup = self
                    .directory
                    .join(format!(".{to_id}.migration-{}.bak", uuid::Uuid::new_v4()));
                #[cfg(test)]
                self.fail_migration_at("backup")?;
                std::fs::hard_link(&target, &backup)?;
                #[cfg(unix)]
                std::fs::File::open(&self.directory)?.sync_all()?;
                #[cfg(test)]
                self.fail_migration_at("replace")?;
                std::fs::rename(&temporary, &target)?;
            } else {
                std::fs::hard_link(&temporary, &target)?;
            }
            Ok(())
        })();
        let _ = std::fs::remove_file(&temporary);
        persist?;
        cache.insert(to_id.to_string(), entry.clone());
        #[cfg(unix)]
        std::fs::File::open(&self.directory)?.sync_all()?;
        // Once target publication succeeds, any cleanup error leaves TWO valid
        // copies. Report failure (never first boot) and reconcile both caches.
        anyhow::ensure!(
            std::fs::read(&source)? == original,
            "configuration source changed during migration; both copies retained"
        );
        #[cfg(test)]
        self.fail_migration_at("delete")?;
        std::fs::remove_file(&source)?;
        cache.remove(from_id);
        Ok(ConfigurationMigrateResult {
            action: MigrateAction::Migrated,
            entry: Some(entry),
        })
    }
}

#[async_trait]
impl ConfigurationAdapter for FsAdapter {
    /// The owning store serializes local filesystem initialization with its other mutations.
    fn ensure_support(&self) -> EnsureSupport {
        // The fs adapter's on-disk store is the sole authority the local cache
        // mirrors, so the store may make the seed-vs-preserve decision itself
        // under its `write_lock` and persist it through `register`.
        EnsureSupport::Local
    }

    /// Persist a complete legacy registration and refresh the filesystem cache used for echo suppression.
    async fn register(&self, entry: ConfigurationEntry) -> anyhow::Result<RegisterOutcome> {
        // Hold the write lock across the disk write so a `write_entry`
        // failure leaves the in-memory cache untouched. Without this, a
        // failed I/O would leave readers observing a value that disappears
        // on restart.
        let mut cache = self.cache.write().await;
        let prior = cache.get(&entry.id).cloned();
        let kind = if prior.is_some() {
            RegisterKind::Replaced
        } else {
            RegisterKind::Created
        };
        self.write_entry(&entry).await?;
        cache.insert(entry.id.clone(), entry.clone());
        Ok(RegisterOutcome {
            kind,
            entry,
            old_value: prior.map(|p| p.value),
        })
    }

    async fn migrate(
        &self,
        from_id: &str,
        to_id: &str,
    ) -> anyhow::Result<ConfigurationMigrateResult> {
        self.migrate_entry(from_id, to_id, false).await
    }

    async fn migrate_replace(
        &self,
        from_id: &str,
        to_id: &str,
    ) -> anyhow::Result<ConfigurationMigrateResult> {
        self.migrate_entry(from_id, to_id, true).await
    }

    async fn set(&self, id: &str, value: Value) -> anyhow::Result<SetOutcome> {
        // Same ordering as `register` — disk first, cache second, both under
        // the same write lock. Read traffic blocks on this lock while the
        // I/O is in flight, which is acceptable for a configuration store.
        let mut cache = self.cache.write().await;
        let mut entry = cache
            .get(id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("configuration '{}' not registered", id))?;
        let old_value = Some(entry.value.clone());
        entry.value = value;
        self.write_entry(&entry).await?;
        cache.insert(id.to_string(), entry.clone());
        Ok(SetOutcome { entry, old_value })
    }

    async fn get(&self, id: &str) -> anyhow::Result<Option<ConfigurationEntry>> {
        Ok(self.cache.read().await.get(id).cloned())
    }

    async fn delete(&self, id: &str) -> anyhow::Result<Option<ConfigurationEntry>> {
        let mut cache = self.cache.write().await;
        let removed = cache.get(id).cloned();
        if removed.is_some() {
            let path = self.entry_path(id);
            match tokio::fs::remove_file(&path).await {
                Ok(()) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => {
                    return Err(anyhow::anyhow!(
                        "failed to delete configuration file '{}': {}",
                        path.display(),
                        err
                    ));
                }
            }
        }
        cache.remove(id);
        Ok(removed)
    }

    async fn list(&self) -> anyhow::Result<Vec<ConfigurationEntry>> {
        Ok(self.cache.read().await.values().cloned().collect())
    }

    async fn watch(&self, sender: ExternalChangeSender) -> anyhow::Result<()> {
        let directory = self.directory.clone();
        let cache = self.cache.clone();

        // Channel used to bridge the synchronous notify callback into our
        // async debounce loop.
        let (raw_tx, mut raw_rx) = tokio::sync::mpsc::unbounded_channel::<()>();

        let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
            if let Ok(event) = res {
                match event.kind {
                    EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_) => {
                        let _ = raw_tx.send(());
                    }
                    _ => {}
                }
            }
        })
        .map_err(|e| anyhow::anyhow!("failed to create configuration watcher: {}", e))?;
        // Watch an absolute path: the macOS FSEvents backend can silently fail
        // to deliver events for a relative path like `./config`, which is
        // exactly the default. Canonicalize (the dir always exists by now —
        // `new` created it) and fall back to the raw path only if that fails.
        let watch_path = std::fs::canonicalize(&directory).unwrap_or_else(|_| directory.clone());
        watcher
            .watch(&watch_path, RecursiveMode::NonRecursive)
            .map_err(|e| {
                anyhow::anyhow!(
                    "failed to watch configuration directory '{}': {}",
                    watch_path.display(),
                    e
                )
            })?;
        *self.watcher.lock().await = Some(watcher);
        tracing::info!(
            directory = %watch_path.display(),
            "Watching configuration directory for external edits (hot-reload enabled)"
        );

        // Debounce loop: drains every queued raw event in a 500ms window
        // and then diffs the directory snapshot against the cache, emitting
        // one ExternalChange per id that actually changed.
        tokio::spawn(async move {
            while let Some(()) = raw_rx.recv().await {
                tokio::time::sleep(Duration::from_millis(500)).await;
                while raw_rx.try_recv().is_ok() {}

                // Register/set hold this lock across their disk writes. Take it
                // before reading the directory so the snapshot and cache always
                // describe the same point in the internal write sequence.
                let mut cache_guard = cache.write().await;
                let snapshot = match Self::load_directory(&directory).await {
                    Ok(s) => s,
                    Err(err) => {
                        tracing::warn!(
                            directory = %directory.display(),
                            error = %err,
                            "Failed to read configuration directory during watch"
                        );
                        continue;
                    }
                };

                let mut events: Vec<ExternalChange> = Vec::new();

                for (id, fresh) in snapshot.iter() {
                    match cache_guard.get(id) {
                        None => {
                            // New file with no cached entry; its schema stays
                            // null until the owning worker registers it.
                            events.push(ExternalChange::Registered(fresh.clone()));
                        }
                        // `schema` is intentionally absent from this diff: it no
                        // longer lives on disk, so an external edit can't change
                        // it. Comparing it would flag every write as changed
                        // (cached real schema vs disk's null).
                        Some(existing)
                            if existing.value != fresh.value
                                || existing.name != fresh.name
                                || existing.description != fresh.description
                                || existing.metadata != fresh.metadata =>
                        {
                            // Carry the cached schema forward so the event (and
                            // the cache update below) keep the real schema rather
                            // than blanking it to the disk's null.
                            let mut entry = fresh.clone();
                            entry.schema = existing.schema.clone();
                            let old_value = Some(existing.value.clone());
                            events.push(ExternalChange::Updated { entry, old_value });
                        }
                        _ => {}
                    }
                }
                let removed_ids: Vec<String> = cache_guard
                    .keys()
                    .filter(|id| !snapshot.contains_key(*id))
                    .cloned()
                    .collect();
                for id in removed_ids {
                    if let Some(prior) = cache_guard.remove(&id) {
                        events.push(ExternalChange::Deleted { entry: prior });
                    }
                }
                for event in &events {
                    match event {
                        ExternalChange::Registered(e)
                        | ExternalChange::Updated { entry: e, .. } => {
                            cache_guard.insert(e.id.clone(), e.clone());
                        }
                        ExternalChange::Deleted { .. } => {}
                    }
                }
                drop(cache_guard);

                for event in events {
                    if sender.send(event).is_err() {
                        // Receiver dropped — stop the watcher loop.
                        return;
                    }
                }
            }
        });
        Ok(())
    }

    async fn destroy(&self) -> anyhow::Result<()> {
        // Drop the watcher so its background thread exits.
        *self.watcher.lock().await = None;
        Ok(())
    }
}

fn make_adapter(_engine: Arc<Engine>, config: Option<Value>) -> ConfigurationAdapterFuture {
    Box::pin(
        async move { Ok(Arc::new(FsAdapter::new(config).await?) as Arc<dyn ConfigurationAdapter>) },
    )
}

crate::register_adapter!(<ConfigurationAdapterRegistration> name: ADAPTER_NAME, make_adapter);

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_dir() -> tempfile::TempDir {
        tempfile::tempdir().expect("create tempdir")
    }

    fn sample_entry(id: &str) -> ConfigurationEntry {
        ConfigurationEntry {
            id: id.into(),
            name: format!("{} display", id),
            description: "test".into(),
            schema: json!({ "type": "object" }),
            value: json!({ "port": 3112 }),
            metadata: None,
        }
    }

    #[tokio::test]
    async fn migration_preserves_raw_document_and_schema_across_reload_without_echo() {
        let dir = temp_dir();
        let config = Some(json!({ "directory": dir.path() }));
        let adapter = FsAdapter::new(config.clone()).await.unwrap();
        let from = "default-harness-a14f3656efb8d5ea";
        let to = "default-harness";
        let mut entry = sample_entry(from);
        entry.value = json!({ "token": "${TOKEN}", "enabled": false, "count": 0, "empty": null });
        entry.metadata = Some(json!({ "manual": [false, 0, null, "${TOKEN}"] }));
        adapter.register(entry.clone()).await.unwrap();
        let source = adapter.entry_path(from);
        // Simulate an edit still inside the watcher's debounce window.
        let mut doc: serde_yaml::Value =
            serde_yaml::from_slice(&std::fs::read(&source).unwrap()).unwrap();
        doc["value"]["manual"] = "retained".into();
        doc["future_field"] = "retained".into();
        std::fs::write(&source, serde_yaml::to_string(&doc).unwrap()).unwrap();
        entry.value["manual"] = json!("retained");
        entry.id = to.into();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        adapter.watch(tx).await.unwrap();
        let result = adapter.migrate(from, to).await.unwrap();
        assert_eq!(result.action, MigrateAction::Migrated);
        assert_eq!(
            serde_json::to_value(result.entry.unwrap()).unwrap(),
            serde_json::to_value(&entry).unwrap()
        );
        assert!(!source.exists());
        assert!(adapter.get(from).await.unwrap().is_none());
        let target = adapter.entry_path(to);
        let bytes = std::fs::read(&target).unwrap();
        let modified = std::fs::metadata(&target).unwrap().modified().unwrap();
        let doc: serde_yaml::Value = serde_yaml::from_slice(&bytes).unwrap();
        assert_eq!(doc["future_field"], "retained");
        assert_eq!(
            adapter.migrate(from, to).await.unwrap().action,
            MigrateAction::Preserved
        );
        assert_eq!(std::fs::read(&target).unwrap(), bytes);
        assert_eq!(
            std::fs::metadata(&target).unwrap().modified().unwrap(),
            modified
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(1200), rx.recv())
                .await
                .is_err()
        );
        adapter.destroy().await.unwrap();
        let reloaded = FsAdapter::new(config).await.unwrap();
        assert_eq!(
            serde_json::to_value(reloaded.get(to).await.unwrap().unwrap()).unwrap(),
            serde_json::to_value(entry).unwrap()
        );
        assert_eq!(reloaded.list().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn replacement_preserves_source_and_destination_on_failure() {
        use crate::workers::configuration::store::ConfigurationStore;
        for stage in ["write", "publish", "backup", "replace", "delete"] {
            let dir = temp_dir();
            let adapter = Arc::new(
                FsAdapter::new(Some(json!({"directory": dir.path()})))
                    .await
                    .unwrap(),
            );
            let mut source = sample_entry("state");
            source.value = json!({"raw": "${TOKEN}", "zero": 0, "null": null});
            adapter.register(source.clone()).await.unwrap();
            adapter
                .register(sample_entry("default-state"))
                .await
                .unwrap();
            let before = std::fs::read(adapter.entry_path("default-state")).unwrap();
            let store = ConfigurationStore::new(adapter.clone());
            store.prime_from_adapter().await.unwrap();
            *adapter.migration_failure.lock().unwrap() = Some(stage);
            assert!(
                store
                    .migrate_replace("state", "default-state")
                    .await
                    .is_err()
            );
            assert_eq!(store.get("state").await.unwrap().value, source.value);
            let committed = stage == "delete";
            assert_eq!(
                store.get("default-state").await.unwrap().value,
                if committed {
                    source.value.clone()
                } else {
                    json!({"port": 3112})
                }
            );
            if !committed {
                assert_eq!(
                    std::fs::read(adapter.entry_path("default-state")).unwrap(),
                    before
                );
            }
            *adapter.migration_failure.lock().unwrap() = None;
            store
                .migrate_replace("state", "default-state")
                .await
                .unwrap();
            assert!(store.get("state").await.is_none());
            let target = adapter.entry_path("default-state");
            let bytes = std::fs::read(&target).unwrap();
            let modified = std::fs::metadata(&target).unwrap().modified().unwrap();
            let count = std::fs::read_dir(dir.path()).unwrap().count();
            assert_eq!(
                store
                    .migrate_replace("state", "default-state")
                    .await
                    .unwrap()
                    .action,
                MigrateAction::Preserved
            );
            assert_eq!(std::fs::read(&target).unwrap(), bytes);
            assert_eq!(
                std::fs::metadata(&target).unwrap().modified().unwrap(),
                modified
            );
            assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), count);
            let reloaded = FsAdapter::new(Some(json!({"directory": dir.path()})))
                .await
                .unwrap();
            assert_eq!(reloaded.list().await.unwrap().len(), 1);
            assert_eq!(
                reloaded.get("default-state").await.unwrap().unwrap().value,
                source.value
            );
        }
    }

    #[tokio::test]
    async fn migration_preserves_top_level_false_zero_and_null() {
        for value in [json!(false), json!(0), Value::Null] {
            let dir = temp_dir();
            let config = Some(json!({ "directory": dir.path() }));
            let adapter = FsAdapter::new(config.clone()).await.unwrap();
            let mut entry = sample_entry("old");
            entry.value = value.clone();
            adapter.register(entry).await.unwrap();
            assert_eq!(
                adapter
                    .migrate("old", "new")
                    .await
                    .unwrap()
                    .entry
                    .unwrap()
                    .value,
                value
            );
            let reloaded = FsAdapter::new(config).await.unwrap();
            assert_eq!(reloaded.get("new").await.unwrap().unwrap().value, value);
        }
    }

    #[tokio::test]
    async fn migration_preserves_both_files_when_target_exists_even_if_null() {
        let dir = temp_dir();
        let adapter = FsAdapter::new(Some(json!({ "directory": dir.path() })))
            .await
            .unwrap();
        adapter.register(sample_entry("old")).await.unwrap();
        let mut target = sample_entry("new");
        target.value = Value::Null;
        adapter.register(target).await.unwrap();
        let before: Vec<_> = ["old", "new"]
            .map(|id| std::fs::read(adapter.entry_path(id)).unwrap())
            .into();
        assert_eq!(
            adapter.migrate("old", "new").await.unwrap().action,
            MigrateAction::Preserved
        );
        for (id, bytes) in ["old", "new"].into_iter().zip(before) {
            assert_eq!(std::fs::read(adapter.entry_path(id)).unwrap(), bytes);
        }
        assert_eq!(
            adapter.migrate("new", "new").await.unwrap().action,
            MigrateAction::Preserved
        );
    }

    #[tokio::test]
    async fn migration_failures_leave_recoverable_data_and_truthful_caches() {
        use crate::workers::configuration::store::ConfigurationStore;
        for stage in ["write", "publish", "delete"] {
            let dir = temp_dir();
            let adapter = Arc::new(
                FsAdapter::new(Some(json!({ "directory": dir.path() })))
                    .await
                    .unwrap(),
            );
            adapter.register(sample_entry("old")).await.unwrap();
            let original = std::fs::read(adapter.entry_path("old")).unwrap();
            let store = ConfigurationStore::new(adapter.clone());
            store.prime_from_adapter().await.unwrap();
            *adapter.migration_failure.lock().unwrap() = Some(stage);
            assert!(store.migrate("old", "new").await.is_err(), "{stage}");
            assert_eq!(std::fs::read(adapter.entry_path("old")).unwrap(), original);
            assert_eq!(
                store.get("old").await.unwrap().value,
                json!({ "port": 3112 })
            );
            let published = stage == "delete";
            assert_eq!(store.get("new").await.is_some(), published);
            assert_eq!(adapter.entry_path("new").exists(), published);
            assert_eq!(
                std::fs::read_dir(dir.path()).unwrap().count(),
                if published { 2 } else { 1 }
            );
            *adapter.migration_failure.lock().unwrap() = None;
            assert_eq!(
                store.migrate("old", "new").await.unwrap().action,
                if published {
                    MigrateAction::Preserved
                } else {
                    MigrateAction::Migrated
                }
            );
        }
    }

    #[tokio::test]
    async fn migration_missing_is_a_noop_and_invalid_source_never_becomes_first_boot() {
        let dir = temp_dir();
        let adapter = FsAdapter::new(Some(json!({ "directory": dir.path() })))
            .await
            .unwrap();
        assert_eq!(
            adapter.migrate("old", "new").await.unwrap().action,
            MigrateAction::Missing
        );
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
        std::fs::write(adapter.entry_path("old"), "invalid: [").unwrap();
        assert!(adapter.migrate("old", "new").await.is_err());
        assert!(!adapter.entry_path("new").exists());
    }

    #[tokio::test]
    async fn register_and_get_round_trip() {
        let dir = temp_dir();
        let adapter = FsAdapter::new(Some(json!({ "directory": dir.path().to_str().unwrap() })))
            .await
            .unwrap();

        let entry = sample_entry("iii-stream");
        let outcome = adapter.register(entry.clone()).await.unwrap();
        assert_eq!(outcome.kind, RegisterKind::Created);
        let read = adapter.get("iii-stream").await.unwrap().unwrap();
        assert_eq!(read.value, entry.value);

        let path = dir.path().join("iii-stream.yaml");
        assert!(path.exists(), "yaml file should be created on disk");

        let contents = tokio::fs::read_to_string(&path).await.unwrap();
        assert!(
            !contents.contains("schema"),
            "schema must not be persisted to disk; got:\n{contents}"
        );
        assert!(contents.contains("port"), "value must be persisted");
    }

    #[tokio::test]
    async fn second_register_replaces_and_returns_old_value() {
        let dir = temp_dir();
        let adapter = FsAdapter::new(Some(json!({ "directory": dir.path().to_str().unwrap() })))
            .await
            .unwrap();

        adapter.register(sample_entry("iii-stream")).await.unwrap();
        let mut updated = sample_entry("iii-stream");
        updated.value = json!({ "port": 9999 });
        let outcome = adapter.register(updated.clone()).await.unwrap();
        assert_eq!(outcome.kind, RegisterKind::Replaced);
        assert_eq!(outcome.old_value, Some(json!({ "port": 3112 })));
    }

    #[tokio::test]
    async fn set_updates_value_and_returns_old() {
        let dir = temp_dir();
        let adapter = FsAdapter::new(Some(json!({ "directory": dir.path().to_str().unwrap() })))
            .await
            .unwrap();
        adapter.register(sample_entry("iii-stream")).await.unwrap();

        let outcome = adapter
            .set("iii-stream", json!({ "port": 4242 }))
            .await
            .unwrap();
        assert_eq!(outcome.old_value, Some(json!({ "port": 3112 })));
        assert_eq!(outcome.entry.value, json!({ "port": 4242 }));
    }

    #[tokio::test]
    async fn set_unknown_id_returns_error() {
        let dir = temp_dir();
        let adapter = FsAdapter::new(Some(json!({ "directory": dir.path().to_str().unwrap() })))
            .await
            .unwrap();
        let err = adapter.set("missing", json!({})).await.unwrap_err();
        assert!(err.to_string().contains("not registered"));
    }

    #[tokio::test]
    async fn delete_removes_file_and_cache() {
        let dir = temp_dir();
        let adapter = FsAdapter::new(Some(json!({ "directory": dir.path().to_str().unwrap() })))
            .await
            .unwrap();
        adapter.register(sample_entry("iii-stream")).await.unwrap();
        let removed = adapter.delete("iii-stream").await.unwrap();
        assert!(removed.is_some());
        assert!(adapter.get("iii-stream").await.unwrap().is_none());
        assert!(!dir.path().join("iii-stream.yaml").exists());
    }

    #[tokio::test]
    async fn list_returns_every_registered_entry() {
        let dir = temp_dir();
        let adapter = FsAdapter::new(Some(json!({ "directory": dir.path().to_str().unwrap() })))
            .await
            .unwrap();
        adapter.register(sample_entry("a")).await.unwrap();
        adapter.register(sample_entry("b")).await.unwrap();
        let mut listed: Vec<String> = adapter
            .list()
            .await
            .unwrap()
            .into_iter()
            .map(|e| e.id)
            .collect();
        listed.sort();
        assert_eq!(listed, vec!["a".to_string(), "b".to_string()]);
    }

    #[tokio::test]
    async fn loading_existing_directory_picks_up_yaml_files() {
        let dir = temp_dir();
        let entry = sample_entry("preexisting");
        let yaml = serde_yaml::to_string(&entry).unwrap();
        tokio::fs::write(dir.path().join("preexisting.yaml"), yaml)
            .await
            .unwrap();

        let adapter = FsAdapter::new(Some(json!({ "directory": dir.path().to_str().unwrap() })))
            .await
            .unwrap();
        let read = adapter.get("preexisting").await.unwrap().unwrap();
        assert_eq!(read.value, entry.value);
    }

    #[tokio::test]
    async fn loading_schemaless_file_defaults_schema_to_null() {
        let dir = temp_dir();
        // A value-only file (no `schema:` key) is exactly what `write_entry`
        // now produces; loading it must default schema to null, not error.
        let yaml = "id: noschema\nname: No Schema\ndescription: test\nvalue:\n  port: 3112\n";
        tokio::fs::write(dir.path().join("noschema.yaml"), yaml)
            .await
            .unwrap();

        let adapter = FsAdapter::new(Some(json!({ "directory": dir.path().to_str().unwrap() })))
            .await
            .unwrap();
        let read = adapter.get("noschema").await.unwrap().unwrap();
        assert!(
            read.schema.is_null(),
            "missing schema should default to null"
        );
        assert_eq!(read.value, json!({ "port": 3112 }));
    }

    #[tokio::test]
    async fn watcher_emits_registered_event_on_external_create() {
        let dir = temp_dir();
        let adapter = FsAdapter::new(Some(json!({ "directory": dir.path().to_str().unwrap() })))
            .await
            .unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        adapter.watch(tx).await.unwrap();

        let entry = sample_entry("external");
        let yaml = serde_yaml::to_string(&entry).unwrap();
        tokio::fs::write(dir.path().join("external.yaml"), yaml)
            .await
            .unwrap();

        let evt = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("watcher should report an event")
            .expect("channel closed");
        match evt {
            ExternalChange::Registered(e) => assert_eq!(e.id, "external"),
            other => panic!("unexpected change: {:?}", other),
        }
    }

    #[tokio::test]
    async fn watcher_carries_cached_schema_forward_on_external_update() {
        let dir = temp_dir();
        let adapter = FsAdapter::new(Some(json!({ "directory": dir.path().to_str().unwrap() })))
            .await
            .unwrap();
        // Register so the adapter cache holds the real schema; the file on disk
        // is value-only (write_entry drops schema).
        adapter.register(sample_entry("watched")).await.unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        adapter.watch(tx).await.unwrap();

        // External edit changing only the value, in the value-only on-disk format.
        let edited =
            "id: watched\nname: watched display\ndescription: test\nvalue:\n  port: 9999\n";
        tokio::fs::write(dir.path().join("watched.yaml"), edited)
            .await
            .unwrap();

        let evt = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("watcher should report an event")
            .expect("channel closed");
        match evt {
            ExternalChange::Updated { entry, old_value } => {
                assert_eq!(entry.value, json!({ "port": 9999 }));
                assert_eq!(old_value, Some(json!({ "port": 3112 })));
                // Schema is absent on disk but must be carried forward from cache,
                // not blanked to null — and the value edit must fire exactly one
                // Updated event (schema is no longer part of the diff).
                assert_eq!(
                    entry.schema,
                    json!({ "type": "object" }),
                    "cached schema must survive an external value edit"
                );
            }
            other => panic!("expected Updated, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn internal_set_does_not_echo_through_watcher() {
        // An internal `set` writes the file AND updates the adapter cache, so
        // the watcher's diff sees disk == cache and emits nothing. This is what
        // stops a save → reload → save loop. (An *external* edit, where the
        // cache is stale relative to disk, still fires — see the tests above.)
        let dir = temp_dir();
        let adapter = FsAdapter::new(Some(json!({ "directory": dir.path().to_str().unwrap() })))
            .await
            .unwrap();
        adapter.register(sample_entry("looptest")).await.unwrap();

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        adapter.watch(tx).await.unwrap();

        adapter
            .set("looptest", json!({ "port": 4242 }))
            .await
            .unwrap();

        // Well past the 500ms debounce: the self-write must not surface as an
        // external change.
        let echoed = tokio::time::timeout(Duration::from_millis(1500), rx.recv()).await;
        assert!(
            echoed.is_err(),
            "an internal set must not echo back through the watcher (would loop)"
        );
    }

    #[tokio::test]
    async fn internal_write_during_watcher_reconcile_does_not_restore_stale_snapshot() {
        let dir = temp_dir();
        let adapter = FsAdapter::new(Some(json!({ "directory": dir.path().to_str().unwrap() })))
            .await
            .unwrap();
        adapter.register(sample_entry("race")).await.unwrap();

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        adapter.watch(tx).await.unwrap();

        // Keep the cache locked while the watcher handles a file event. The
        // watcher must take this lock before reading the directory; otherwise,
        // it can retain the old disk value and restore it after this simulated
        // internal write updates both disk and cache.
        let mut cache = adapter.cache.write().await;
        let current = cache.get("race").unwrap().clone();
        adapter.write_entry(&current).await.unwrap();
        tokio::time::sleep(Duration::from_millis(750)).await;

        let mut updated = current;
        updated.value = json!({ "port": 4242 });
        adapter.write_entry(&updated).await.unwrap();
        cache.insert(updated.id.clone(), updated);
        drop(cache);

        let echoed = tokio::time::timeout(Duration::from_millis(1500), rx.recv()).await;
        assert!(
            echoed.is_err(),
            "the watcher must not restore a snapshot read before an internal write"
        );
    }

    #[tokio::test]
    async fn migrate_dir_moves_yaml_entries_and_ignores_others() {
        let legacy = temp_dir();
        let target = temp_dir();
        let stream = "id: iii-stream\nvalue:\n  port: 1\n";
        tokio::fs::write(legacy.path().join("iii-stream.yaml"), stream)
            .await
            .unwrap();
        tokio::fs::write(legacy.path().join("iii-http.yaml"), "id: iii-http\n")
            .await
            .unwrap();
        // A non-yaml file must be left behind.
        tokio::fs::write(legacy.path().join("notes.txt"), "ignore me")
            .await
            .unwrap();

        FsAdapter::migrate_dir(legacy.path(), target.path()).await;

        assert!(target.path().join("iii-stream.yaml").exists());
        assert!(target.path().join("iii-http.yaml").exists());
        assert_eq!(
            tokio::fs::read_to_string(target.path().join("iii-stream.yaml"))
                .await
                .unwrap(),
            stream,
            "content must be preserved across the move"
        );
        // Moved, not copied.
        assert!(!legacy.path().join("iii-stream.yaml").exists());
        assert!(!legacy.path().join("iii-http.yaml").exists());
        // Non-yaml left untouched in the legacy folder.
        assert!(!target.path().join("notes.txt").exists());
        assert!(legacy.path().join("notes.txt").exists());
    }

    #[tokio::test]
    async fn migrate_dir_skips_conflicts_and_keeps_legacy_copy() {
        let legacy = temp_dir();
        let target = temp_dir();
        tokio::fs::write(
            legacy.path().join("iii-stream.yaml"),
            "id: iii-stream\nvalue:\n  port: 1\n",
        )
        .await
        .unwrap();
        tokio::fs::write(legacy.path().join("iii-http.yaml"), "id: iii-http\n")
            .await
            .unwrap();
        // The new location already has a file with the same name (different
        // content): the migration of THAT file must be skipped (WARNING).
        let existing = "id: iii-stream\nvalue:\n  port: 999\n";
        tokio::fs::write(target.path().join("iii-stream.yaml"), existing)
            .await
            .unwrap();

        FsAdapter::migrate_dir(legacy.path(), target.path()).await;

        // Conflict: target keeps its own content; legacy copy is left in place.
        assert_eq!(
            tokio::fs::read_to_string(target.path().join("iii-stream.yaml"))
                .await
                .unwrap(),
            existing,
            "a conflicting file must not be overwritten"
        );
        assert!(
            legacy.path().join("iii-stream.yaml").exists(),
            "the conflicting legacy copy is kept for the operator to reconcile"
        );
        // The non-conflicting file still migrates.
        assert!(target.path().join("iii-http.yaml").exists());
        assert!(!legacy.path().join("iii-http.yaml").exists());
    }

    #[tokio::test]
    async fn migrate_dir_is_a_noop_when_legacy_absent() {
        let target = temp_dir();
        let legacy = target.path().join("nonexistent-legacy");
        // Must not panic or create anything.
        FsAdapter::migrate_dir(&legacy, target.path()).await;
        assert!(
            tokio::fs::read_dir(target.path())
                .await
                .unwrap()
                .next_entry()
                .await
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn id_from_path_extracts_stem() {
        assert_eq!(
            FsAdapter::id_from_path(Path::new("/tmp/iii-stream.yaml")),
            Some("iii-stream".to_string())
        );
        assert_eq!(FsAdapter::id_from_path(Path::new("/tmp/no_ext")), None);
        assert_eq!(FsAdapter::id_from_path(Path::new("/tmp/.yaml")), None);
    }
}

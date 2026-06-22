// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! File-system adapter — one YAML file per configuration id.
//!
//! Layout: `<directory>/<id>.yaml` holding the entry's id/name/description,
//! `value`, and optional `metadata`. The JSON Schema is deliberately NOT
//! persisted — it is large and workers re-register it on every boot, so the
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
    ConfigurationAdapter, ExternalChange, ExternalChangeSender, RegisterKind, RegisterOutcome,
    SetOutcome,
};
use crate::workers::configuration::registry::{
    ConfigurationAdapterFuture, ConfigurationAdapterRegistration,
};
use crate::workers::configuration::structs::ConfigurationEntry;

/// Registered name of the file-backed configuration adapter, and the default
/// adapter the configuration worker selects when none is configured. Exposed so
/// the boot-time persisted-config read (`crate::logging`) resolves the same
/// adapter/dir/extension this adapter actually uses, instead of duplicating the
/// literals and risking silent drift.
pub(crate) const ADAPTER_NAME: &str = "fs";
// `DEFAULT_DIRECTORY` / `FILE_EXTENSION` are `pub(crate)` so boot-time
// persisted-config readers (e.g. the `iii-state` boot-read) resolve the same
// on-disk location the configuration worker persists entries under.
pub(crate) const DEFAULT_DIRECTORY: &str = "./data/configuration";
pub(crate) const FILE_EXTENSION: &str = "yaml";

pub struct FsAdapter {
    directory: PathBuf,
    /// In-adapter cache, used by the watcher loop to diff disk state.
    /// The store holds its own cache too — they are intentionally redundant
    /// so the watcher can detect "what changed" without locking the worker.
    cache: Arc<RwLock<HashMap<String, ConfigurationEntry>>>,
    watcher: TokioMutex<Option<RecommendedWatcher>>,
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
        })
    }

    fn entry_path(&self, id: &str) -> PathBuf {
        self.directory.join(format!("{}.{}", id, FILE_EXTENSION))
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

#[async_trait]
impl ConfigurationAdapter for FsAdapter {
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
        let removed = self.cache.write().await.remove(id);
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
        watcher
            .watch(&directory, RecursiveMode::NonRecursive)
            .map_err(|e| {
                anyhow::anyhow!(
                    "failed to watch configuration directory '{}': {}",
                    directory.display(),
                    e
                )
            })?;
        *self.watcher.lock().await = Some(watcher);

        // Debounce loop: drains every queued raw event in a 500ms window
        // and then diffs the directory snapshot against the cache, emitting
        // one ExternalChange per id that actually changed.
        tokio::spawn(async move {
            while let Some(()) = raw_rx.recv().await {
                tokio::time::sleep(Duration::from_millis(500)).await;
                while raw_rx.try_recv().is_ok() {}

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

                // Diff under the cache write lock so concurrent register/set
                // calls don't race the watcher.
                let mut cache_guard = cache.write().await;
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
                                || existing.description != fresh.description =>
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

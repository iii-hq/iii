// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Project lock for registry-backed compose workers.
//!
//! The compose file keeps the operator's selector, such as `next`. This file
//! keeps the concrete registry result and is the only source used by normal
//! starts. A selected entry is resolved again only when a declaration changes
//! or `compose::update` forces it.

use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    path::{Path, PathBuf},
};

use futures::StreamExt;
use serde::{Deserialize, Serialize};

use crate::{
    config::{ComposeFile, WorkerSource},
    error::{ComposeError, Result},
    registry::ResolvedPackage,
};

const LOCKFILE_VERSION: u8 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ComposeLock {
    version: u8,
    containers: BTreeMap<String, LockedContainer>,
    /// Declared graph members selected for each explicit registry root.
    ///
    /// This provenance lets update replace one resolved graph and remove only
    /// generated dependencies that no remaining root owns.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    graphs: BTreeMap<String, BTreeSet<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LockedContainer {
    worker: String,
    requested: String,
    resolved: ResolvedPackage,
}

/// A complete lock candidate with optional artifact acquisition results.
/// Writing stays separate so callers can validate all other file changes first.
pub struct PreparedLock {
    path: PathBuf,
    lock: ComposeLock,
    changed: bool,
    package_changes: BTreeSet<String>,
    install_statuses: BTreeMap<String, crate::registry::InstallStatus>,
}

impl PreparedLock {
    /// Writes the lock atomically when its serialized state changed.
    pub fn write_if_changed(&self) -> Result<()> {
        if !self.changed {
            return Ok(());
        }
        let text =
            serde_yaml::to_string(&self.lock).map_err(|error| ComposeError::InvalidLock {
                path: self.path.clone(),
                message: error.to_string(),
            })?;
        write_atomically(&self.path, &text)
    }

    /// True when a worker would execute different resolved package content.
    pub fn package_changed(&self, container: &str) -> bool {
        self.package_changes.contains(container)
    }

    /// Concrete version selected for one container.
    pub fn resolved_version(&self, container: &str) -> Option<&str> {
        self.lock
            .containers
            .get(container)
            .map(|entry| entry.resolved.version.as_str())
    }

    /// Whether the lock itself changed, including selector-only changes.
    pub fn changed(&self) -> bool {
        self.changed
    }

    /// Cache result for each package acquired while preparing this lock.
    pub fn install_statuses(&self) -> &BTreeMap<String, crate::registry::InstallStatus> {
        &self.install_statuses
    }

    /// Records the complete package graph selected for one explicit root.
    pub fn replace_graph(&mut self, root: &str, nodes: BTreeSet<String>) {
        if self.lock.graphs.get(root) == Some(&nodes) {
            return;
        }
        self.changed = true;
        if nodes.is_empty() {
            self.lock.graphs.remove(root);
        } else {
            self.lock.graphs.insert(root.to_string(), nodes);
        }
    }

    /// Acquires every resolved artifact with the Compose concurrency limit.
    async fn install(&mut self, cache_root: &Path) -> Result<()> {
        let requests = self
            .lock
            .containers
            .iter()
            .map(|(key, entry)| (key.clone(), entry.resolved.clone()))
            .collect::<Vec<_>>();
        let cache_root = cache_root.to_path_buf();
        let mut installs = futures::stream::iter(requests.into_iter().map(|(key, resolved)| {
            let cache_root = cache_root.clone();
            async move {
                let result = crate::registry::install_resolved(&key, &resolved, &cache_root).await;
                (key, result)
            }
        }))
        .buffer_unordered(crate::parallelism::max_parallel_workers());

        while let Some((key, result)) = installs.next().await {
            self.install_statuses.insert(key, result?.status);
        }
        Ok(())
    }
}

/// Attaches matching package metadata from an existing lock without resolving
/// selectors or acquiring artifacts. Missing and stale entries stay detached
/// until an operation that can create a lock prepares them.
pub fn attach(compose: &mut ComposeFile) -> Result<()> {
    let Some(lock) = load(&lock_path(&compose.path))? else {
        return Ok(());
    };
    for (key, container) in &mut compose.containers {
        let WorkerSource::Package { reference } = &container.worker else {
            continue;
        };
        let requested = container.version.as_deref().unwrap_or("*");
        let worker = format!("package://{reference}");
        if let Some(entry) = lock
            .containers
            .get(key)
            .filter(|entry| entry.worker == worker && entry.requested == requested)
        {
            container.resolved_package = Some(entry.resolved.clone());
        }
    }
    Ok(())
}

/// Resolves missing, changed, or explicitly forced declarations, verifies the
/// selected artifacts, and attaches the immutable result to the runtime model.
pub async fn prepare(
    compose: &mut ComposeFile,
    cache_root: &Path,
    force: &BTreeSet<String>,
) -> Result<PreparedLock> {
    prepare_with_versions(compose, cache_root, force, &BTreeMap::new()).await
}

/// Prepares a lock while using exact versions already selected by one registry
/// graph. This keeps `compose::add worker=name@next` on the same graph result
/// even if the tag moves before its artifacts are acquired.
pub async fn prepare_with_versions(
    compose: &mut ComposeFile,
    cache_root: &Path,
    force: &BTreeSet<String>,
    selected_versions: &BTreeMap<String, String>,
) -> Result<PreparedLock> {
    let mut prepared = prepare_metadata_with_versions(compose, force, selected_versions).await?;
    prepared.install(cache_root).await?;
    Ok(prepared)
}

/// Uses only the exact resolution in an existing lock.
///
/// This is the Compose equivalent of `npm ci`: it never resolves a selector
/// and never changes the lock. A missing cache entry is still downloaded from
/// the immutable URL and digest already recorded in the lock.
pub async fn prepare_frozen(compose: &mut ComposeFile, cache_root: &Path) -> Result<PreparedLock> {
    let path = lock_path(&compose.path);
    let lock =
        load(&path)?.ok_or_else(|| ComposeError::FrozenLockMissing { path: path.clone() })?;
    let declarations = package_declarations(compose);

    for (key, reference, requested) in &declarations {
        let worker = format!("package://{reference}");
        let Some(entry) = lock.containers.get(key) else {
            return Err(ComposeError::FrozenLockOutOfDate {
                path,
                message: format!("container '{key}' is missing from the lock"),
            });
        };
        if entry.worker != worker || entry.requested != *requested {
            return Err(ComposeError::FrozenLockOutOfDate {
                path,
                message: format!(
                    "container '{key}' changed from {}@{} to package://{reference}@{requested}",
                    entry.worker, entry.requested,
                ),
            });
        }
    }
    if let Some(extra) = lock
        .containers
        .keys()
        .find(|key| !declarations.iter().any(|(declared, _, _)| declared == *key))
    {
        return Err(ComposeError::FrozenLockOutOfDate {
            path,
            message: format!("container '{extra}' exists only in the lock"),
        });
    }

    for (key, _, _) in &declarations {
        if let (Some(container), Some(entry)) =
            (compose.containers.get_mut(key), lock.containers.get(key))
        {
            container.resolved_package = Some(entry.resolved.clone());
        }
    }
    let mut prepared = PreparedLock {
        path,
        lock,
        changed: false,
        package_changes: BTreeSet::new(),
        install_statuses: BTreeMap::new(),
    };
    prepared.install(cache_root).await?;
    Ok(prepared)
}

/// Returns package graph ownership recorded beside one compose file.
pub(crate) fn graphs(compose_path: &Path) -> Result<BTreeMap<String, BTreeSet<String>>> {
    Ok(load(&lock_path(compose_path))?
        .map(|lock| lock.graphs)
        .unwrap_or_default())
}

/// Resolves and attaches lock metadata without acquiring package artifacts.
/// Removal uses this to prune the lock before it stops existing workers.
pub async fn prepare_metadata(
    compose: &mut ComposeFile,
    force: &BTreeSet<String>,
) -> Result<PreparedLock> {
    prepare_metadata_with_versions(compose, force, &BTreeMap::new()).await
}

/// Builds a lock candidate and attaches its resolved packages to the runtime
/// model. Artifact acquisition is a separate step for operations that need it.
async fn prepare_metadata_with_versions(
    compose: &mut ComposeFile,
    force: &BTreeSet<String>,
    selected_versions: &BTreeMap<String, String>,
) -> Result<PreparedLock> {
    let path = lock_path(&compose.path);
    let previous = load(&path)?;
    let mut containers = BTreeMap::new();
    let mut package_changes = BTreeSet::new();

    let declarations = package_declarations(compose);

    for (key, reference, requested) in declarations {
        let worker = format!("package://{reference}");
        let reusable = previous
            .as_ref()
            .and_then(|lock| lock.containers.get(&key))
            .filter(|entry| {
                !force.contains(&key) && entry.worker == worker && entry.requested == requested
            });
        let entry = match reusable {
            Some(entry) => entry.clone(),
            None => LockedContainer {
                worker,
                requested: requested.clone(),
                resolved: crate::registry::resolve_package(
                    &key,
                    &reference,
                    selected_versions
                        .get(&key)
                        .map(String::as_str)
                        .unwrap_or(&requested),
                )
                .await?,
            },
        };

        if previous
            .as_ref()
            .and_then(|lock| lock.containers.get(&key))
            .is_none_or(|old| runtime_package_changed(&old.resolved, &entry.resolved))
        {
            package_changes.insert(key.clone());
        }
        if let Some(container) = compose.containers.get_mut(&key) {
            container.resolved_package = Some(entry.resolved.clone());
        }
        containers.insert(key, entry);
    }

    let declared = compose.containers.keys().cloned().collect::<BTreeSet<_>>();
    let graphs = previous
        .as_ref()
        .map(|lock| {
            lock.graphs
                .iter()
                .filter(|(root, _)| containers.contains_key(*root))
                .filter_map(|(root, nodes)| {
                    let nodes = nodes
                        .iter()
                        .filter(|node| declared.contains(*node))
                        .cloned()
                        .collect::<BTreeSet<_>>();
                    (!nodes.is_empty()).then(|| (root.clone(), nodes))
                })
                .collect()
        })
        .unwrap_or_default();
    let lock = ComposeLock {
        version: LOCKFILE_VERSION,
        containers,
        graphs,
    };
    let changed = previous.as_ref() != Some(&lock);
    Ok(PreparedLock {
        path,
        lock,
        changed,
        package_changes,
        install_statuses: BTreeMap::new(),
    })
}

fn package_declarations(compose: &ComposeFile) -> Vec<(String, String, String)> {
    compose
        .containers
        .iter()
        .filter_map(|(key, container)| match &container.worker {
            WorkerSource::Package { reference } => Some((
                key.clone(),
                reference.clone(),
                container.version.clone().unwrap_or_else(|| "*".to_string()),
            )),
            WorkerSource::Path { .. } => None,
        })
        .collect()
}

/// Compares the package fields that can change worker runtime behavior.
fn runtime_package_changed(previous: &ResolvedPackage, next: &ResolvedPackage) -> bool {
    let target = crate::registry::host_target();
    previous.kind != next.kind
        || previous.default_config != next.default_config
        || previous
            .artifacts
            .get(target)
            .map(|artifact| artifact.sha256.to_ascii_lowercase())
            != next
                .artifacts
                .get(target)
                .map(|artifact| artifact.sha256.to_ascii_lowercase())
}

/// The lock sits beside its compose file and replaces the YAML extension.
pub fn lock_path(compose_path: &Path) -> PathBuf {
    compose_path.with_extension("lock")
}

/// Reads and validates a lock, treating a missing file as an unlocked project.
fn load(path: &Path) -> Result<Option<ComposeLock>> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(ComposeError::Io {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    let lock: ComposeLock =
        serde_yaml::from_str(&text).map_err(|error| ComposeError::InvalidLock {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;
    validate(path, &lock)?;
    Ok(Some(lock))
}

/// Validates all untrusted lock fields before they reach the cache or runtime.
fn validate(path: &Path, lock: &ComposeLock) -> Result<()> {
    let invalid = |message: String| ComposeError::InvalidLock {
        path: path.to_path_buf(),
        message,
    };
    if lock.version != LOCKFILE_VERSION {
        return Err(invalid(format!(
            "unsupported version {}; expected {LOCKFILE_VERSION}",
            lock.version
        )));
    }
    for (container, entry) in &lock.containers {
        let Some(reference) = entry.worker.strip_prefix("package://") else {
            return Err(invalid(format!(
                "container '{container}' worker must start with package://"
            )));
        };
        let expected_name = reference.rsplit('/').next().unwrap_or(reference);
        if entry.requested.trim().is_empty() {
            return Err(invalid(format!(
                "container '{container}' has an empty requested version"
            )));
        }
        if entry.resolved.name != expected_name {
            return Err(invalid(format!(
                "container '{container}' resolved '{}', expected '{expected_name}'",
                entry.resolved.name
            )));
        }
        if !crate::registry::is_path_safe(&entry.resolved.name) {
            return Err(invalid(format!(
                "container '{container}' has an unsafe resolved package name"
            )));
        }
        if !crate::registry::is_path_safe(&entry.resolved.version) {
            return Err(invalid(format!(
                "container '{container}' has an unsafe resolved version"
            )));
        }
        if !matches!(entry.resolved.kind.as_str(), "binary" | "bundle") {
            return Err(invalid(format!(
                "container '{container}' has unsupported package type '{}'",
                entry.resolved.kind
            )));
        }
        if entry.resolved.artifacts.is_empty() {
            return Err(invalid(format!("container '{container}' has no artifacts")));
        }
        for (target, artifact) in &entry.resolved.artifacts {
            if target.trim().is_empty() {
                return Err(invalid(format!(
                    "container '{container}' has an artifact with no target"
                )));
            }
            if artifact.url.trim().is_empty() {
                return Err(invalid(format!(
                    "container '{container}' artifact '{target}' has no URL"
                )));
            }
            if artifact.sha256.len() != 64
                || !artifact.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                return Err(invalid(format!(
                    "container '{container}' artifact '{target}' has an invalid SHA-256"
                )));
            }
        }
    }
    for root in lock.graphs.keys() {
        if !lock.containers.contains_key(root) {
            return Err(invalid(format!(
                "graph root '{root}' is not a locked package container"
            )));
        }
    }
    Ok(())
}

/// Replaces a lock only after its complete contents are durable in a temp file.
fn write_atomically(path: &Path, text: &str) -> Result<()> {
    let temp = path.with_extension(format!("lock-{}.tmp", uuid::Uuid::new_v4()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o644);
    }
    let mut file = options.open(&temp).map_err(|source| ComposeError::Io {
        path: temp.clone(),
        source,
    })?;
    if let Err(source) = file
        .write_all(text.as_bytes())
        .and_then(|()| file.sync_all())
    {
        drop(file);
        let _ = std::fs::remove_file(&temp);
        return Err(ComposeError::Io { path: temp, source });
    }
    drop(file);
    std::fs::rename(&temp, path).map_err(|source| {
        let _ = std::fs::remove_file(&temp);
        ComposeError::Io {
            path: path.to_path_buf(),
            source,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::RegistryArtifact;
    use sha2::{Digest, Sha256};
    use wiremock::{Mock, MockServer, ResponseTemplate, matchers};

    fn lock() -> ComposeLock {
        ComposeLock {
            version: 1,
            containers: BTreeMap::from([(
                "state".to_string(),
                LockedContainer {
                    worker: "package://api.workers.iii.dev/state".to_string(),
                    requested: "next".to_string(),
                    resolved: ResolvedPackage {
                        name: "state".to_string(),
                        version: "0.22.8".to_string(),
                        kind: "binary".to_string(),
                        artifacts: BTreeMap::from([(
                            crate::registry::host_target().to_string(),
                            RegistryArtifact {
                                url: "https://example.com/state.tar.gz".to_string(),
                                sha256: "a".repeat(64),
                            },
                        )]),
                        default_config: None,
                    },
                },
            )]),
            graphs: BTreeMap::from([("state".to_string(), BTreeSet::from(["state".to_string()]))]),
        }
    }

    #[test]
    fn lock_path_replaces_the_compose_extension() {
        assert_eq!(
            lock_path(Path::new("config/worker-compose.yaml")),
            PathBuf::from("config/worker-compose.lock")
        );
    }

    #[test]
    fn lock_round_trip_preserves_the_requested_tag() {
        let expected = lock();
        let yaml = serde_yaml::to_string(&expected).unwrap();

        let actual: ComposeLock = serde_yaml::from_str(&yaml).unwrap();

        assert_eq!(actual, expected);
    }

    #[test]
    fn validate_rejects_a_changed_artifact_digest() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("worker-compose.lock");
        let mut lock = lock();
        lock.containers
            .get_mut("state")
            .unwrap()
            .resolved
            .artifacts
            .values_mut()
            .next()
            .unwrap()
            .sha256 = "not-a-digest".to_string();

        let error = validate(&path, &lock).unwrap_err();

        assert_eq!(error.code(), "INVALID_COMPOSE_LOCK");
    }

    #[test]
    fn atomic_write_leaves_a_parseable_lock() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("worker-compose.lock");
        let yaml = serde_yaml::to_string(&lock()).unwrap();

        write_atomically(&path, &yaml).unwrap();

        assert_eq!(load(&path).unwrap(), Some(lock()));
    }

    #[test]
    fn attach_uses_locked_metadata_without_acquiring_the_artifact() {
        let dir = tempfile::tempdir().unwrap();
        let compose_path = dir.path().join("worker-compose.yaml");
        std::fs::write(
            &compose_path,
            "containers:\n  state:\n    worker: package://api.workers.iii.dev/state\n    version: next\n",
        )
        .unwrap();
        let lock_path = lock_path(&compose_path);
        write_atomically(&lock_path, &serde_yaml::to_string(&lock()).unwrap()).unwrap();
        let mut compose = ComposeFile::load(&compose_path).unwrap();

        attach(&mut compose).unwrap();

        let resolved = compose
            .containers
            .get("state")
            .unwrap()
            .resolved_package
            .as_ref()
            .unwrap();
        assert_eq!(resolved.version, "0.22.8");
        assert!(!dir.path().join("cache").exists());
    }

    #[tokio::test]
    async fn frozen_prepare_requires_an_existing_lock() {
        let dir = tempfile::tempdir().unwrap();
        let compose_path = dir.path().join("worker-compose.yaml");
        std::fs::write(
            &compose_path,
            "containers:\n  local:\n    worker: path://./local\n    scripts: { run: ./start }\n",
        )
        .unwrap();
        let mut compose = ComposeFile::load(&compose_path).unwrap();

        let error = match prepare_frozen(&mut compose, &dir.path().join("cache")).await {
            Ok(_) => panic!("frozen mode must not create a lock"),
            Err(error) => error,
        };

        assert_eq!(error.code(), "COMPOSE_LOCK_REQUIRED");
        assert!(!lock_path(&compose_path).exists());
    }

    #[tokio::test]
    async fn frozen_prepare_rejects_a_changed_selector_without_writing() {
        let dir = tempfile::tempdir().unwrap();
        let compose_path = dir.path().join("worker-compose.yaml");
        std::fs::write(
            &compose_path,
            "containers:\n  state:\n    worker: package://api.workers.iii.dev/state\n    version: latest\n",
        )
        .unwrap();
        let lock_path = lock_path(&compose_path);
        let before = serde_yaml::to_string(&lock()).unwrap();
        write_atomically(&lock_path, &before).unwrap();
        let mut compose = ComposeFile::load(&compose_path).unwrap();

        let error = match prepare_frozen(&mut compose, &dir.path().join("cache")).await {
            Ok(_) => panic!("frozen mode must reject a stale lock"),
            Err(error) => error,
        };

        assert_eq!(error.code(), "COMPOSE_LOCK_OUT_OF_DATE");
        assert_eq!(std::fs::read_to_string(lock_path).unwrap(), before);
    }

    #[test]
    fn a_new_artifact_url_with_the_same_digest_does_not_change_runtime_content() {
        let previous = lock().containers.remove("state").unwrap().resolved;
        let mut next = previous.clone();
        next.artifacts.values_mut().next().unwrap().url =
            "https://mirror.example.com/state.tar.gz".to_string();

        assert!(!runtime_package_changed(&previous, &next));
    }

    #[tokio::test]
    async fn prepare_reuses_a_locked_tag_without_registry_resolution() {
        let server = MockServer::start().await;
        let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut archive = tar::Builder::new(encoder);
        let body = b"#!/bin/sh\nexit 0\n";
        let mut header = tar::Header::new_gnu();
        header.set_size(body.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        archive
            .append_data(
                &mut header,
                format!("worker{}", std::env::consts::EXE_SUFFIX),
                &body[..],
            )
            .unwrap();
        let archive = archive.into_inner().unwrap().finish().unwrap();
        let digest = hex::encode(Sha256::digest(&archive));
        Mock::given(matchers::method("GET"))
            .and(matchers::path("/artifact"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(archive))
            .expect(1)
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let compose_path = dir.path().join("worker-compose.yaml");
        std::fs::write(
            &compose_path,
            "containers:\n  state:\n    worker: package://api.workers.iii.dev/locked-only\n    version: next\n",
        )
        .unwrap();
        let mut lock = lock();
        let entry = lock.containers.get_mut("state").unwrap();
        entry.worker = "package://api.workers.iii.dev/locked-only".to_string();
        entry.resolved.name = "locked-only".to_string();
        let artifact = entry.resolved.artifacts.values_mut().next().unwrap();
        artifact.url = format!("{}/artifact", server.uri());
        artifact.sha256 = digest;
        write_atomically(
            &lock_path(&compose_path),
            &serde_yaml::to_string(&lock).unwrap(),
        )
        .unwrap();
        let cache = dir.path().join("cache");
        let mut compose = ComposeFile::load(&compose_path).unwrap();

        let prepared = prepare(&mut compose, &cache, &BTreeSet::new())
            .await
            .unwrap();

        assert_eq!(prepared.resolved_version("state"), Some("0.22.8"));
    }

    #[tokio::test]
    async fn frozen_prepare_downloads_the_locked_artifact_when_the_cache_is_empty() {
        let server = MockServer::start().await;
        let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut archive = tar::Builder::new(encoder);
        let body = b"#!/bin/sh\nexit 0\n";
        let mut header = tar::Header::new_gnu();
        header.set_size(body.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        archive
            .append_data(
                &mut header,
                format!("worker{}", std::env::consts::EXE_SUFFIX),
                &body[..],
            )
            .unwrap();
        let archive = archive.into_inner().unwrap().finish().unwrap();
        let digest = hex::encode(Sha256::digest(&archive));
        Mock::given(matchers::method("GET"))
            .and(matchers::path("/state"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(archive))
            .expect(1)
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let compose_path = dir.path().join("worker-compose.yaml");
        std::fs::write(
            &compose_path,
            "containers:\n  state:\n    worker: package://api.workers.iii.dev/state\n    version: next\n",
        )
        .unwrap();
        let mut lock = lock();
        let artifact = lock
            .containers
            .get_mut("state")
            .unwrap()
            .resolved
            .artifacts
            .values_mut()
            .next()
            .unwrap();
        artifact.url = format!("{}/state", server.uri());
        artifact.sha256 = digest;
        let lock_path = lock_path(&compose_path);
        let before = serde_yaml::to_string(&lock).unwrap();
        write_atomically(&lock_path, &before).unwrap();
        let mut compose = ComposeFile::load(&compose_path).unwrap();

        prepare_frozen(&mut compose, &dir.path().join("cache"))
            .await
            .unwrap();
        assert_eq!(std::fs::read_to_string(lock_path).unwrap(), before);
    }

    #[tokio::test]
    async fn an_unavailable_locked_artifact_preserves_the_previous_lock() {
        let server = MockServer::start().await;
        Mock::given(matchers::method("GET"))
            .and(matchers::path("/state"))
            .respond_with(ResponseTemplate::new(404))
            .expect(1)
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let compose_path = dir.path().join("worker-compose.yaml");
        std::fs::write(
            &compose_path,
            "containers:\n  state:\n    worker: package://api.workers.iii.dev/state\n    version: next\n",
        )
        .unwrap();
        let mut lock = lock();
        lock.containers
            .get_mut("state")
            .unwrap()
            .resolved
            .artifacts
            .values_mut()
            .next()
            .unwrap()
            .url = format!("{}/state", server.uri());
        let lock_path = lock_path(&compose_path);
        let previous = serde_yaml::to_string(&lock).unwrap();
        write_atomically(&lock_path, &previous).unwrap();
        let mut compose = ComposeFile::load(&compose_path).unwrap();

        let error = match prepare(&mut compose, &dir.path().join("cache"), &BTreeSet::new()).await {
            Ok(_) => panic!("the unavailable locked artifact must fail"),
            Err(error) => error,
        };

        assert_eq!(error.code(), "PACKAGE_DOWNLOAD_FAILED");
        assert_eq!(std::fs::read_to_string(lock_path).unwrap(), previous);
    }
}

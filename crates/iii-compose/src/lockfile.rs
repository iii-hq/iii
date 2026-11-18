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
    registry::{Platform, ResolvedPackage},
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
    /// A binary package whose container runs in a VM ([`crate::vm_binary::declares_vm`]): it
    /// installs the Linux build of the host's architecture out of `resolved.artifacts`, which
    /// holds every build the registry published, instead of the host's. The triple is chosen on
    /// each host, as for a host build, so the lock stays the same on every machine. Absent
    /// (`false`) for a host binary, whose entry is as before, and for every bundle, whose one
    /// archive serves both.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    vm: bool,
    resolved: ResolvedPackage,
    /// Present only for an explicitly validated v1 request-to-package binding.
    /// Old same-name locks omit it. New readers retain v1 lock compatibility; old
    /// readers fail closed on this optional field rather than guessing a basename.
    /// It is provenance, not authentication of a registry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    root_binding: Option<crate::registry::ResolveRoot>,
}

impl LockedContainer {
    /// The platform whose build this entry installs.
    fn platform(&self) -> Platform {
        if self.vm {
            Platform::Vm
        } else {
            Platform::Host
        }
    }

    /// Whether this entry still answers a declaration of `worker` at `requested` on `platform`:
    /// a binary package moved between the host and a VM is resolved again.
    fn matches(&self, worker: &str, requested: &str, platform: Platform) -> bool {
        self.worker == worker
            && self.requested == requested
            && self.vm == vm_entry(platform, &self.resolved)
    }
}

/// Whether a package resolved as `resolved` for a container on `platform` installs its VM
/// build: only a binary package has one.
fn vm_entry(platform: Platform, resolved: &ResolvedPackage) -> bool {
    platform == Platform::Vm && resolved.kind == "binary"
}

/// A complete lock candidate with optional artifact acquisition results.
/// Writing stays separate so callers can validate all other file changes first.
pub struct PreparedLock {
    path: PathBuf,
    lock: ComposeLock,
    existed: bool,
    changed: bool,
    package_changes: BTreeSet<String>,
    install_statuses: BTreeMap<String, crate::registry::InstallStatus>,
}

impl PreparedLock {
    /// Writes the lock atomically when its serialized state changed.
    pub fn write_if_changed(&self) -> Result<()> {
        validate(&self.path, &self.lock)?;
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

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn existed(&self) -> bool {
        self.existed
    }

    /// Cache result for each package acquired while preparing this lock.
    pub fn install_statuses(&self) -> &BTreeMap<String, crate::registry::InstallStatus> {
        &self.install_statuses
    }

    /// Registry aliases present in this resolved lock.
    pub(crate) fn aliases(&self) -> impl Iterator<Item = (&str, &str, &str)> {
        self.lock
            .containers
            .iter()
            .filter_map(|(container, entry)| {
                entry.resolved.alias_of.as_deref().map(|canonical| {
                    (
                        container.as_str(),
                        entry
                            .worker
                            .strip_prefix("package://")
                            .unwrap_or(&entry.worker),
                        canonical,
                    )
                })
            })
    }

    /// Attaches this candidate's packages to `compose`, read again after an edit that changed no
    /// package (the explicit VM mode of spec 7.1), as [`attach`] does from the lock on disk.
    pub(crate) fn attach_to(&self, compose: &mut ComposeFile) {
        attach_entries(&self.lock, compose);
    }

    /// The platform whose build container `key` installs: [`Platform::Vm`] for a binary package
    /// that runs in a VM, else [`Platform::Host`].
    pub fn platform(&self, key: &str) -> Platform {
        self.lock
            .containers
            .get(key)
            .map_or(Platform::Host, LockedContainer::platform)
    }

    /// Every locked container with the package it resolved to, in key order.
    pub fn packages(&self) -> impl Iterator<Item = (&str, &ResolvedPackage)> {
        self.lock
            .containers
            .iter()
            .map(|(container, entry)| (container.as_str(), &entry.resolved))
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
    pub(crate) async fn install(&mut self, cache_root: &Path) -> Result<()> {
        validate(&self.path, &self.lock)?;
        let requests = self
            .lock
            .containers
            .iter()
            .map(|(key, entry)| (key.clone(), entry.resolved.clone(), entry.platform()))
            .collect::<Vec<_>>();
        let cache_root = cache_root.to_path_buf();
        let mut installs =
            futures::stream::iter(requests.into_iter().map(|(key, resolved, platform)| {
                let cache_root = cache_root.clone();
                async move {
                    let result = crate::registry::install_resolved_on(
                        &key,
                        &resolved,
                        platform,
                        &cache_root,
                    )
                    .await;
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
    attach_entries(&lock, compose);
    Ok(())
}

/// Attaches to each `package://` container of `compose` the package `lock` resolved for it,
/// when the entry still matches the declared worker and version.
fn attach_entries(lock: &ComposeLock, compose: &mut ComposeFile) {
    for (key, container) in &mut compose.containers {
        let WorkerSource::Package { reference } = &container.worker else {
            continue;
        };
        let requested = container.version.as_deref().unwrap_or("*");
        let worker = format!("package://{reference}");
        let platform = crate::vm_binary::platform(container);
        if let Some(entry) = lock
            .containers
            .get(key)
            .filter(|entry| entry.matches(&worker, requested, platform))
        {
            container.resolved_package = Some(entry.resolved.clone());
        }
    }
}

/// Resolves missing, changed, or explicitly forced declarations, verifies the
/// selected artifacts, and attaches the immutable result to the runtime model.
#[cfg(test)]
pub async fn prepare(
    compose: &mut ComposeFile,
    cache_root: &Path,
    force: &BTreeSet<String>,
) -> Result<PreparedLock> {
    let mut prepared = prepare_metadata(compose, force).await?;
    prepared.install(cache_root).await?;
    Ok(prepared)
}

/// Uses only the exact resolution in an existing lock.
///
/// This is the Compose equivalent of `npm ci`: it never resolves a selector
/// and never changes the lock. A missing cache entry is still downloaded from
/// the immutable URL and digest already recorded in the lock.
pub async fn prepare_frozen(compose: &mut ComposeFile, cache_root: &Path) -> Result<PreparedLock> {
    let lock = frozen_lock(compose)?;
    let path = lock_path(&compose.path);
    let declarations = package_declarations(compose);

    for (key, _, _, _) in &declarations {
        if let (Some(container), Some(entry)) =
            (compose.containers.get_mut(key), lock.containers.get(key))
        {
            container.resolved_package = Some(entry.resolved.clone());
        }
    }
    let mut prepared = PreparedLock {
        path,
        lock,
        existed: true,
        changed: false,
        package_changes: BTreeSet::new(),
        install_statuses: BTreeMap::new(),
    };
    prepared.install(cache_root).await?;
    Ok(prepared)
}

/// Checks that frozen startup can use the lock before any engine is started.
pub(crate) fn preflight_frozen(compose: &ComposeFile) -> Result<()> {
    frozen_lock(compose).map(|_| ())
}

fn frozen_lock(compose: &ComposeFile) -> Result<ComposeLock> {
    let path = lock_path(&compose.path);
    let lock =
        load(&path)?.ok_or_else(|| ComposeError::FrozenLockMissing { path: path.clone() })?;
    let declarations = package_declarations(compose);

    for (key, reference, requested, platform) in &declarations {
        let worker = format!("package://{reference}");
        let Some(entry) = lock.containers.get(key) else {
            return Err(ComposeError::FrozenLockOutOfDate {
                path: path.clone(),
                message: format!("container '{key}' is missing from the lock"),
            });
        };
        if entry.worker != worker || entry.requested != *requested {
            return Err(ComposeError::FrozenLockOutOfDate {
                path: path.clone(),
                message: format!(
                    "container '{key}' changed from {}@{} to package://{reference}@{requested}",
                    entry.worker, entry.requested,
                ),
            });
        }
        if !entry.matches(&worker, requested, *platform) {
            let (now, locked) = match platform {
                Platform::Vm => ("in a VM", "the host's build"),
                Platform::Host => ("on the host", "the Linux build of a VM"),
            };
            return Err(ComposeError::FrozenLockOutOfDate {
                path: path.clone(),
                message: format!(
                    "binary package container '{key}' runs {now} now, and its entry locks {locked}"
                ),
            });
        }
    }
    if let Some(extra) = lock
        .containers
        .keys()
        .find(|key| !declarations.iter().any(|(declared, ..)| declared == *key))
    {
        return Err(ComposeError::FrozenLockOutOfDate {
            path,
            message: format!("container '{extra}' exists only in the lock"),
        });
    }

    Ok(lock)
}

/// Concrete package versions attached to the current, validated lock entries.
/// Entries whose declaration identity does not match are intentionally absent.
pub(crate) fn concrete_versions(compose: &ComposeFile) -> Result<BTreeMap<String, String>> {
    let Some(lock) = load(&lock_path(&compose.path))? else {
        return Ok(BTreeMap::new());
    };
    Ok(compose
        .containers
        .iter()
        .filter_map(|(key, container)| {
            let WorkerSource::Package { reference } = &container.worker else {
                return None;
            };
            let requested = container.version.as_deref().unwrap_or("*");
            let worker = format!("package://{reference}");
            let platform = crate::vm_binary::platform(container);
            lock.containers
                .get(key)
                .filter(|entry| entry.matches(&worker, requested, platform))
                .map(|entry| (key.clone(), entry.resolved.version.clone()))
        })
        .collect())
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
pub(crate) async fn prepare_metadata_with_versions(
    compose: &mut ComposeFile,
    force: &BTreeSet<String>,
    selected_versions: &BTreeMap<String, String>,
) -> Result<PreparedLock> {
    prepare_metadata_with_selections(
        compose,
        force,
        selected_versions,
        |key, reference, range, platform| async move {
            crate::registry::resolve_selection_on(&key, &reference, &range, platform).await
        },
    )
    .await
}

/// [`prepare_metadata_with_versions`] again after an edit, with `earlier` the
/// candidate prepared before it: a declaration whose exact version `earlier`
/// already resolved takes that result instead of a second registry round trip.
/// Without it, adding the sandbox adapter to a project that has no lock yet
/// resolved every container twice. Selected versions take precedence over the lock on disk,
/// including selections outside `force` that an earlier phase of this operation made.
pub(crate) async fn prepare_metadata_reusing(
    compose: &mut ComposeFile,
    force: &BTreeSet<String>,
    selected_versions: &BTreeMap<String, String>,
    earlier: &PreparedLock,
) -> Result<PreparedLock> {
    // Selected versions belong to the in-flight operation, not the older lock on disk.
    let mut force = force.clone();
    force.extend(selected_versions.keys().cloned());
    prepare_metadata_with_selections(
        compose,
        &force,
        selected_versions,
        |key, reference, range, platform| {
            let reused = earlier
                .lock
                .containers
                .get(&key)
                .filter(|entry| {
                    entry.worker == format!("package://{reference}")
                        && entry.resolved.version == range
                        && entry.vm == vm_entry(platform, &entry.resolved)
                })
                .map(|entry| crate::registry::Selection {
                    package: entry.resolved.clone(),
                    root_binding: entry.root_binding.clone(),
                });
            async move {
                match reused {
                    Some(resolved) => Ok(resolved),
                    None => {
                        crate::registry::resolve_selection_on(&key, &reference, &range, platform)
                            .await
                    }
                }
            }
        },
    )
    .await
}

/// Consume the exact graph metadata selected by the dependency planner. An existing
/// opaque dependency may need an exact lookup only to obtain its missing provenance;
/// its identity/content must match, and its planned package/graph are never replaced.
pub(crate) async fn prepare_metadata_with_plan(
    compose: &mut ComposeFile,
    force: &BTreeSet<String>,
    versions: &BTreeMap<String, String>,
    selections: &BTreeMap<String, crate::registry::Selection>,
) -> Result<PreparedLock> {
    let mut force = force.clone();
    force.extend(versions.keys().cloned());
    prepare_metadata_with_selections(
        compose,
        &force,
        versions,
        |key, reference, range, platform| {
            let selected = selections.get(&key).cloned();
            async move {
                match selected {
                    Some(mut selection) if selection.package.version == range => {
                        let (registry, name) = crate::registry::split_reference(&reference);
                        if selection.root_binding.is_none() && name != selection.package.name {
                            // An existing opaque dependency keeps its declared reference,
                            // but its node in another root's graph has no request echo.
                            // Obtain that proof without replacing the planned package/graph.
                            if selection.package.registry != registry {
                                return Err(ComposeError::PackageNotResolved {
                                    container: key, name: reference, range,
                                    message: "the planned package authority differs from the preserved reference".into(),
                                });
                            }
                            let proven = crate::registry::resolve_selection_on(
                                &key, &reference, &range, platform,
                            ).await?;
                            if !selection.same_package(&proven)
                                || !proven.root_binding.as_ref().is_some_and(|binding| {
                                    binding.validates_lock(&name, &range, &proven.package)
                                })
                            {
                                return Err(ComposeError::PackageNotResolved {
                                    container: key, name: reference, range,
                                    message: "the preserved reference resolves to different package metadata or lacks a valid root binding".into(),
                                });
                            }
                            selection.root_binding = proven.root_binding;
                        }
                        Ok(selection)
                    }
                    Some(_) => Err(ComposeError::PackageNotResolved {
                        container: key,
                        name: reference,
                        range,
                        message: "the planned package version differs from the lock selection"
                            .into(),
                    }),
                    None => {
                        crate::registry::resolve_selection_on(&key, &reference, &range, platform)
                            .await
                    }
                }
            }
        },
    )
    .await
}

#[cfg(test)]
async fn prepare_metadata_with_resolver<F, Fut>(
    compose: &mut ComposeFile,
    force: &BTreeSet<String>,
    versions: &BTreeMap<String, String>,
    resolve: F,
) -> Result<PreparedLock>
where
    F: Fn(String, String, String, Platform) -> Fut,
    Fut: std::future::Future<Output = Result<ResolvedPackage>>,
{
    prepare_metadata_with_selections(
        compose,
        force,
        versions,
        |key, reference, range, platform| {
            let future = resolve(key, reference, range, platform);
            async move { future.await.map(Into::into) }
        },
    )
    .await
}

async fn prepare_metadata_with_selections<F, Fut>(
    compose: &mut ComposeFile,
    force: &BTreeSet<String>,
    selected_versions: &BTreeMap<String, String>,
    resolve: F,
) -> Result<PreparedLock>
where
    F: Fn(String, String, String, Platform) -> Fut,
    Fut: std::future::Future<Output = Result<crate::registry::Selection>>,
{
    let path = lock_path(&compose.path);
    let previous = load(&path)?;
    let mut containers = BTreeMap::new();
    let mut package_changes = BTreeSet::new();

    let declarations = package_declarations(compose);

    let mut resolutions = futures::stream::iter(declarations.into_iter().map(
        |(key, reference, requested, platform)| {
            let previous = &previous;
            let resolve = &resolve;
            async move {
                let worker = format!("package://{reference}");
                let reusable = previous
                    .as_ref()
                    .and_then(|lock| lock.containers.get(&key))
                    .filter(|entry| {
                        !force.contains(&key) && entry.matches(&worker, &requested, platform)
                    });
                let entry = match reusable {
                    Some(entry) => entry.clone(),
                    None => {
                        let range = selected_versions.get(&key).unwrap_or(&requested).clone();
                        let selection = resolve(key.clone(), reference, range, platform).await?;
                        let resolved = selection.package;
                        LockedContainer {
                            root_binding: selection.root_binding,
                            worker,
                            requested,
                            vm: vm_entry(platform, &resolved),
                            resolved,
                        }
                    }
                };
                Ok::<_, ComposeError>((key, entry))
            }
        },
    ))
    // Overlap registry requests without changing which declared failure is reported first.
    .buffered(crate::parallelism::max_parallel_resolves());

    while let Some(result) = resolutions.next().await {
        let (key, entry) = result?;
        if previous
            .as_ref()
            .and_then(|lock| lock.containers.get(&key))
            .is_none_or(|old| runtime_entry_changed(old, &entry))
        {
            package_changes.insert(key.clone());
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
    validate(&path, &lock)?;
    attach_entries(&lock, compose);
    let changed = previous.as_ref() != Some(&lock);
    Ok(PreparedLock {
        path,
        lock,
        existed: previous.is_some(),
        changed,
        package_changes,
        install_statuses: BTreeMap::new(),
    })
}

/// Every `package://` container: its key, reference, selector, and the platform it runs on.
fn package_declarations(compose: &ComposeFile) -> Vec<(String, String, String, Platform)> {
    compose
        .containers
        .iter()
        .filter_map(|(key, container)| match &container.worker {
            WorkerSource::Package { reference } => Some((
                key.clone(),
                reference.clone(),
                container.version.clone().unwrap_or_else(|| "*".to_string()),
                crate::vm_binary::platform(container),
            )),
            WorkerSource::Path { .. } => None,
        })
        .collect()
}

/// Whether the worker of `next` runs different content than that of `previous`: another
/// platform, or another package or build on the same one.
fn runtime_entry_changed(previous: &LockedContainer, next: &LockedContainer) -> bool {
    previous.vm != next.vm
        || runtime_package_changed_for(
            next.platform().targets(),
            &previous.resolved,
            &next.resolved,
        )
}

/// Compares the package fields that can change worker runtime behavior.
#[cfg(test)]
fn runtime_package_changed(previous: &ResolvedPackage, next: &ResolvedPackage) -> bool {
    runtime_package_changed_for(crate::registry::host_targets(), previous, next)
}

/// Each side's build is selected on its own: a lock that drops the musl
/// build a musl host was running switches the worker to the glibc one, even
/// when that glibc build did not change.
fn runtime_package_changed_for(
    candidates: &[&str],
    previous: &ResolvedPackage,
    next: &ResolvedPackage,
) -> bool {
    use crate::registry::selected_artifact;
    previous.kind != next.kind
        || previous.default_config != next.default_config
        || selected_artifact(candidates, &previous.artifacts)
            != selected_artifact(candidates, &next.artifacts)
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
        let (expected_registry, expected_name) = crate::registry::split_reference(reference);
        if entry.requested.trim().is_empty() {
            return Err(invalid(format!(
                "container '{container}' has an empty requested version"
            )));
        }
        if let Some(binding) = &entry.root_binding {
            if !binding.validates_lock(&expected_name, &entry.requested, &entry.resolved) {
                return Err(invalid(format!(
                    "container '{container}' has an invalid requested-root binding"
                )));
            }
        } else if entry.resolved.name != expected_name {
            return Err(invalid(format!(
                "container '{container}' resolved '{}', expected '{expected_name}'",
                entry.resolved.name
            )));
        }
        if entry.resolved.registry != expected_registry {
            return Err(invalid(format!(
                "container '{container}' resolved from '{}', expected '{expected_registry}'",
                entry.resolved.registry
            )));
        }
        if !crate::registry::is_path_safe(&entry.resolved.name) {
            return Err(invalid(format!(
                "container '{container}' has an unsafe resolved package name"
            )));
        }
        if entry
            .resolved
            .alias_of
            .as_deref()
            .is_some_and(|alias| !crate::registry::is_path_safe(alias))
        {
            return Err(invalid(format!(
                "container '{container}' has an unsafe resolved canonical package name"
            )));
        }
        if !crate::registry::is_path_safe(&entry.resolved.version) {
            return Err(invalid(format!(
                "container '{container}' has an unsafe resolved version"
            )));
        }
        if entry.vm && entry.resolved.kind != "binary" {
            return Err(invalid(format!(
                "container '{container}' locks a VM build of a {} package; only a binary \
                 package has one",
                entry.resolved.kind
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
                    root_binding: None,
                    worker: "package://api.workers.iii.dev/state".to_string(),
                    requested: "next".to_string(),
                    vm: false,
                    resolved: ResolvedPackage {
                        name: "state".to_string(),
                        alias_of: None,
                        registry: crate::registry::DEFAULT_REGISTRY.to_string(),
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

    fn bound_lock() -> ComposeLock {
        let mut lock = lock();
        let entry = lock.containers.get_mut("state").unwrap();
        entry.worker = "package://registry.example.test/team/tool".into();
        entry.requested = "next".into();
        entry.resolved.registry = "https://registry.example.test".into();
        entry.resolved.name = "tool-linux".into();
        entry.resolved.version = "1.2.3".into();
        entry.root_binding = Some(
            serde_json::from_value(serde_json::json!({
                "schema":1, "requested":{"worker":"team/tool","version":"next"},
                "name":"tool-linux", "version":"1.2.3"
            }))
            .unwrap(),
        );
        lock
    }

    #[test]
    fn bound_root_lock_roundtrips_and_legacy_same_name_locks_stay_readable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("worker-compose.lock");
        for lock in [lock(), bound_lock()] {
            validate(&path, &lock).unwrap();
            write_atomically(&path, &serde_yaml::to_string(&lock).unwrap()).unwrap();
            assert_eq!(load(&path).unwrap(), Some(lock));
        }
    }

    #[test]
    fn forged_or_inconsistent_root_bindings_are_refused_on_reload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("worker-compose.lock");
        let original = serde_json::to_value(bound_lock()).unwrap();
        for (pointer, value) in [
            (
                "/containers/state/root_binding/schema",
                serde_json::json!(2),
            ),
            (
                "/containers/state/root_binding/requested/worker",
                serde_json::json!("tool"),
            ),
            (
                "/containers/state/root_binding/requested/version",
                serde_json::json!("other-tag"),
            ),
            (
                "/containers/state/root_binding/name",
                serde_json::json!("tool"),
            ),
            (
                "/containers/state/root_binding/version",
                serde_json::json!("1.2.4"),
            ),
            (
                "/containers/state/resolved/registry",
                serde_json::json!("https://other.example"),
            ),
            (
                "/containers/state/resolved/alias_of",
                serde_json::json!("../escape"),
            ),
            ("/containers/state/requested", serde_json::json!("1.2.4")),
            (
                "/containers/state/root_binding",
                serde_json::json!({"name":"tool-linux","version":"1.2.3"}),
            ),
        ] {
            let mut body = original.clone();
            if pointer.ends_with("alias_of") {
                body["containers"]["state"]["resolved"]["alias_of"] = value;
            } else {
                *body.pointer_mut(pointer).unwrap() = value;
            }
            std::fs::write(&path, serde_yaml::to_string(&body).unwrap()).unwrap();
            let error = load(&path).unwrap_err();
            assert_eq!(error.code(), "INVALID_COMPOSE_LOCK", "{pointer}: {error}");
        }
        let mut no_proof = bound_lock();
        let entry = no_proof.containers.get_mut("state").unwrap();
        entry.root_binding = None;
        entry.resolved.alias_of = Some("tool".into());
        assert!(
            validate(&path, &no_proof).is_err(),
            "alias cannot replace request provenance"
        );
    }

    #[tokio::test]
    async fn invalid_candidate_is_refused_before_attach_write_or_install() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("worker-compose.yaml");
        let mut compose = ComposeFile::parse(
            "containers:\n  state: {worker: package://state, version: next}\n",
            &path,
        )
        .unwrap();
        let mut package = lock().containers.remove("state").unwrap().resolved;
        package.name = "unrequested".into();
        let error = prepare_metadata_with_resolver(
            &mut compose,
            &BTreeSet::new(),
            &BTreeMap::new(),
            |_, _, _, _| std::future::ready(Ok(package.clone())),
        )
        .await
        .err()
        .unwrap();
        assert_eq!(error.code(), "INVALID_COMPOSE_LOCK");
        assert!(compose.containers["state"].resolved_package.is_none());
        assert!(!lock_path(&path).exists());
        let invalid = PreparedLock {
            path: lock_path(&path),
            lock: bound_lock(),
            existed: false,
            changed: true,
            package_changes: BTreeSet::new(),
            install_statuses: BTreeMap::new(),
        };
        let mut invalid = invalid;
        invalid
            .lock
            .containers
            .get_mut("state")
            .unwrap()
            .root_binding = None;
        assert_eq!(
            invalid.write_if_changed().unwrap_err().code(),
            "INVALID_COMPOSE_LOCK"
        );
        let cache = dir.path().join("must-not-be-created");
        assert_eq!(
            invalid.install(&cache).await.unwrap_err().code(),
            "INVALID_COMPOSE_LOCK"
        );
        assert!(!cache.exists());
        assert!(!lock_path(&path).exists());
    }

    #[tokio::test]
    async fn plan_handoff_rejects_a_changed_version_before_attach_or_write() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("worker-compose.yaml");
        let mut compose = ComposeFile::parse(
            "containers:\n  state: {worker: package://registry.example.test/team/tool, version: next}\n", &path).unwrap();
        let entry = bound_lock().containers.remove("state").unwrap();
        let selections = BTreeMap::from([(
            "state".into(),
            crate::registry::Selection {
                package: entry.resolved,
                root_binding: entry.root_binding,
            },
        )]);
        let versions = BTreeMap::from([("state".into(), "1.2.4".into())]);
        let error =
            prepare_metadata_with_plan(&mut compose, &BTreeSet::new(), &versions, &selections)
                .await
                .err()
                .unwrap();
        let ComposeError::PackageNotResolved { message, .. } = error else {
            panic!("{error}")
        };
        assert_eq!(
            message,
            "the planned package version differs from the lock selection"
        );
        assert!(compose.containers["state"].resolved_package.is_none());
        assert!(!lock_path(&path).exists());
    }

    #[test]
    fn lock_path_replaces_the_compose_extension() {
        assert_eq!(
            lock_path(Path::new("config/worker-compose.yaml")),
            PathBuf::from("config/worker-compose.lock")
        );
    }

    #[test]
    fn concrete_versions_require_matching_identity_and_never_use_selector() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("worker-compose.yaml");
        let compose = ComposeFile::parse("containers:\n  state:\n    worker: package://api.workers.iii.dev/state\n    version: next\n", &path).unwrap();
        assert!(concrete_versions(&compose).unwrap().is_empty());
        let mut stored = lock();
        std::fs::write(lock_path(&path), serde_yaml::to_string(&stored).unwrap()).unwrap();
        assert_eq!(concrete_versions(&compose).unwrap()["state"], "0.22.8");
        stored.containers.get_mut("state").unwrap().requested = "*".into();
        std::fs::write(lock_path(&path), serde_yaml::to_string(&stored).unwrap()).unwrap();
        assert!(concrete_versions(&compose).unwrap().is_empty());
        std::fs::write(lock_path(&path), "invalid: lock").unwrap();
        assert!(concrete_versions(&compose).is_err());
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

    #[test]
    fn a_switch_between_host_builds_changes_runtime_content() {
        const MUSL: &str = "x86_64-unknown-linux-musl";
        const GNU: &str = "x86_64-unknown-linux-gnu";
        let package = |builds: &[(&str, char)]| ResolvedPackage {
            artifacts: builds
                .iter()
                .map(|(target, digest)| {
                    (
                        target.to_string(),
                        RegistryArtifact {
                            url: "https://example.com/state.tar.gz".to_string(),
                            sha256: digest.to_string().repeat(64),
                        },
                    )
                })
                .collect(),
            ..lock().containers.remove("state").unwrap().resolved
        };
        let both = package(&[(MUSL, 'a'), (GNU, 'b')]);
        let glibc_only = package(&[(GNU, 'b')]);
        let musl_on_glibc = [MUSL, GNU];

        // A musl host ran musl A; the next lock has only the unchanged glibc
        // build B, so the worker moves from A to B, and back again.
        assert!(runtime_package_changed_for(
            &musl_on_glibc,
            &both,
            &glibc_only
        ));
        assert!(runtime_package_changed_for(
            &musl_on_glibc,
            &glibc_only,
            &both
        ));
        // A new build this host does not run changes nothing it runs.
        let new_glibc = package(&[(MUSL, 'a'), (GNU, 'c')]);
        assert!(!runtime_package_changed_for(
            &musl_on_glibc,
            &both,
            &new_glibc
        ));
        // A glibc host ran B all along.
        assert!(!runtime_package_changed_for(&[GNU], &both, &glibc_only));
    }

    /// A compose file with a host binary, a binary in a VM and a bundle in a VM.
    const PLACED: &str = "containers:\n  host:\n    worker: package://tool\n    version: 1.0.0\n  \
                          vm:\n    worker: package://tool\n    version: 1.0.0\n    runtime:\n      \
                          base_image: docker.io/library/alpine:3.20\n    scripts:\n      build: iii \
                          sandbox build\n      run: iii sandbox run\n  bundle:\n    worker: \
                          package://kit\n    version: 1.0.0\n    scripts:\n      build: iii sandbox \
                          build\n      run: iii sandbox run\n";

    /// What the registry resolves `key` to: a bundle for `bundle`, else a binary with a host
    /// build and the two Linux builds of the host's architecture.
    fn placed_package(key: &str) -> ResolvedPackage {
        let mut package = lock().containers.remove("state").unwrap().resolved;
        package.name = if key == "bundle" { "kit" } else { "tool" }.to_string();
        package.registry = crate::registry::DEFAULT_REGISTRY.to_string();
        if key == "bundle" {
            package.kind = "bundle".to_string();
            return package;
        }
        for (target, digest) in [
            (crate::registry::vm_targets()[0], 'b'),
            (*crate::registry::vm_targets().last().unwrap(), 'c'),
        ] {
            package
                .artifacts
                .entry(target.to_string())
                .or_insert(RegistryArtifact {
                    url: format!("https://example.com/{target}"),
                    sha256: digest.to_string().repeat(64),
                });
        }
        package
    }

    /// The lock records the VM build of a binary in a VM and nothing new for the rest: a host
    /// binary and a bundle keep the entry they had.
    #[tokio::test]
    async fn a_binary_in_a_vm_locks_its_vm_build_and_the_rest_keep_their_entry() {
        let dir = tempfile::tempdir().unwrap();
        let mut compose =
            ComposeFile::parse(PLACED, dir.path().join("worker-compose.yaml")).unwrap();
        let calls = std::cell::RefCell::new(BTreeMap::new());
        let prepared = prepare_metadata_with_resolver(
            &mut compose,
            &BTreeSet::new(),
            &BTreeMap::new(),
            |key, _, _, platform| {
                calls.borrow_mut().insert(key.clone(), platform);
                std::future::ready(Ok(placed_package(&key)))
            },
        )
        .await
        .unwrap();

        assert_eq!(
            calls.into_inner(),
            BTreeMap::from([
                ("bundle".to_string(), Platform::Vm),
                ("host".to_string(), Platform::Host),
                ("vm".to_string(), Platform::Vm),
            ])
        );
        assert_eq!(prepared.platform("vm"), Platform::Vm);
        assert_eq!(prepared.platform("host"), Platform::Host);
        assert_eq!(prepared.platform("bundle"), Platform::Host);
        let written = serde_yaml::to_value(&prepared.lock).unwrap();
        let fields = |key: &str| {
            written["containers"][key]
                .as_mapping()
                .unwrap()
                .keys()
                .map(|field| field.as_str().unwrap().to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(fields("host"), ["worker", "requested", "resolved"]);
        assert_eq!(fields("bundle"), ["worker", "requested", "resolved"]);
        assert_eq!(fields("vm"), ["worker", "requested", "vm", "resolved"]);
        assert_eq!(
            written["containers"]["vm"]["vm"],
            serde_yaml::Value::Bool(true)
        );

        prepared.write_if_changed().unwrap();
        assert_eq!(load(prepared.path()).unwrap(), Some(prepared.lock.clone()));
        // Read again, nothing is resolved anew and the lock stays as it is.
        let resolved_again = std::cell::RefCell::new(Vec::new());
        let again = prepare_metadata_with_resolver(
            &mut compose,
            &BTreeSet::new(),
            &BTreeMap::new(),
            |key, _, _, _| {
                resolved_again.borrow_mut().push(key.clone());
                std::future::ready(Ok(placed_package(&key)))
            },
        )
        .await
        .unwrap();
        assert!(resolved_again.into_inner().is_empty());
        assert!(!again.changed());
        assert!(again.package_changes.is_empty());
    }

    /// A binary moved between the host and a VM is resolved again for its new platform; the old
    /// entry is not attached meanwhile, and `--frozen` refuses it.
    #[tokio::test]
    async fn a_binary_moved_into_a_vm_is_resolved_again() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("worker-compose.yaml");
        let host_file = PLACED.replace(
            "    runtime:\n      base_image: docker.io/library/alpine:3.20\n    scripts:\n      \
             build: iii sandbox build\n      run: iii sandbox run\n  bundle:",
            "  bundle:",
        );
        assert_ne!(host_file, PLACED);
        let mut on_host = ComposeFile::parse(&host_file, &path).unwrap();
        prepare_metadata_with_resolver(
            &mut on_host,
            &BTreeSet::new(),
            &BTreeMap::new(),
            |key, _, _, _| std::future::ready(Ok(placed_package(&key))),
        )
        .await
        .unwrap()
        .write_if_changed()
        .unwrap();
        assert!(!load(&lock_path(&path)).unwrap().unwrap().containers["vm"].vm);

        let mut in_vm = ComposeFile::parse(PLACED, &path).unwrap();
        attach(&mut in_vm).unwrap();
        assert!(in_vm.containers["vm"].resolved_package.is_none());
        assert!(in_vm.containers["host"].resolved_package.is_some());
        let error = preflight_frozen(&in_vm).unwrap_err();
        assert_eq!(error.code(), "COMPOSE_LOCK_OUT_OF_DATE");
        assert!(
            error.to_string().contains("'vm' runs in a VM now"),
            "{error}"
        );

        let calls = std::cell::RefCell::new(Vec::new());
        let prepared = prepare_metadata_with_resolver(
            &mut in_vm,
            &BTreeSet::new(),
            &BTreeMap::new(),
            |key, _, _, platform| {
                calls.borrow_mut().push((key.clone(), platform));
                std::future::ready(Ok(placed_package(&key)))
            },
        )
        .await
        .unwrap();
        assert_eq!(calls.into_inner(), [("vm".to_string(), Platform::Vm)]);
        assert!(prepared.lock.containers["vm"].vm);
        assert_eq!(prepared.package_changes, BTreeSet::from(["vm".to_string()]));
    }

    #[test]
    fn only_a_binary_package_locks_a_vm_build() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("worker-compose.lock");
        let mut bundle = lock();
        let entry = bundle.containers.get_mut("state").unwrap();
        entry.resolved.kind = "bundle".to_string();
        entry.vm = true;
        std::fs::write(&path, serde_yaml::to_string(&bundle).unwrap()).unwrap();
        let error = load(&path).unwrap_err();
        assert_eq!(error.code(), "INVALID_COMPOSE_LOCK");
        assert!(
            error.to_string().contains("only a binary package"),
            "{error}"
        );
    }

    #[test]
    fn a_move_between_host_and_vm_changes_runtime_content() {
        let host = lock().containers.remove("state").unwrap();
        let vm = LockedContainer {
            vm: true,
            ..host.clone()
        };
        assert!(runtime_entry_changed(&host, &vm));
        assert!(runtime_entry_changed(&vm, &host));
        assert!(!runtime_entry_changed(&host, &host));
        assert!(!runtime_entry_changed(&vm, &vm));
    }

    #[tokio::test]
    async fn metadata_resolution_overlaps_requests_with_the_resolve_limit() {
        use std::cell::Cell;

        let dir = tempfile::tempdir().unwrap();
        let limit = crate::parallelism::max_parallel_resolves();
        let count = limit + 1;
        let mut yaml = String::from("containers:\n");
        for index in 0..count {
            yaml.push_str(&format!(
                "  worker-{index}:\n    worker: package://worker-{index}\n    version: latest\n"
            ));
        }
        let mut compose =
            ComposeFile::parse(&yaml, dir.path().join("worker-compose.yaml")).unwrap();
        let started = Cell::new(0);
        let permits = tokio::sync::Semaphore::new(0);
        let force = BTreeSet::new();
        let versions = BTreeMap::new();
        let package = lock().containers.remove("state").unwrap().resolved;
        let prepare =
            prepare_metadata_with_resolver(&mut compose, &force, &versions, |key, _, _, _| {
                let started = &started;
                let permits = &permits;
                let mut package = package.clone();
                async move {
                    started.set(started.get() + 1);
                    permits.acquire().await.unwrap().forget();
                    package.name = key;
                    Ok(package)
                }
            });
        tokio::pin!(prepare);

        // No clock-based assertion: all slots must start before any can finish.
        assert!(futures::poll!(prepare.as_mut()).is_pending());
        assert_eq!(started.get(), limit);
        permits.add_permits(1);
        assert!(futures::poll!(prepare.as_mut()).is_pending());
        assert_eq!(started.get(), count);
        permits.add_permits(limit);
        let prepared = prepare.await.unwrap();

        assert_eq!(prepared.lock.containers.len(), count);
        for (key, entry) in &prepared.lock.containers {
            assert_eq!(&entry.resolved.name, key);
            assert_eq!(entry.requested, "latest");
        }
        prepared.write_if_changed().unwrap();
        assert_eq!(load(prepared.path()).unwrap(), Some(prepared.lock));
    }

    #[tokio::test]
    async fn metadata_resolution_reuses_locks_and_honors_forced_selected_versions() {
        let dir = tempfile::tempdir().unwrap();
        let compose_path = dir.path().join("worker-compose.yaml");
        let mut previous = lock();
        let mut queue = previous.containers["state"].clone();
        queue.worker = "package://queue".to_string();
        queue.resolved.name = "queue".to_string();
        previous.containers.insert("queue".to_string(), queue);
        write_atomically(
            &lock_path(&compose_path),
            &serde_yaml::to_string(&previous).unwrap(),
        )
        .unwrap();
        let mut compose = ComposeFile::parse(
            "containers:\n  state:\n    worker: package://api.workers.iii.dev/state\n    version: next\n  queue:\n    worker: package://queue\n    version: next\n  local:\n    worker: path://./local\n    scripts: { run: ./start }\n",
            &compose_path,
        ).unwrap();
        let calls = std::cell::RefCell::new(Vec::new());
        let mut updated = previous.containers["queue"].resolved.clone();
        updated.version = "0.23.0".to_string();
        updated.artifacts.values_mut().next().unwrap().sha256 = "b".repeat(64);
        let force = BTreeSet::from(["queue".to_string()]);
        let versions = BTreeMap::from([("queue".to_string(), "0.23.0".to_string())]);

        let prepared = prepare_metadata_with_resolver(
            &mut compose,
            &force,
            &versions,
            |key, reference, range, _| {
                calls.borrow_mut().push((key, reference, range));
                std::future::ready(Ok(updated.clone()))
            },
        )
        .await
        .unwrap();

        assert_eq!(
            calls.into_inner(),
            vec![(
                "queue".to_string(),
                "queue".to_string(),
                "0.23.0".to_string()
            )]
        );
        assert_eq!(
            prepared.lock.containers["state"],
            previous.containers["state"]
        );
        assert_eq!(prepared.lock.containers["queue"].requested, "next");
        assert_eq!(
            prepared.package_changes,
            BTreeSet::from(["queue".to_string()])
        );
        assert_eq!(
            compose.containers["queue"].resolved_package.as_ref(),
            Some(&updated)
        );
        assert!(!prepared.lock.containers.contains_key("local"));
    }

    #[tokio::test]
    async fn reusing_metadata_keeps_selected_candidate_instead_of_disk_lock() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("worker-compose.yaml");
        let text = "containers:\n  state:\n    worker: package://api.workers.iii.dev/state\n    version: next\n";
        let previous = lock();
        let before = serde_yaml::to_string(&previous).unwrap();
        std::fs::write(lock_path(&path), &before).unwrap();
        let mut compose = ComposeFile::parse(text, &path).unwrap();
        for version in ["0.23.0", "0.22.8"] {
            let mut updated = previous.containers["state"].resolved.clone();
            updated.version = version.to_string();
            updated.default_config =
                Some(serde_yaml::from_str("capabilities: [worker.build]\n").unwrap());
            updated.artifacts.values_mut().next().unwrap().sha256 = "b".repeat(64);
            let force = BTreeSet::from(["state".to_string()]);
            let versions = BTreeMap::from([("state".to_string(), version.to_string())]);
            let earlier =
                prepare_metadata_with_resolver(&mut compose, &force, &versions, |_, _, _, _| {
                    std::future::ready(Ok(updated.clone()))
                })
                .await
                .unwrap();
            let reused =
                prepare_metadata_reusing(&mut compose, &BTreeSet::new(), &versions, &earlier)
                    .await
                    .unwrap();
            assert_eq!(reused.lock, earlier.lock);
            assert_eq!(reused.package_changes, earlier.package_changes);
            assert!(reused.changed());
            assert_eq!(compose.containers["state"].version.as_deref(), Some("next"));
            assert_eq!(
                compose.containers["state"].resolved_package.as_ref(),
                Some(&updated)
            );
            assert_eq!(std::fs::read_to_string(lock_path(&path)).unwrap(), before);
        }
    }

    #[tokio::test]
    async fn metadata_resolution_reports_first_declared_failure_and_preserves_lock() {
        let dir = tempfile::tempdir().unwrap();
        let compose_path = dir.path().join("worker-compose.yaml");
        let before = serde_yaml::to_string(&lock()).unwrap();
        write_atomically(&lock_path(&compose_path), &before).unwrap();
        let mut compose = ComposeFile::parse(
            "containers:\n  first:\n    worker: package://first\n    version: latest\n  second:\n    worker: package://second\n    version: latest\n",
            &compose_path,
        ).unwrap();
        let error = match prepare_metadata_with_resolver(
            &mut compose,
            &BTreeSet::new(),
            &BTreeMap::new(),
            |key, _, range, _| async move {
                if key == "first" {
                    tokio::task::yield_now().await;
                }
                Err(ComposeError::PackageNotResolved {
                    container: key.clone(),
                    name: key,
                    range,
                    message: "resolution failed".to_string(),
                })
            },
        )
        .await
        {
            Ok(_) => panic!("failed resolutions must not produce a lock"),
            Err(error) => error,
        };

        assert!(
            matches!(error, ComposeError::PackageNotResolved { container, .. } if container == "first")
        );
        assert_eq!(
            std::fs::read_to_string(lock_path(&compose_path)).unwrap(),
            before
        );
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

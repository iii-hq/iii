// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Turning `package://host/name` into a binary on disk.
//!
//! Three steps, and the middle one is the reason the other two exist:
//!
//! 1. **Resolve.** The registry is asked which exact version satisfies the
//!    declared range for this host's target, and answers with a URL and a
//!    digest.
//! 2. **Verify.** The archive is hashed before anything is written where a
//!    process will run it. A download that does not match its digest is not a
//!    slow download or a corrupt one — it is a different artefact than the
//!    registry promised, and it never reaches disk.
//! 3. **Cache.** Installs are keyed by package metadata, target, and SHA-256,
//!    so only the exact verified artefact is reused across registries and
//!    projects.

use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    error::{ComposeError, Result},
    lockfile::PreparedLock,
};

/// Registry used when a `package://` reference names no host.
pub const DEFAULT_REGISTRY: &str = "https://api.workers.iii.dev";

/// A bundle archive carries its manifest at the root; the start command lives
/// there rather than in the compose file.
pub const BUNDLE_MANIFEST: &str = "iii.worker.yaml";

/// Registry resolution is small and idempotent. A short transport outage or
/// overloaded server should not roll back an otherwise healthy project.
const RESOLVE_ATTEMPTS: usize = 3;
/// Three attempts keep the whole operation near the original one-minute
/// budget, instead of multiplying that budget for every retry.
const RESOLVE_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(20);
const RESOLVE_RETRY_DELAY: Duration = Duration::from_millis(250);

/// Downloads get their own budget: an artefact is megabytes over a link we do
/// not control.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(300);
const INTEGRITY_FILE: &str = ".iii-compose-integrity.json";

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CacheIntegrity {
    archive_sha256: String,
    tree_sha256: String,
}

/// What the registry answers to `POST /resolve`.
#[derive(Debug, Deserialize)]
struct ResolveResponse {
    graph: Vec<ResolvedWorker>,
    /// Who calls whom. Two workers may need the same one, so this is a graph
    /// and not a tree: the shared worker is declared once and depended on
    /// twice.
    #[serde(default)]
    edges: Vec<ResolvedEdge>,
}

#[derive(Debug, Deserialize)]
struct ResolvedEdge {
    from: String,
    to: String,
}

#[derive(Debug, Deserialize)]
struct ResolvedWorker {
    name: String,
    #[serde(default)]
    alias_of: Option<String>,
    version: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    binaries: std::collections::BTreeMap<String, RegistryArtifact>,
    /// A bundle ships one archive for every platform, named here rather than in
    /// `binaries`. The registry sends both fields or neither.
    #[serde(default)]
    archive_url: Option<String>,
    #[serde(default)]
    sha256: Option<String>,
    /// The worker's own default configuration, if it ships one.
    #[serde(default)]
    config: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistryArtifact {
    pub sha256: String,
    pub url: String,
}

/// Immutable registry result stored in `worker-compose.lock`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedPackage {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias_of: Option<String>,
    #[serde(
        default = "default_registry",
        skip_serializing_if = "is_default_registry"
    )]
    pub registry: String,
    pub version: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub artifacts: std::collections::BTreeMap<String, RegistryArtifact>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_config: Option<serde_yaml::Value>,
}

fn default_registry() -> String {
    DEFAULT_REGISTRY.to_string()
}

fn is_default_registry(registry: &str) -> bool {
    registry == DEFAULT_REGISTRY
}

/// What was installed, and therefore how it is started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Payload {
    /// A native executable, run as a child process.
    Binary(PathBuf),
    /// An extracted bundle directory holding `iii.worker.yaml`. The command it
    /// declares is publisher-controlled, so it runs in a VM rather than on the
    /// host — see `lifecycle::start_one`.
    Bundle(PathBuf),
}

/// Whether this invocation downloaded an artefact or reused one already on
/// disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallStatus {
    Downloaded,
    Cached,
}

/// A package resolved and installed locally.
#[derive(Debug, Clone)]
pub struct InstalledPackage {
    pub name: String,
    pub alias_of: Option<String>,
    pub version: String,
    pub payload: Payload,
    /// Configuration the worker ships with, to be merged under anything the
    /// compose file overrides.
    pub default_config: Option<serde_yaml::Value>,
    pub status: InstallStatus,
}

/// Alias warnings belong to an operation, not the daemon's lifetime: add may
/// acquire a package and then install it again during reconciliation.
pub(crate) async fn warn_alias(
    container: &str,
    reference: &str,
    alias_of: Option<&str>,
    operation: Option<&crate::operation::Operation>,
) {
    let Some(canonical) = alias_of else { return };
    let (registry, name) = split_reference(reference);
    let detail = format!(
        "worker '{name}' is an alias of '{canonical}'. Using container '{container}'. \
         Use 'package://{}/{canonical}' for new references.",
        registry.trim_start_matches("https://"),
    );
    if let Some(operation) = operation {
        operation
            .warn_once(format!("{registry}/{name}/{canonical}"), container, detail)
            .await;
    } else {
        crate::report::daemon_line(&format!("warning: {detail}"), true);
    }
}

/// The rust target triple this daemon is running on, which is the one its
/// children have to run on too.
pub fn host_target() -> &'static str {
    // Build constants rather than a runtime probe: the daemon and its children
    // share a machine, so the triple compose was built for is the triple it
    // needs.
    target_triple(
        std::env::consts::ARCH,
        std::env::consts::OS,
        cfg!(target_env = "musl"),
    )
}

/// The triple of a build for `arch` and `os`; `musl` marks a static Linux
/// build. `unknown` for a platform the registry publishes no builds for.
fn target_triple(arch: &str, os: &str, musl: bool) -> &'static str {
    match (arch, os, musl) {
        ("x86_64", "linux", true) => "x86_64-unknown-linux-musl",
        ("x86_64", "linux", false) => "x86_64-unknown-linux-gnu",
        ("aarch64", "linux", true) => "aarch64-unknown-linux-musl",
        ("aarch64", "linux", false) => "aarch64-unknown-linux-gnu",
        ("x86_64", "macos", _) => "x86_64-apple-darwin",
        ("aarch64", "macos", _) => "aarch64-apple-darwin",
        ("x86_64", "windows", _) => "x86_64-pc-windows-msvc",
        ("aarch64", "windows", _) => "aarch64-pc-windows-msvc",
        _ => "unknown",
    }
}

/// The glibc and musl dynamic loaders of a Linux `arch`. A glibc distribution
/// has the first. A musl one (Alpine) has the second, and may have the first
/// too as a compatibility link to itself (libc6-compat, gcompat), so the musl
/// loader decides, as it does for the installer.
fn linux_loaders(arch: &str) -> Option<(&'static str, &'static str)> {
    match arch {
        "x86_64" => Some(("/lib64/ld-linux-x86-64.so.2", "/lib/ld-musl-x86_64.so.1")),
        "aarch64" => Some(("/lib/ld-linux-aarch64.so.1", "/lib/ld-musl-aarch64.so.1")),
        _ => None,
    }
}

/// Every triple this host can run, preferred first: [`host_target`], then,
/// for a static musl build on a glibc machine, the glibc one. That machine
/// runs both, and some workers publish only glibc builds (a C++ dependency
/// with no musl toolchain, or backends loaded as shared libraries).
pub fn host_targets() -> &'static [&'static str] {
    static TARGETS: std::sync::OnceLock<Vec<&'static str>> = std::sync::OnceLock::new();
    TARGETS.get_or_init(|| {
        let glibc_host = linux_loaders(std::env::consts::ARCH)
            .is_some_and(|(glibc, musl)| Path::new(glibc).exists() && !Path::new(musl).exists());
        runnable_targets(host_target(), glibc_host)
    })
}

fn runnable_targets(native: &'static str, glibc_host: bool) -> Vec<&'static str> {
    let mut targets = vec![native];
    if glibc_host && let Some(glibc) = glibc_counterpart(native) {
        targets.push(glibc);
    }
    targets
}

/// The glibc build of the same machine as a static musl `native` build.
fn glibc_counterpart(native: &str) -> Option<&'static str> {
    match native {
        "x86_64-unknown-linux-musl" => Some("x86_64-unknown-linux-gnu"),
        "aarch64-unknown-linux-musl" => Some("aarch64-unknown-linux-gnu"),
        _ => None,
    }
}

/// The first of `candidates` that `artifacts` has a build for.
fn first_available<'a, T>(
    candidates: &[&'a str],
    artifacts: &std::collections::BTreeMap<String, T>,
) -> Option<&'a str> {
    candidates
        .iter()
        .copied()
        .find(|target| artifacts.contains_key(*target))
}

/// The triple whose build this host installs out of `artifacts`: the first of
/// [`host_targets`] it has, else [`host_target`] (so the error names it).
pub(crate) fn runnable_target<T>(
    artifacts: &std::collections::BTreeMap<String, T>,
) -> &'static str {
    first_available(host_targets(), artifacts).unwrap_or(host_target())
}

/// The build a host with `candidates` (see [`host_targets`]) runs out of
/// `artifacts`, as its triple and lowercase digest: the first candidate it
/// has, else the first candidate with no digest.
pub(crate) fn selected_artifact<'a>(
    candidates: &[&'a str],
    artifacts: &std::collections::BTreeMap<String, RegistryArtifact>,
) -> (&'a str, Option<String>) {
    let target = first_available(candidates, artifacts).unwrap_or(candidates[0]);
    (
        target,
        artifacts
            .get(target)
            .map(|artifact| artifact.sha256.to_ascii_lowercase()),
    )
}

/// Splits `workers.iii.dev/state` into its registry base and worker name. A
/// reference with no host uses [`DEFAULT_REGISTRY`].
pub(crate) fn split_reference(reference: &str) -> (String, String) {
    match reference.split_once('/') {
        Some((host, name)) => (format!("https://{host}"), name.to_string()),
        None => (DEFAULT_REGISTRY.to_string(), reference.to_string()),
    }
}

/// Resolves a package reference and makes sure its binary is on disk.
///
/// Resolution is always refreshed; cached artifacts do not need another download.
pub async fn install(
    container: &str,
    reference: &str,
    version_range: &str,
    cache_root: &Path,
) -> Result<InstalledPackage> {
    let resolved = resolve_package(container, reference, version_range).await?;
    install_resolved(container, &resolved, cache_root).await
}

#[cfg(test)]
async fn install_from_registry(
    container: &str,
    registry: &str,
    name: &str,
    version_range: &str,
    cache_root: &Path,
) -> Result<InstalledPackage> {
    let worker = resolve(container, registry, name, version_range, host_targets()).await?;
    let resolved = into_resolved_package(container, registry, worker, host_target())?;
    install_resolved(container, &resolved, cache_root).await
}

/// Resolves a selector such as `next` to the immutable package metadata that
/// can be stored in a compose lock.
pub async fn resolve_package(
    container: &str,
    reference: &str,
    version_range: &str,
) -> Result<ResolvedPackage> {
    let (registry, name) = split_reference(reference);
    #[cfg(test)]
    let endpoint = TEST_REGISTRY
        .try_with(Clone::clone)
        .unwrap_or_else(|_| registry.clone());
    #[cfg(not(test))]
    let endpoint = registry.clone();
    let worker = resolve(container, &endpoint, &name, version_range, host_targets()).await?;
    into_resolved_package(container, &registry, worker, host_target())
}

/// Converts and validates one registry response for lock persistence.
fn into_resolved_package(
    container: &str,
    registry: &str,
    worker: ResolvedWorker,
    target: &str,
) -> Result<ResolvedPackage> {
    let mut artifacts = match worker.kind.as_str() {
        "binary" => worker.binaries,
        "bundle" => {
            let (Some(url), Some(sha256)) = (worker.archive_url, worker.sha256) else {
                return Err(ComposeError::PackageNotResolved {
                    container: container.to_string(),
                    name: worker.name,
                    range: worker.version,
                    message: "the registry resolved it as a bundle but sent no archive_url + \
                              sha256 pair, so the archive could not be verified"
                        .to_string(),
                });
            };
            std::iter::once((target.to_string(), RegistryArtifact { sha256, url })).collect()
        }
        _ => std::collections::BTreeMap::new(),
    };
    for (artifact_target, artifact) in &mut artifacts {
        if artifact.url.trim().is_empty() {
            return Err(ComposeError::PackageNotResolved {
                container: container.to_string(),
                name: worker.name,
                range: worker.version,
                message: format!("the registry returned an empty URL for {artifact_target}"),
            });
        }
        if artifact.sha256.len() != 64
            || !artifact.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(ComposeError::PackageNotResolved {
                container: container.to_string(),
                name: worker.name,
                range: worker.version,
                message: format!(
                    "the registry returned an invalid SHA-256 digest for {artifact_target}"
                ),
            });
        }
        artifact.sha256.make_ascii_lowercase();
    }
    let default_config = match worker.config {
        Some(config) if !config.is_null() => serde_yaml::to_value(config).ok(),
        _ => None,
    };
    Ok(ResolvedPackage {
        name: worker.name,
        alias_of: worker.alias_of,
        registry: registry.to_string(),
        version: worker.version,
        kind: worker.kind,
        artifacts,
        default_config,
    })
}

/// Installs exactly the package represented by a lock entry, without asking
/// the registry to resolve its selector again.
pub async fn install_resolved(
    container: &str,
    resolved: &ResolvedPackage,
    cache_root: &Path,
) -> Result<InstalledPackage> {
    let target = runnable_target(&resolved.artifacts);
    let cache_root = registry_cache_root(cache_root, resolved);

    let (payload, status) = match resolved.kind.as_str() {
        "binary" => {
            let (program, status) =
                install_binary(container, resolved, target, &cache_root).await?;
            (Payload::Binary(program), status)
        }
        // Refused before the download on a platform that could not start it:
        // the archive is megabytes, and an operator learns nothing from having
        // fetched it.
        #[cfg(not(unix))]
        "bundle" => {
            return Err(ComposeError::BundleNeedsAVm {
                container: container.to_string(),
                name: resolved.name.clone(),
            });
        }
        #[cfg(unix)]
        "bundle" => {
            let (install_dir, status) =
                install_bundle(container, resolved, target, &cache_root).await?;
            // The registry filters by `iii_version` when it resolves, but a bundle from
            // `worker-compose.lock` is never resolved; cache hits come through here too.
            crate::manifest::check_bundle_engine(container, &install_dir)?;
            (Payload::Bundle(install_dir), status)
        }
        // `engine` workers are compiled into the engine itself: there is no
        // artefact to install and nothing for compose to start. `image` workers
        // have one, but running it needs the OCI runtime.
        _ => {
            return Err(ComposeError::UnsupportedPackageKind {
                container: container.to_string(),
                name: resolved.name.clone(),
                kind: resolved.kind.clone(),
            });
        }
    };

    Ok(InstalledPackage {
        name: resolved.name.clone(),
        alias_of: resolved.alias_of.clone(),
        version: resolved.version.clone(),
        payload,
        default_config: resolved.default_config.clone(),
        status,
    })
}

/// `.workers/.packages.json`: which artifact each package directory holds (spec 3.2, 11.2).
pub(crate) const PROJECT_PACKAGES_FILE: &str = ".packages.json";
/// The `schema` of the `.packages.json` this version writes and reads.
const PROJECT_PACKAGES_SCHEMA: u32 = 1;

/// `.workers/.packages.json`: `{ "schema": 1, "containers": { <container>: { name, version,
/// sha256 } } }`. It sits outside the package directories, so no guest can write it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ProjectPackages {
    pub schema: u32,
    pub containers: BTreeMap<String, ProjectPackage>,
}

impl Default for ProjectPackages {
    fn default() -> Self {
        Self {
            schema: PROJECT_PACKAGES_SCHEMA,
            containers: BTreeMap::new(),
        }
    }
}

/// The artifact one `.workers/<container>/` holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ProjectPackage {
    /// The package; for an alias, the package it stands for, as the cache names it.
    pub name: String,
    pub version: String,
    /// `sha256:<hex>` of the archive this host installs.
    pub sha256: String,
}

impl ProjectPackage {
    /// The artifact `resolved` installs on this host.
    fn locked(container: &str, resolved: &ResolvedPackage) -> Result<Self> {
        let target = runnable_target(&resolved.artifacts);
        let artifact = artifact_for_target(container, resolved, target)?;
        let digest = validated_cache_digest(container, resolved, &artifact.sha256)?;
        Ok(Self {
            name: package_name(resolved).to_string(),
            version: resolved.version.clone(),
            sha256: format!("sha256:{digest}"),
        })
    }
}

/// Reads `.workers/.packages.json` of the project in `compose_dir`; `None` when there is none.
pub(crate) fn read_project_packages(compose_dir: &Path) -> Result<Option<ProjectPackages>> {
    let path = compose_dir
        .join(crate::build_record::WORKERS_DIR)
        .join(PROJECT_PACKAGES_FILE);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(ComposeError::Io { path, source }),
    };
    crate::build_record::parse_versioned(
        &path,
        &bytes,
        PROJECT_PACKAGES_SCHEMA,
        "`iii compose build` rewrites it",
    )
    .map(Some)
}

/// The lock file in `.workers/` that one materialization holds at a time.
const WORKERS_LOCK_FILE: &str = ".lock";

/// Makes `.workers/` match the lock: copies missing or changed packages from the cache, removes
/// the directories of containers no longer locked, rewrites `.workers/.packages.json`.
///
/// - One run at a time per project: the daemon and `iii compose build` both materialize, so a
///   run holds `.workers/.lock` from its first change to its last, its disk jobs included.
/// - A directory whose `.packages.json` entry matches the lock is left as it is, with whatever
///   a host build wrote into it. Any other is replaced whole by a copy of the cache entry, made
///   beside it and renamed into place; the cache entry is verified first, and downloaded again
///   when it is missing or damaged. The cache itself is only read. Its entry leaves
///   `.packages.json` before it is replaced, and comes back once the copy is in place: a
///   replacement that never finished is copied again by the next run, never taken for the
///   package it replaced. An entry whose directory is gone leaves the file before any copy.
///   The tree it replaced stays aside until the next run: see [`replace_directory`].
/// - A directory made again for the artifact it held (it was gone, or `.packages.json` could
///   not be read) loses what a host build wrote into it, so that build's record
///   (`.build/<container>/build.json` without `image`, made from this same package) is deleted
///   first, and the container gets its first build again (spec 5.2). A package update keeps
///   the record: its source no longer matches, `build_record::changes` reports it, and the
///   container rebuilds as spec 5.3 says. A VM build keeps its record: its image holds its own
///   copy of the package.
/// - Only what Compose made is removed: the directories `.packages.json` recorded for containers
///   no longer locked, the trees an earlier run replaced, and the temporary copies and files an
///   interrupted run left. Anything else in `.workers/` stays.
/// - A project with no package and no `.workers/` gets none.
pub async fn materialize_project(
    compose_dir: &Path,
    cache_root: &Path,
    lock: &PreparedLock,
) -> Result<()> {
    materialize_project_checked(compose_dir, cache_root, lock, || Ok(())).await
}

/// [`materialize_project`], calling `check` before anything changes, once `.workers/.lock` is
/// held: `iii compose build` makes sure there that the compose file and the lock it worked from
/// are still the ones on disk, so a stale build never undoes the daemon's newer copy.
pub(crate) async fn materialize_project_checked(
    compose_dir: &Path,
    cache_root: &Path,
    lock: &PreparedLock,
    check: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let workers = compose_dir.join(crate::build_record::WORKERS_DIR);
    let mut wanted = BTreeMap::new();
    for (container, resolved) in lock.packages() {
        if !crate::build_record::is_directory_name(container) {
            return Err(ComposeError::Io {
                path: workers.join(container),
                source: std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("container key '{container}' cannot name a directory in .workers/"),
                ),
            });
        }
        wanted.insert(
            container.to_string(),
            (resolved, ProjectPackage::locked(container, resolved)?),
        );
    }
    if wanted.is_empty() && std::fs::symlink_metadata(&workers).is_err() {
        return check();
    }
    std::fs::create_dir_all(&workers).map_err(io_error(&workers))?;
    // Shared with every disk job below: a job outlives a dropped future until it next checks
    // its cancellation, and must not meet another run in `.workers/` meanwhile.
    let guard = std::sync::Arc::new(lock_exclusive(&workers.join(WORKERS_LOCK_FILE)).await?);
    // Before the check: `.workers/.lock` exists now, and must stay ignored even when the check
    // fails.
    crate::build_record::ignore_all(&workers)?;
    check()?;

    // An unreadable record only costs copies: every directory is then made again, and none is
    // pruned, since none is known to be Compose's.
    let previous = read_project_packages(compose_dir)
        .ok()
        .flatten()
        .unwrap_or_default();
    let retired: std::collections::BTreeSet<String> = previous
        .containers
        .keys()
        .filter(|container| {
            !wanted.contains_key(*container) && crate::build_record::is_directory_name(container)
        })
        .cloned()
        .collect();
    let (pruned, held) = (workers.clone(), std::sync::Arc::clone(&guard));
    crate::shutdown::blocking(workers.clone(), move |cancel| {
        let _held = held;
        prune_packages(&pruned, &retired, &cancel)
    })
    .await?;

    // A directory that is gone takes its entry with it, so a failure below leaves no entry
    // naming a package that is not on disk.
    let found = previous.containers.len();
    let mut record = ProjectPackages {
        containers: previous
            .containers
            .into_iter()
            .filter(|(container, _)| {
                wanted.contains_key(container)
                    && std::fs::symlink_metadata(workers.join(container))
                        .is_ok_and(|metadata| metadata.is_dir())
            })
            .collect(),
        ..ProjectPackages::default()
    };
    // On disk too, before any directory changes: an entry whose directory is gone must not
    // outlive a copy that this run starts and does not finish.
    if record.containers.len() != found {
        write_project_packages(&workers, &record)?;
    }
    let mut failure = None;
    for (container, (resolved, package)) in &wanted {
        if record.containers.get(container) == Some(package) {
            continue;
        }
        // Out of `.packages.json` before the directory changes: a run stopped before the copy is
        // in place (a crash, a kill) leaves no entry that would let the next one skip it.
        let previous = record.containers.remove(container);
        let invalidated = match previous {
            Some(_) => write_project_packages(&workers, &record),
            None => Ok(()),
        };
        // On failure the previous directory is still in place, and so is its entry.
        let copied =
            match invalidated.and_then(|()| forget_host_build(compose_dir, container, package)) {
                Ok(()) => copy_package(container, resolved, cache_root, &workers, &guard).await,
                Err(error) => Err(error),
            };
        if let Err(error) = copied {
            if let Some(previous) = previous {
                record.containers.insert(container.clone(), previous);
            }
            failure = Some(error);
            break;
        }
        record.containers.insert(container.clone(), package.clone());
    }
    let written = write_project_packages(&workers, &record);
    match failure {
        Some(error) => Err(error),
        None => written,
    }
}

/// Deletes `.build/<container>/build.json` when it records a host build made from `package`,
/// the artifact about to be copied again: see [`materialize_project`]. No record, a VM build's,
/// one made from another package, or one that does not parse (which already asks for a
/// rebuild) is left alone.
fn forget_host_build(compose_dir: &Path, container: &str, package: &ProjectPackage) -> Result<()> {
    let build = crate::build_record::build_dir(compose_dir, container);
    let Ok(Some(record)) = crate::build_record::read(&build) else {
        return Ok(());
    };
    let same_package = crate::build_record::BuildSource::Package {
        name: package.name.clone(),
        version: package.version.clone(),
        sha256: package.sha256.clone(),
    };
    if record.image.is_some() || record.source != same_package {
        return Ok(());
    }
    let path = build.join(crate::build_record::BUILD_FILE);
    match std::fs::remove_file(&path) {
        Err(source) if source.kind() != std::io::ErrorKind::NotFound => {
            Err(ComposeError::Io { path, source })
        }
        _ => Ok(()),
    }
}

/// Removes from `workers` what Compose made and no longer needs: the directories of `retired`
/// containers, and what an interrupted run left: the temporary copies of
/// [`replace_directory`], and the temporary files of `.packages.json` and `.gitignore`.
fn prune_packages(
    workers: &Path,
    retired: &std::collections::BTreeSet<String>,
    cancel: &crate::shutdown::BlockingCancellation,
) -> Result<()> {
    for entry in std::fs::read_dir(workers).map_err(io_error(workers))? {
        cancel.check(workers)?;
        let entry = entry.map_err(io_error(workers))?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if retired.contains(name) || is_temporary_copy(name) || is_temporary_file(name) {
            let path = entry.path();
            remove_path(&path).map_err(io_error(&path))?;
        }
    }
    Ok(())
}

/// `.<container>.staging-<id>` or `.<container>.replaced-<id>`, the names
/// [`replace_directory`] gives its temporary directories.
fn is_temporary_copy(name: &str) -> bool {
    name.strip_prefix('.')
        .and_then(|rest| rest.rsplit_once('.'))
        .is_some_and(|(container, suffix)| {
            crate::build_record::is_directory_name(container)
                && (suffix.starts_with("staging-") || suffix.starts_with("replaced-"))
        })
}

/// `.packages.json.tmp-<id>` or `.gitignore.tmp-<id>`, the temporary files
/// `build_record::replace_file` writes Compose's own files of `.workers/` through.
fn is_temporary_file(name: &str) -> bool {
    [PROJECT_PACKAGES_FILE, ".gitignore"].iter().any(|file| {
        name.strip_prefix(file)
            .is_some_and(|rest| rest.starts_with(".tmp-"))
    })
}

/// Replaces `<workers>/<container>` with a copy of the package's cache entry; the copy holds
/// `guard`, the project's `.workers/.lock`, until it has finished.
async fn copy_package(
    container: &str,
    resolved: &ResolvedPackage,
    cache_root: &Path,
    workers: &Path,
    guard: &std::sync::Arc<fslock::LockFile>,
) -> Result<()> {
    // Verifies the entry, and downloads it again when it is missing or damaged.
    install_resolved(container, resolved, cache_root).await?;
    let target = runnable_target(&resolved.artifacts);
    let artifact = artifact_for_target(container, resolved, target)?;
    let digest = validated_cache_digest(container, resolved, &artifact.sha256)?;
    let source = cache_install_dir(
        &registry_cache_root(cache_root, resolved),
        resolved,
        target,
        &digest,
    );
    // Held while copying: an install repairing this entry waits for the copy to finish.
    let lock = lock_artifact(&source).await?;
    let held = std::sync::Arc::clone(guard);
    let destination = workers.join(container);
    crate::shutdown::blocking(destination.clone(), move |cancel| {
        let (_lock, _held) = (lock, held);
        replace_directory(&source, &destination, &cancel)
    })
    .await
}

/// Copies `source` beside `destination`, then swaps the copy into place: the old directory is
/// renamed aside first, so `destination` is only ever missing between two renames. The old tree
/// stays aside, whole, until the next materialization prunes it with any leftover staging copy
/// ([`prune_packages`]), whether a process still runs from it or not. A host bundle that runs
/// through the swap keeps what it reads relative to its working directory, which moved with the
/// rename; what it resolves through an absolute path (`III_PACKAGE_DIR`, Node's `require`,
/// Python's `sys.path`) reads the new tree.
fn replace_directory(
    source: &Path,
    destination: &Path,
    cancel: &crate::shutdown::BlockingCancellation,
) -> Result<()> {
    let parent = destination.parent().unwrap_or(Path::new("."));
    let name = destination
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let unique = uuid::Uuid::new_v4();
    let staging = parent.join(format!(".{name}.staging-{unique}"));

    struct Staging(PathBuf);
    impl Drop for Staging {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _staging = Staging(staging.clone());
    copy_tree(source, &staging, true, cancel)?;
    cancel.check(destination)?;

    match std::fs::symlink_metadata(destination) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            std::fs::rename(&staging, destination).map_err(io_error(destination))
        }
        Err(error) => Err(io_error(destination)(error)),
        Ok(_) => {
            let aside = parent.join(format!(".{name}.replaced-{unique}"));
            std::fs::rename(destination, &aside).map_err(io_error(destination))?;
            if let Err(error) = std::fs::rename(&staging, destination) {
                let _ = std::fs::rename(&aside, destination);
                return Err(io_error(destination)(error));
            }
            Ok(())
        }
    }
}

/// Copies the tree at `from` to a new directory `to`, without the cache's integrity marker at
/// the root. Files keep their permissions and symlinks stay symlinks (on Windows, where creating
/// one needs a privilege, the file it points to is copied instead); directories get the default
/// mode, so the copy can always be replaced.
fn copy_tree(
    from: &Path,
    to: &Path,
    root: bool,
    cancel: &crate::shutdown::BlockingCancellation,
) -> Result<()> {
    cancel.check(from)?;
    std::fs::create_dir(to).map_err(io_error(to))?;
    for entry in std::fs::read_dir(from).map_err(io_error(from))? {
        let entry = entry.map_err(io_error(from))?;
        if root && entry.file_name() == INTEGRITY_FILE {
            continue;
        }
        let source = entry.path();
        let target = to.join(entry.file_name());
        cancel.check(&source)?;
        let kind = entry.file_type().map_err(io_error(&source))?;
        if kind.is_dir() {
            copy_tree(&source, &target, false, cancel)?;
        } else if kind.is_symlink() {
            #[cfg(unix)]
            std::fs::read_link(&source)
                .and_then(|link| std::os::unix::fs::symlink(link, &target))
                .map_err(io_error(&source))?;
            #[cfg(not(unix))]
            std::fs::copy(&source, &target).map_err(io_error(&source))?;
        } else if kind.is_file() {
            std::fs::copy(&source, &target).map_err(io_error(&source))?;
        }
        // Anything else, a socket or a FIFO, is not part of a package.
    }
    Ok(())
}

/// Removes `path`, a directory tree or a single file or symlink.
fn remove_path(path: &Path) -> std::io::Result<()> {
    if std::fs::symlink_metadata(path)?.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

/// Turns an I/O error on `path` into [`ComposeError::Io`], for `map_err`.
fn io_error(path: &Path) -> impl FnOnce(std::io::Error) -> ComposeError {
    let path = path.to_path_buf();
    move |source| ComposeError::Io { path, source }
}

/// Writes `.workers/.packages.json` atomically.
fn write_project_packages(workers: &Path, record: &ProjectPackages) -> Result<()> {
    let path = workers.join(PROJECT_PACKAGES_FILE);
    let mut json = serde_json::to_vec_pretty(record).map_err(|error| ComposeError::Io {
        path: path.clone(),
        source: std::io::Error::other(error),
    })?;
    json.push(b'\n');
    crate::build_record::replace_file(&path, &json)
}

/// The version the registry hands back for `*`, so `compose::add` can pin what
/// it just resolved rather than writing a range that drifts under the operator.
pub async fn latest_version(container: &str, reference: &str) -> Result<String> {
    let resolved = resolve_package(container, reference, "*").await?;
    Ok(resolved.version)
}

/// One worker in a resolved graph.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Node {
    pub name: String,
    pub alias_of: Option<String>,
    pub version: String,
    /// `binary`, `bundle`, `engine`, `image` — what it is, and therefore
    /// whether compose can run it at all.
    pub kind: String,
    /// Digest for this host, or the bundle archive. Used to compare resolutions.
    pub artifact_digest: Option<String>,
    pub default_config: Option<serde_json::Value>,
}

impl Node {
    pub fn canonical_name(&self) -> &str {
        self.alias_of.as_deref().unwrap_or(&self.name)
    }

    pub(crate) fn same_release(&self, other: &Self) -> bool {
        self.version == other.version
            && self.kind == other.kind
            && self.artifact_digest == other.artifact_digest
            && self.default_config == other.default_config
    }
}

impl From<ResolvedWorker> for Node {
    fn from(worker: ResolvedWorker) -> Self {
        let artifact_digest = if worker.kind == "bundle" {
            worker.sha256.as_deref()
        } else {
            worker
                .binaries
                .get(runnable_target(&worker.binaries))
                .map(|artifact| artifact.sha256.as_str())
        }
        .map(str::to_ascii_lowercase);
        Self {
            name: worker.name,
            alias_of: worker.alias_of,
            version: worker.version,
            kind: worker.kind,
            artifact_digest,
            default_config: worker.config.filter(|config| !config.is_null()),
        }
    }
}

impl From<&ResolvedPackage> for Node {
    fn from(package: &ResolvedPackage) -> Self {
        Self {
            name: package.name.clone(),
            alias_of: package.alias_of.clone(),
            version: package.version.clone(),
            kind: package.kind.clone(),
            artifact_digest: package
                .artifacts
                .get(runnable_target(&package.artifacts))
                .map(|artifact| artifact.sha256.to_ascii_lowercase()),
            default_config: package
                .default_config
                .as_ref()
                .and_then(|config| serde_json::to_value(config).ok()),
        }
    }
}

pub(crate) async fn resolve_node(container: &str, reference: &str, range: &str) -> Result<Node> {
    let (registry, name) = split_reference(reference);
    #[cfg(test)]
    let registry = TEST_REGISTRY.try_with(Clone::clone).unwrap_or(registry);
    resolve(container, &registry, &name, range, host_targets())
        .await
        .map(Node::from)
}

/// A resolved graph: what to declare, and what each one needs.
#[derive(Debug, Clone, Default)]
pub struct Graph {
    pub nodes: Vec<Node>,
    /// `(from, to)`: `from` calls `to`.
    pub edges: Vec<(String, String)>,
}

#[cfg(test)]
tokio::task_local! {
    pub(crate) static TEST_REGISTRY: String;
}

/// Everything a worker needs, resolved in one request.
///
/// The registry answers `/resolve` with the whole graph, not just the worker
/// named: its dependencies come back already pinned to versions that satisfy
/// each other. Asking again per dependency would be slower and could resolve a
/// different set, because each answer is computed on its own.
pub async fn resolve_graph(container: &str, reference: &str, version_range: &str) -> Result<Graph> {
    let (registry, name) = split_reference(reference);
    #[cfg(test)]
    let registry = TEST_REGISTRY.try_with(Clone::clone).unwrap_or(registry);
    let response =
        resolve_response(container, &registry, &name, version_range, host_targets()).await?;
    Ok(Graph {
        nodes: response.graph.into_iter().map(Node::from).collect(),
        edges: response
            .edges
            .into_iter()
            .map(|edge| (edge.from, edge.to))
            .collect(),
    })
}

/// The part of the cache that holds `resolved`'s registry: `cache_root` itself for
/// [`DEFAULT_REGISTRY`], else a directory named after the hash of the registry's URL.
fn registry_cache_root(cache_root: &Path, resolved: &ResolvedPackage) -> PathBuf {
    if resolved.registry == DEFAULT_REGISTRY {
        cache_root.to_path_buf()
    } else {
        cache_root.join(hex::encode(Sha256::digest(resolved.registry.as_bytes())))
    }
}

/// The directory one artifact is extracted to under its registry's `cache_root`, keyed by
/// package, version, target and digest: `<name>-<version>-<target>-<digest>`. A bundle has one
/// archive for every platform, so `bundle` stands in for the target. An alias shares the entry of
/// the package it stands for.
fn cache_install_dir(
    cache_root: &Path,
    resolved: &ResolvedPackage,
    target: &str,
    digest: &str,
) -> PathBuf {
    let platform = if resolved.kind == "bundle" {
        "bundle"
    } else {
        target
    };
    cache_root.join(format!(
        "{}-{}-{platform}-{digest}",
        package_name(resolved),
        resolved.version
    ))
}

/// The package `resolved` installs: for an alias, the package it stands for.
fn package_name(resolved: &ResolvedPackage) -> &str {
    resolved.alias_of.as_deref().unwrap_or(&resolved.name)
}

/// Installs a native executable for this host's target.
async fn install_binary(
    container: &str,
    resolved: &ResolvedPackage,
    target: &str,
    cache_root: &Path,
) -> Result<(PathBuf, InstallStatus)> {
    let artifact = artifact_for_target(container, resolved, target)?;

    let digest = validated_cache_digest(container, resolved, &artifact.sha256)?;
    let install_dir = cache_install_dir(cache_root, resolved, target, &digest);
    let (lock, cached) = checked_cache(&install_dir, &digest).await?;
    if cached && let Some(existing) = installed_binary(&install_dir) {
        return Ok((existing, InstallStatus::Cached));
    }
    crate::report::download_version(container, &resolved.version);
    download_and_extract(container, artifact, &install_dir, &digest, lock).await?;
    let program =
        installed_binary(&install_dir).ok_or_else(|| ComposeError::PackageArtifactEmpty {
            container: container.to_string(),
            name: resolved.name.clone(),
            path: install_dir.clone(),
        })?;
    Ok((program, InstallStatus::Downloaded))
}

/// Selects the artifact that can run on this Compose host.
fn artifact_for_target<'a>(
    container: &str,
    resolved: &'a ResolvedPackage,
    target: &str,
) -> Result<&'a RegistryArtifact> {
    resolved
        .artifacts
        .get(target)
        .ok_or_else(|| ComposeError::UnsupportedPlatform {
            container: container.to_string(),
            name: resolved.name.clone(),
            version: resolved.version.clone(),
            target: target.to_string(),
            available: resolved
                .artifacts
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(", "),
        })
}

/// Installs a bundle: one archive, no target in its identity.
///
/// Unix only, because starting one is: see [`ComposeError::BundleNeedsAVm`].
///
/// The install is keyed by name, version and digest under Compose's package
/// cache, so two projects pinning different versions of the same bundle never
/// overwrite each other between one `up` and the next.
#[cfg(unix)]
async fn install_bundle(
    container: &str,
    resolved: &ResolvedPackage,
    target: &str,
    cache_root: &Path,
) -> Result<(PathBuf, InstallStatus)> {
    let artifact = artifact_for_target(container, resolved, target)?;

    let digest = validated_cache_digest(container, resolved, &artifact.sha256)?;
    let install_dir = cache_install_dir(cache_root, resolved, target, &digest);
    let (lock, cached) = checked_cache(&install_dir, &digest).await?;
    // The manifest is the bundle's entry point, so its presence is what makes
    // an install dir a cache hit — not the first executable, which a bundle
    // need not have at all.
    if cached && install_dir.join(BUNDLE_MANIFEST).is_file() {
        return Ok((install_dir, InstallStatus::Cached));
    }
    crate::report::download_version(container, &resolved.version);
    download_and_extract(container, artifact, &install_dir, &digest, lock).await?;

    if !install_dir.join(BUNDLE_MANIFEST).is_file() {
        return Err(ComposeError::PackageArtifactEmpty {
            container: container.to_string(),
            name: resolved.name.clone(),
            path: install_dir.clone(),
        });
    }
    Ok((install_dir, InstallStatus::Downloaded))
}

/// The kernel releases this lock on cancellation or process exit. Keep the lock
/// file in place so every process continues to lock the same inode.
async fn lock_artifact(install_dir: &Path) -> Result<fslock::LockFile> {
    let mut path = install_dir.as_os_str().to_os_string();
    path.push(".lock");
    lock_exclusive(&PathBuf::from(path)).await
}

/// Takes the exclusive lock of the file at `path`, creating it and its parent, and waits while
/// another holder has it: in another process, or through another handle in this one.
async fn lock_exclusive(path: &Path) -> Result<fslock::LockFile> {
    let path = path.to_path_buf();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| ComposeError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    let mut lock = fslock::LockFile::open(&path).map_err(|source| ComposeError::Io {
        path: path.clone(),
        source,
    })?;
    loop {
        if lock.try_lock().map_err(|source| ComposeError::Io {
            path: path.clone(),
            source,
        })? {
            return Ok(lock);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Returns the normalized digest used as the immutable part of a cache key.
fn validated_cache_digest(
    container: &str,
    resolved: &ResolvedPackage,
    sha256: &str,
) -> Result<String> {
    if sha256.len() == 64 && sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Ok(sha256.to_ascii_lowercase());
    }

    Err(ComposeError::PackageNotResolved {
        container: container.to_string(),
        name: resolved.name.clone(),
        range: resolved.version.clone(),
        message: "the registry returned an invalid SHA-256 digest".to_string(),
    })
}

/// Removes a stale cache entry that cannot be used as an installed package.
fn remove_invalid_install(path: &Path) -> Result<()> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            return Err(ComposeError::Io {
                path: path.to_path_buf(),
                source,
            });
        }
    };

    let result = if metadata.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    };
    result.map_err(|source| ComposeError::Io {
        path: path.to_path_buf(),
        source,
    })
}

/// The blocking job retains the artifact lock even when its caller is dropped.
async fn checked_cache(install_dir: &Path, digest: &str) -> Result<(fslock::LockFile, bool)> {
    let lock = lock_artifact(install_dir).await?;
    let path = install_dir.to_path_buf();
    let digest = digest.to_string();
    crate::shutdown::blocking(path.clone(), move |cancel| {
        let cached = cache_matches(&path, &digest, &cancel)?;
        Ok((lock, cached))
    })
    .await
}

/// Verifies both the archive identity and the extracted files in one cache entry.
fn cache_matches(
    install_dir: &Path,
    archive_sha256: &str,
    cancel: &crate::shutdown::BlockingCancellation,
) -> Result<bool> {
    cancel.check(install_dir)?;
    let marker = install_dir.join(INTEGRITY_FILE);
    let bytes = match std::fs::read(&marker) {
        Ok(bytes) => bytes,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(source) => {
            return Err(ComposeError::Io {
                path: marker,
                source,
            });
        }
    };
    let integrity: CacheIntegrity = match serde_json::from_slice(&bytes) {
        Ok(integrity) => integrity,
        Err(_) => return Ok(false),
    };
    if !integrity
        .archive_sha256
        .eq_ignore_ascii_case(archive_sha256)
    {
        return Ok(false);
    }
    Ok(tree_digest(install_dir, cancel)? == integrity.tree_sha256)
}

/// Hashes the extracted tree in stable path order, excluding its own marker.
fn tree_digest(root: &Path, cancel: &crate::shutdown::BlockingCancellation) -> Result<String> {
    fn visit(
        root: &Path,
        directory: &Path,
        hasher: &mut Sha256,
        cancel: &crate::shutdown::BlockingCancellation,
    ) -> Result<()> {
        cancel.check(directory)?;
        let mut entries = std::fs::read_dir(directory)
            .map_err(|source| ComposeError::Io {
                path: directory.to_path_buf(),
                source,
            })?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|source| ComposeError::Io {
                path: directory.to_path_buf(),
                source,
            })?;
        entries.sort_by_key(std::fs::DirEntry::file_name);

        for entry in entries {
            cancel.check(directory)?;
            let path = entry.path();
            if path == root.join(INTEGRITY_FILE) {
                continue;
            }
            let relative = path.strip_prefix(root).unwrap_or(&path);
            let name = relative.to_string_lossy().replace('\\', "/");
            hasher.update((name.len() as u64).to_be_bytes());
            hasher.update(name.as_bytes());
            let metadata = std::fs::symlink_metadata(&path).map_err(|source| ComposeError::Io {
                path: path.clone(),
                source,
            })?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                hasher.update(metadata.permissions().mode().to_be_bytes());
            }

            if metadata.file_type().is_dir() {
                hasher.update(b"directory");
                visit(root, &path, hasher, cancel)?;
            } else if metadata.file_type().is_file() {
                hasher.update(b"file");
                let mut file = std::fs::File::open(&path).map_err(|source| ComposeError::Io {
                    path: path.clone(),
                    source,
                })?;
                let mut buffer = [0_u8; 64 * 1024];
                loop {
                    cancel.check(&path)?;
                    let read = file.read(&mut buffer).map_err(|source| ComposeError::Io {
                        path: path.clone(),
                        source,
                    })?;
                    if read == 0 {
                        break;
                    }
                    hasher.update(&buffer[..read]);
                }
            } else if metadata.file_type().is_symlink() {
                hasher.update(b"symlink");
                let target = std::fs::read_link(&path).map_err(|source| ComposeError::Io {
                    path: path.clone(),
                    source,
                })?;
                hasher.update(target.to_string_lossy().as_bytes());
            }
        }
        Ok(())
    }

    let mut hasher = Sha256::new();
    visit(root, root, &mut hasher, cancel)?;
    Ok(hex::encode(hasher.finalize()))
}

fn write_integrity_marker(
    install_dir: &Path,
    archive_sha256: &str,
    cancel: &crate::shutdown::BlockingCancellation,
) -> Result<()> {
    let marker = install_dir.join(INTEGRITY_FILE);
    let integrity = CacheIntegrity {
        archive_sha256: archive_sha256.to_ascii_lowercase(),
        tree_sha256: tree_digest(install_dir, cancel)?,
    };
    let bytes = serde_json::to_vec(&integrity).map_err(|source| ComposeError::Io {
        path: marker.clone(),
        source: std::io::Error::other(source),
    })?;
    std::fs::write(&marker, bytes).map_err(|source| ComposeError::Io {
        path: marker,
        source,
    })
}

/// The raw `/resolve` answer: the worker and everything it depends on.
/// Resolves for the first of `candidates` (normally [`host_targets`]), moving
/// on to a later one only when the registry says the worker has no build for
/// the platform asked.
async fn resolve_response(
    container: &str,
    registry: &str,
    name: &str,
    version_range: &str,
    candidates: &[&str],
) -> Result<ResolveResponse> {
    let mut builder = reqwest::Client::builder().timeout(RESOLVE_ATTEMPT_TIMEOUT);
    if let Some(version) = crate::host_version() {
        builder = builder.user_agent(format!("iii/{version}"));
    }
    let client = builder
        .build()
        .map_err(|err| registry_error(container, registry, &err.to_string()))?;

    let endpoint = format!("{registry}/resolve");
    let mut index = 0;
    let response = loop {
        let mut request = serde_json::json!({
            "worker": name,
            "version": version_range,
            "target": candidates[index],
        });
        if let Some(version) = crate::host_version() {
            request["iii_version"] = version.into();
        }
        let response = send_resolve_request(&client, &endpoint, &request)
            .await
            .map_err(|err| registry_error(container, registry, &err.to_string()))?;
        if response.status().is_success() {
            break response;
        }
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if let Some(next) = fallback_index(&body, candidates, index) {
            index = next;
            continue;
        }
        if let Some(available) = refused_platform(&body) {
            return Err(ComposeError::UnsupportedPlatform {
                container: container.to_string(),
                name: name.to_string(),
                version: version_range.to_string(),
                target: candidates[index].to_string(),
                available: available.join(", "),
            });
        }
        return Err(ComposeError::PackageNotResolved {
            container: container.to_string(),
            name: name.to_string(),
            range: version_range.to_string(),
            message: registry_message(status.as_u16(), &body),
        });
    };

    let resolved: ResolveResponse = response
        .json()
        .await
        .map_err(|err| registry_error(container, registry, &err.to_string()))?;

    check_names(container, registry, &resolved)?;
    Ok(resolved)
}

/// The index of the next of `candidates` after `tried` to resolve for: only
/// when the registry refused `tried` as a platform the worker has no build
/// for (`unsupported_platform`), and only one it lists as available.
fn fallback_index(body: &str, candidates: &[&str], tried: usize) -> Option<usize> {
    let available = refused_platform(body)?;
    (tried + 1..candidates.len())
        .find(|&index| available.iter().any(|target| target == candidates[index]))
}

/// The targets an `unsupported_platform` refusal lists as available, or
/// `None` when `body` is not that refusal.
fn refused_platform(body: &str) -> Option<Vec<String>> {
    let parsed = serde_json::from_str::<serde_json::Value>(body).ok()?;
    let error = parsed.get("error")?;
    if error.get("code")?.as_str()? != "unsupported_platform" {
        return None;
    }
    Some(
        error
            .get("available")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_str)
            .map(str::to_string)
            .collect(),
    )
}

async fn send_resolve_request(
    client: &reqwest::Client,
    endpoint: &str,
    request: &serde_json::Value,
) -> std::result::Result<reqwest::Response, reqwest::Error> {
    for attempt in 1..RESOLVE_ATTEMPTS {
        let result = client.post(endpoint).json(request).send().await;
        let should_retry = match &result {
            Err(_) => true,
            Ok(response) => {
                let status = response.status();
                status.is_server_error()
                    || status == reqwest::StatusCode::REQUEST_TIMEOUT
                    || status == reqwest::StatusCode::TOO_MANY_REQUESTS
            }
        };

        if !should_retry {
            return result;
        }

        tokio::time::sleep(RESOLVE_RETRY_DELAY * attempt as u32).await;
    }

    client.post(endpoint).json(request).send().await
}

/// Whether a value from the registry may be used to build a path.
///
/// Compose installs into a directory named after the package, so a name or
/// version holding a separator or `..` would place the download outside the
/// cache. The registry is whatever `package://<host>/…` names, so this is not a
/// check on our own service: it is a check on wherever compose was pointed.
pub(crate) fn is_path_safe(value: &str) -> bool {
    !value.is_empty()
        && value != ".."
        && !value.starts_with('.')
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// Refuses a resolve answer before anything in it reaches the filesystem.
///
/// The whole graph is checked, not only the worker asked for: `compose::add`
/// can turn each node into a declaration. Even a node below a local dependency
/// boundary remains untrusted registry input and must carry a safe name.
fn check_names(container: &str, registry: &str, resolved: &ResolveResponse) -> Result<()> {
    let refuse = |field: &str, value: &str| ComposeError::RegistryNameRefused {
        container: container.to_string(),
        registry: registry.to_string(),
        field: field.to_string(),
        value: value.to_string(),
    };
    for worker in &resolved.graph {
        if !is_path_safe(&worker.name) {
            return Err(refuse("name", &worker.name));
        }
        if !is_path_safe(&worker.version) {
            return Err(refuse("version", &worker.version));
        }
        if let Some(alias_of) = &worker.alias_of
            && !is_path_safe(alias_of)
        {
            return Err(refuse("alias_of", alias_of));
        }
    }
    Ok(())
}

/// The named worker out of its own graph.
///
/// Installing takes only the root: what a project runs is what its compose file
/// declares. `compose::add` is where the rest of the graph is turned into
/// declarations, so an operator sees them before they run.
async fn resolve(
    container: &str,
    registry: &str,
    name: &str,
    version_range: &str,
    candidates: &[&str],
) -> Result<ResolvedWorker> {
    let resolved = resolve_response(container, registry, name, version_range, candidates).await?;
    resolved
        .graph
        .into_iter()
        .find(|worker| worker.name == name)
        .ok_or_else(|| ComposeError::PackageNotResolved {
            container: container.to_string(),
            name: name.to_string(),
            range: version_range.to_string(),
            message: "the registry resolved a graph that does not contain it".to_string(),
        })
}

/// Turns the registry's failure body into one sentence.
///
/// It answers with `{"error": {message, fix, available}}` — already written for
/// a human, and it knows things we do not, like which versions exist. Printing
/// the raw body instead would put a JSON blob in the operator's terminal; a
/// body we do not recognise falls back to it anyway, since even that beats a
/// bare status code.
fn registry_message(status: u16, body: &str) -> String {
    let Some(error) = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|parsed| parsed.get("error").cloned())
    else {
        return format!("HTTP {status}: {}", body.trim());
    };

    let text = |key: &str| {
        error
            .get(key)
            .and_then(|value| value.as_str())
            .map(str::to_string)
    };

    let Some(message) = text("message") else {
        return format!("HTTP {status}: {}", body.trim());
    };

    let mut sentence = message;
    if let Some(fix) = text("fix") {
        // The registry sometimes already ends the message with the fix ("; run: iii update").
        let needle = fix.trim().trim_end_matches(['.', '!']).to_lowercase();
        if !needle.is_empty() && !sentence.to_lowercase().contains(&needle) {
            sentence.push(' ');
            sentence.push_str(&fix);
        }
    }

    // `fix` usually recites the versions already ("request one of: ..."), so the
    // test is whether they are in the sentence — not whether it used the word
    // "available". Otherwise the operator reads the same list twice.
    if let Some(versions) = error.get("available").and_then(|value| value.as_array()) {
        let listed: Vec<_> = versions.iter().filter_map(|v| v.as_str()).collect();
        if !listed.is_empty() && !listed.iter().any(|version| sentence.contains(version)) {
            sentence.push_str(&format!(" (available: {})", listed.join(", ")));
        }
    }

    sentence
}

/// Downloads, verifies, then extracts — in that order, and never into the final
/// directory until the digest matches.
async fn download_and_extract(
    container: &str,
    artifact: &RegistryArtifact,
    install_dir: &Path,
    archive_sha256: &str,
    lock: fslock::LockFile,
) -> Result<()> {
    let client = reqwest::Client::builder()
        .timeout(DOWNLOAD_TIMEOUT)
        .build()
        .map_err(|err| download_error(container, &artifact.url, &err.to_string()))?;

    let mut response = client
        .get(&artifact.url)
        .send()
        .await
        .and_then(|response| response.error_for_status())
        .map_err(|err| download_error(container, &artifact.url, &err.to_string()))?;
    crate::report::download_started(container, response.content_length());

    let capacity = response
        .content_length()
        .and_then(|length| usize::try_from(length).ok())
        .unwrap_or_default()
        .min(8 * 1024 * 1024);
    let mut bytes = Vec::with_capacity(capacity);
    let mut hasher = Sha256::new();
    let mut downloaded = 0_u64;
    loop {
        let chunk = match response.chunk().await {
            Ok(Some(chunk)) => chunk,
            Ok(None) => break,
            Err(err) => {
                crate::report::download_failed(container);
                return Err(download_error(container, &artifact.url, &err.to_string()));
            }
        };
        downloaded = downloaded.saturating_add(chunk.len() as u64);
        hasher.update(&chunk);
        bytes.extend_from_slice(&chunk);
        crate::report::download_progress(container, downloaded);
    }

    let digest = hex::encode(hasher.finalize());
    if !digest.eq_ignore_ascii_case(&artifact.sha256) {
        crate::report::download_failed(container);
        return Err(ComposeError::PackageDigestMismatch {
            container: container.to_string(),
            url: artifact.url.clone(),
            expected: artifact.sha256.clone(),
            actual: digest,
        });
    }
    crate::report::download_finished(container, downloaded);

    let install_dir = install_dir.to_path_buf();
    let archive_sha256 = archive_sha256.to_string();
    crate::shutdown::blocking(install_dir.clone(), move |cancel| {
        let _lock = lock;
        extract_and_publish(bytes, &install_dir, &archive_sha256, &cancel)
    })
    .await
}

/// Checks between chunks of one large archive entry, not only between files.
/// ErrorKind::Interrupted must not be used: std::io::copy retries it.
struct ArchiveReader<'a, R> {
    inner: R,
    cancel: &'a crate::shutdown::BlockingCancellation,
    path: &'a Path,
}
impl<R: Read> Read for ArchiveReader<'_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.cancel
            .check(self.path)
            .map_err(std::io::Error::other)?;
        self.inner.read(buffer)
    }
}

fn extract_and_publish(
    bytes: Vec<u8>,
    install_dir: &Path,
    archive_sha256: &str,
    cancel: &crate::shutdown::BlockingCancellation,
) -> Result<()> {
    unpack_and_publish(
        |staging| {
            if bytes.starts_with(b"PK\x03\x04") || bytes.starts_with(b"PK\x05\x06") {
                use zip::HasZipMetadata;

                let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&bytes))?;
                for index in 0..archive.len() {
                    cancel.check(staging).map_err(std::io::Error::other)?;
                    let mut entry = archive.by_index(index)?;
                    let relative = entry.enclosed_name().ok_or_else(|| {
                        std::io::Error::other("ZIP entry is outside the install directory")
                    })?;
                    let mode = entry.unix_mode().or_else(|| {
                        // zip 2 discards Darwin's creator OS (19), but its external
                        // attributes contain Unix modes just like Unix-created ZIPs.
                        let creator = entry.central_header_start() as usize + 5;
                        (bytes.get(creator) == Some(&19))
                            .then(|| entry.get_metadata().external_attributes >> 16)
                            .filter(|mode| *mode != 0)
                    });
                    if mode.is_some_and(|mode| mode & 0o170000 == 0o120000) {
                        return Err(std::io::Error::other("ZIP symlinks are not supported"));
                    }
                    let path = staging.join(relative);
                    if entry.is_dir() {
                        std::fs::create_dir_all(&path)?;
                    } else {
                        if let Some(parent) = path.parent() {
                            std::fs::create_dir_all(parent)?;
                        }
                        let mut output = std::fs::File::create(&path)?;
                        std::io::copy(
                            &mut ArchiveReader {
                                inner: &mut entry,
                                cancel,
                                path: staging,
                            },
                            &mut output,
                        )?;
                        #[cfg(unix)]
                        if let Some(mode) = mode {
                            use std::os::unix::fs::PermissionsExt;
                            // Like tar's default, keep only the rwx bits: an archive
                            // must never create setuid, setgid or sticky files.
                            let permissions = std::fs::Permissions::from_mode(mode & 0o777);
                            std::fs::set_permissions(&path, permissions)?;
                        }
                    }
                }
                Ok(())
            } else {
                tar::Archive::new(ArchiveReader {
                    inner: flate2::read::GzDecoder::new(std::io::Cursor::new(bytes)),
                    cancel,
                    path: staging,
                })
                .unpack(staging)
            }
        },
        install_dir,
        archive_sha256,
        cancel,
    )
}

fn unpack_and_publish(
    unpack: impl FnOnce(&Path) -> std::io::Result<()>,
    install_dir: &Path,
    archive_sha256: &str,
    cancel: &crate::shutdown::BlockingCancellation,
) -> Result<()> {
    cancel.check(install_dir)?;
    // Extract beside the destination and rename: a crash mid-extraction must
    // not leave a half-unpacked directory that the next run treats as a cache
    // hit.
    // Each writer needs its own staging directory. Two containers can resolve
    // to the same artefact and download it at the same time; sharing one
    // `.unpacking` path would let either writer remove files under the other.
    let staging = install_dir.with_extension(format!("unpacking-{}", uuid::Uuid::new_v4()));
    if let Err(source) = std::fs::create_dir_all(&staging) {
        return Err(ComposeError::Io {
            path: staging,
            source,
        });
    }

    // A successful publish renames this directory away before drop.
    struct Staging(std::path::PathBuf);
    impl Drop for Staging {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _staging = Staging(staging.clone());
    if let Err(source) = unpack(&staging) {
        let error = ComposeError::Io {
            path: staging.clone(),
            source,
        };
        let _ = std::fs::remove_dir_all(&staging);
        return Err(error);
    }
    if let Err(error) = write_integrity_marker(&staging, archive_sha256, cancel) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(error);
    }

    if let Some(parent) = install_dir.parent()
        && let Err(source) = std::fs::create_dir_all(parent)
    {
        let error = ComposeError::Io {
            path: parent.to_path_buf(),
            source,
        };
        let _ = std::fs::remove_dir_all(&staging);
        return Err(error);
    }
    cancel.check(install_dir)?;
    let result = publish(&staging, install_dir, archive_sha256, cancel);
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    result
}

/// Moves a verified install into place without ever unmaking one.
///
/// Containers start in parallel, so two may need the same artefact at once and
/// race to install it. Removing the destination first would open a window where
/// a directory another container is executing from does not exist. Renaming
/// onto a populated directory fails instead, and that failure is the answer:
/// somebody else got there, their copy passed the same digest check, so theirs
/// is used and this one is dropped.
///
/// A directory left half-written by an interrupted run is the one case worth
/// clearing: it is not another writer's, and nothing can start from it.
fn publish(
    staging: &Path,
    install_dir: &Path,
    archive_sha256: &str,
    cancel: &crate::shutdown::BlockingCancellation,
) -> Result<()> {
    match std::fs::rename(staging, install_dir) {
        Ok(()) => return Ok(()),
        Err(_) if cache_matches(install_dir, archive_sha256, cancel)? => {
            let _ = std::fs::remove_dir_all(staging);
            return Ok(());
        }
        Err(_) => {}
    }

    remove_invalid_install(install_dir)?;
    std::fs::rename(staging, install_dir).map_err(|source| ComposeError::Io {
        path: install_dir.to_path_buf(),
        source,
    })
}

/// Finds the executable inside an install directory.
///
/// Archives are not uniform: some hold the binary at the root, some inside one
/// directory. Rather than guess a layout, take the first executable file.
fn installed_binary(install_dir: &Path) -> Option<PathBuf> {
    fn first_executable(dir: &Path, depth: usize) -> Option<PathBuf> {
        if depth > 3 {
            return None;
        }
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .ok()?
            .filter_map(|e| e.ok())
            .collect();
        // Deterministic: the same archive must yield the same binary on every
        // machine, and read_dir order is not promised.
        entries.sort_by_key(|entry| entry.file_name());

        for entry in &entries {
            // Inherited ACLs can make the integrity marker executable, but it
            // is cache metadata, never the worker program.
            if entry.file_name() == INTEGRITY_FILE {
                continue;
            }
            let path = entry.path();
            if path.is_file() && is_executable(&path) {
                return Some(path);
            }
        }
        for entry in &entries {
            let path = entry.path();
            if path.is_dir()
                && let Some(found) = first_executable(&path, depth + 1)
            {
                return Some(found);
            }
        }
        None
    }

    first_executable(install_dir, 0)
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|meta| meta.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(windows)]
fn is_executable(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
}

fn registry_error(container: &str, registry: &str, message: &str) -> ComposeError {
    ComposeError::RegistryUnreachable {
        container: container.to_string(),
        registry: registry.to_string(),
        message: message.to_string(),
    }
}

fn download_error(container: &str, url: &str, message: &str) -> ComposeError {
    ComposeError::PackageDownloadFailed {
        container: container.to_string(),
        url: url.to_string(),
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use super::*;
    use wiremock::{Mock, MockServer, ResponseTemplate, matchers};

    #[test]
    fn cancellation_during_archive_extraction_removes_staging_without_publishing() {
        struct CancelAfterRead<R> {
            inner: R,
            cancel: crate::shutdown::BlockingCancellation,
        }
        impl<R: Read> Read for CancelAfterRead<R> {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                let count = self.inner.read(buf)?;
                self.cancel.cancel();
                Ok(count)
            }
        }
        let root = tempfile::tempdir().unwrap();
        let install = root.path().join("install");
        let cancel = crate::shutdown::BlockingCancellation::default();
        let decoder = CancelAfterRead {
            inner: flate2::read::GzDecoder::new(std::io::Cursor::new(executable_archive(
                &vec![42; 128 * 1024],
            ))),
            cancel: cancel.clone(),
        };
        assert!(
            unpack_and_publish(
                |staging| {
                    tar::Archive::new(ArchiveReader {
                        inner: decoder,
                        cancel: &cancel,
                        path: staging,
                    })
                    .unpack(staging)
                },
                &install,
                "digest",
                &cancel,
            )
            .is_err()
        );
        assert!(!install.exists());
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }

    #[test]
    fn cancelled_cache_verification_preserves_the_installed_files() {
        let root = tempfile::tempdir().unwrap();
        let install = root.path().join("install");
        let active = crate::shutdown::BlockingCancellation::default();
        extract_and_publish(executable_archive(b"original"), &install, "digest", &active).unwrap();
        let cancelled = crate::shutdown::BlockingCancellation::default();
        cancelled.cancel();
        assert!(cache_matches(&install, "digest", &cancelled).is_err());
        assert!(tree_digest(&install, &cancelled).is_err());
        assert!(cache_matches(&install, "digest", &active).unwrap());
    }

    fn executable_archive(body: &[u8]) -> Vec<u8> {
        let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut archive = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_size(body.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        let name = format!("worker{}", std::env::consts::EXE_SUFFIX);
        archive.append_data(&mut header, name, body).unwrap();
        let encoder = archive.into_inner().unwrap();
        encoder.finish().unwrap()
    }

    fn zip_archive(name: &str, body: &[u8]) -> Vec<u8> {
        use std::io::Write;

        let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        archive
            .start_file(
                name,
                zip::write::SimpleFileOptions::default().unix_permissions(0o755),
            )
            .unwrap();
        archive.write_all(body).unwrap();
        archive.finish().unwrap().into_inner()
    }

    #[tokio::test]
    async fn zip_binary_is_installed_and_reused_from_cache() {
        let name = format!("package/worker{}", std::env::consts::EXE_SUFFIX);
        let server = alias_registry("binary", zip_archive(&name, b"worker contents"), 1).await;
        let cache = tempfile::tempdir().unwrap();
        let installed =
            install_from_registry("shell", &server.uri(), "shell", "1.0.0", cache.path())
                .await
                .unwrap();
        let cached = install_from_registry("shell", &server.uri(), "shell", "1.0.0", cache.path())
            .await
            .unwrap();
        assert_eq!(installed.payload, cached.payload);
        assert_eq!(cached.status, InstallStatus::Cached);
    }

    #[test]
    fn invalid_zip_is_not_published_and_staging_is_removed() {
        let root = tempfile::tempdir().unwrap();
        let install = root.path().join("install");
        let cancel = crate::shutdown::BlockingCancellation::default();
        for bytes in [
            zip_archive("../escaped.exe", b"outside"),
            darwin_zip_archive("worker", b"outside", 0o120777),
            b"PK\x03\x04truncated".to_vec(),
        ] {
            assert!(extract_and_publish(bytes, &install, "digest", &cancel).is_err());
            assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
        }
    }

    fn darwin_zip_archive(name: &str, body: &[u8], mode: u32) -> Vec<u8> {
        let mut bytes = zip_archive(name, body);
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
        let header = archive.by_index(0).unwrap().central_header_start() as usize;
        drop(archive);
        // Central directory: creator OS is byte 5; external attributes start at 38.
        bytes[header + 5] = 19;
        bytes[header + 38..header + 42].copy_from_slice(&(mode << 16).to_le_bytes());
        bytes
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn darwin_zip_binary_is_installed_and_reused_from_cache() {
        let server = alias_registry(
            "binary",
            darwin_zip_archive("package/worker", b"worker contents", 0o100755),
            1,
        )
        .await;
        let cache = tempfile::tempdir().unwrap();
        let installed =
            install_from_registry("shell", &server.uri(), "shell", "1.0.0", cache.path())
                .await
                .unwrap();
        let cached = install_from_registry("shell", &server.uri(), "shell", "1.0.0", cache.path())
            .await
            .unwrap();
        assert_eq!(installed.payload, cached.payload);
        assert_eq!(cached.status, InstallStatus::Cached);
    }

    #[cfg(unix)]
    #[test]
    fn zip_setuid_setgid_and_sticky_bits_are_not_preserved() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let install = root.path().join("install");
        let cancel = crate::shutdown::BlockingCancellation::default();
        let bytes = darwin_zip_archive("worker", b"worker contents", 0o107755);
        extract_and_publish(bytes, &install, "digest", &cancel).unwrap();
        let mode = std::fs::metadata(install.join("worker"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o7777, 0o755);
    }

    #[test]
    fn a_reference_without_a_host_uses_the_default_registry() {
        let (registry, name) = split_reference("state");
        assert_eq!(registry, DEFAULT_REGISTRY);
        assert_eq!(name, "state");
    }

    async fn alias_registry(kind: &str, archive: Vec<u8>, downloads: u64) -> MockServer {
        let server = MockServer::start().await;
        let digest = hex::encode(Sha256::digest(&archive));
        let url = format!("{}/artifact", server.uri());
        let kind = kind.to_string();
        Mock::given(matchers::method("POST"))
            .and(matchers::path("/resolve"))
            .respond_with(move |request: &wiremock::Request| {
                let request: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
                let name = request["worker"].as_str().unwrap();
                let mut worker = serde_json::json!({
                    "name": name, "version": "1.0.0", "type": kind,
                    "binaries": { (host_target()): { "sha256": digest, "url": url } },
                    "archive_url": url, "sha256": digest,
                });
                if name == "console" {
                    worker["alias_of"] = "shell".into();
                }
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"graph": [worker]}))
            })
            .mount(&server)
            .await;
        Mock::given(matchers::method("GET"))
            .and(matchers::path("/artifact"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_bytes(archive)
                    .set_delay(Duration::from_millis(100)),
            )
            .expect(downloads)
            .mount(&server)
            .await;
        server
    }

    #[tokio::test]
    async fn concurrent_alias_and_canonical_installs_download_one_binary() {
        let server = alias_registry("binary", executable_archive(b"#!/bin/sh\nexit 0\n"), 1).await;
        let cache = tempfile::tempdir().unwrap();
        let registry = server.uri();
        let (alias, canonical) = tokio::join!(
            install_from_registry("console", &registry, "console", "1.0.0", cache.path()),
            install_from_registry("shell", &registry, "shell", "1.0.0", cache.path()),
        );
        let alias = alias.unwrap();
        let canonical = canonical.unwrap();
        assert_eq!(alias.payload, canonical.payload);
        assert_ne!(alias.status, canonical.status);
        assert_eq!(alias.alias_of.as_deref(), Some("shell"));
        assert_eq!(canonical.alias_of, None);
        let cached_alias =
            install_from_registry("console", &registry, "console", "1.0.0", cache.path())
                .await
                .unwrap();
        assert_eq!(cached_alias.status, InstallStatus::Cached);
        assert_eq!(cached_alias.alias_of.as_deref(), Some("shell"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn concurrent_alias_and_canonical_installs_share_a_bundle() {
        let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut archive = tar::Builder::new(encoder);
        let body = b"name: shell\n";
        let mut header = tar::Header::new_gnu();
        header.set_size(body.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive
            .append_data(&mut header, BUNDLE_MANIFEST, &body[..])
            .unwrap();
        let server =
            alias_registry("bundle", archive.into_inner().unwrap().finish().unwrap(), 1).await;
        let registry = server.uri();
        let cache = tempfile::tempdir().unwrap();
        let (alias, canonical) = tokio::join!(
            install_from_registry("console", &registry, "console", "1.0.0", cache.path()),
            install_from_registry("shell", &registry, "shell", "1.0.0", cache.path()),
        );
        let alias = alias.unwrap();
        let canonical = canonical.unwrap();
        assert_eq!(alias.payload, canonical.payload);
        assert_ne!(alias.status, canonical.status);
        assert!(matches!(alias.payload, Payload::Bundle(_)));
    }

    #[tokio::test]
    async fn registries_with_the_same_package_name_keep_separate_cache_entries() {
        let archive = executable_archive(b"#!/bin/sh\nexit 0\n");
        let first = alias_registry("binary", archive.clone(), 1).await;
        let second = alias_registry("binary", archive, 1).await;
        let cache = tempfile::tempdir().unwrap();
        let one = install_from_registry("shell", &first.uri(), "shell", "1.0.0", cache.path())
            .await
            .unwrap();
        let two = install_from_registry("shell", &second.uri(), "shell", "1.0.0", cache.path())
            .await
            .unwrap();
        assert_ne!(one.payload, two.payload);
        assert_eq!(two.status, InstallStatus::Downloaded);
    }

    #[test]
    fn aliases_are_preserved_for_root_and_dependency_graph_nodes() {
        let response: ResolveResponse = serde_json::from_value(serde_json::json!({
            "graph": [
                {"name": "console", "alias_of": "shell", "version": "1.0.0", "type": "binary"},
                {"name": "api", "version": "1.0.0", "type": "binary"}
            ]
        }))
        .unwrap();
        let nodes: Vec<Node> = response.graph.into_iter().map(Node::from).collect();
        assert_eq!(nodes[0].name, "console");
        assert_eq!(nodes[0].canonical_name(), "shell");
        assert_eq!(nodes[1].alias_of, None);
    }

    #[test]
    fn an_unsafe_canonical_name_is_refused_before_installation() {
        let response: ResolveResponse = serde_json::from_value(serde_json::json!({
            "graph": [{"name": "console", "alias_of": "../shell", "version": "1.0.0", "type": "binary"}]
        })).unwrap();
        let error = check_names("console", DEFAULT_REGISTRY, &response).unwrap_err();
        assert_eq!(error.code(), "REGISTRY_NAME_REFUSED");
        assert!(error.to_string().contains("alias_of"));
    }

    #[tokio::test]
    async fn resolve_retries_a_transient_server_failure() {
        let server = MockServer::start().await;
        let attempts = Arc::new(AtomicUsize::new(0));
        let responder_attempts = Arc::clone(&attempts);
        Mock::given(matchers::method("POST"))
            .and(matchers::path("/resolve"))
            .respond_with(move |_: &wiremock::Request| {
                if responder_attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                    ResponseTemplate::new(503)
                } else {
                    ResponseTemplate::new(200).set_body_json(serde_json::json!({
                        "graph": [{
                            "name": "state",
                            "version": "1.0.0",
                            "type": "binary",
                            "binaries": {}
                        }]
                    }))
                }
            })
            .expect(2)
            .mount(&server)
            .await;

        let resolved = resolve_response(
            "state",
            &server.uri(),
            "state",
            "1.0.0",
            &["x86_64-unknown-linux-gnu"],
        )
        .await
        .unwrap();

        assert_eq!(resolved.graph[0].name, "state");
    }

    #[tokio::test]
    async fn resolve_does_not_retry_a_client_error() {
        let server = MockServer::start().await;
        Mock::given(matchers::method("POST"))
            .and(matchers::path("/resolve"))
            .respond_with(ResponseTemplate::new(422).set_body_json(serde_json::json!({
                "error": {"message": "version does not exist"}
            })))
            .expect(1)
            .mount(&server)
            .await;

        let error = resolve_response(
            "state",
            &server.uri(),
            "state",
            "99.0.0",
            &["x86_64-unknown-linux-gnu"],
        )
        .await
        .unwrap_err();

        assert_eq!(error.code(), "PACKAGE_NOT_RESOLVED");
    }

    #[tokio::test]
    async fn resolve_sends_host_iii_version() {
        crate::set_host_version("0.26.0");
        let server = MockServer::start().await;
        Mock::given(matchers::method("POST"))
            .and(matchers::path("/resolve"))
            .and(matchers::body_partial_json(
                serde_json::json!({"iii_version": "0.26.0"}),
            ))
            .and(matchers::header("user-agent", "iii/0.26.0"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "graph": [{
                    "name": "state",
                    "version": "1.0.0",
                    "type": "binary",
                    "binaries": {}
                }]
            })))
            .expect(1)
            .mount(&server)
            .await;

        resolve_response(
            "state",
            &server.uri(),
            "state",
            "1.0.0",
            &["x86_64-unknown-linux-gnu"],
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn incompatible_engine_409_is_reported_with_registry_message() {
        let server = MockServer::start().await;
        Mock::given(matchers::method("POST"))
            .and(matchers::path("/resolve"))
            .respond_with(ResponseTemplate::new(409).set_body_json(serde_json::json!({
                "error": {
                    "code": "incompatible_engine",
                    "scope": "worker",
                    "message": "state 1.0.0 requires iii >=0.26.0, running 0.25.0; run: iii update",
                    "required_iii_engine_version": ">=0.26.0"
                }
            })))
            .expect(1)
            .mount(&server)
            .await;

        let error = resolve_response(
            "state",
            &server.uri(),
            "state",
            "1.0.0",
            &["x86_64-unknown-linux-gnu"],
        )
        .await
        .unwrap_err();

        match error {
            ComposeError::PackageNotResolved { message, .. } => {
                assert!(message.contains("requires iii >=0.26.0, running 0.25.0; run: iii update"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_cached_install_is_reused_and_corruption_is_repaired() {
        let server = MockServer::start().await;
        let archive = executable_archive(b"#!/bin/sh\nexit 0\n");
        let digest = hex::encode(Sha256::digest(&archive));
        let target = host_target();
        let artifact_url = format!("{}/artifact", server.uri());

        Mock::given(matchers::method("POST"))
            .and(matchers::path("/resolve"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "graph": [{
                    "name": "state",
                    "version": "1.0.0",
                    "type": "binary",
                    "binaries": {
                        (target): {
                            "sha256": digest,
                            "url": artifact_url,
                        }
                    },
                    "config": {"prefix": "state"}
                }]
            })))
            .expect(3)
            .mount(&server)
            .await;
        Mock::given(matchers::method("GET"))
            .and(matchers::path("/artifact"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(archive))
            .expect(2)
            .mount(&server)
            .await;

        let cache = tempfile::tempdir().unwrap();
        let first = install_from_registry("state", &server.uri(), "state", "1.0.0", cache.path())
            .await
            .unwrap();
        let second = install_from_registry("state", &server.uri(), "state", "1.0.0", cache.path())
            .await
            .unwrap();
        let Payload::Binary(program) = &second.payload else {
            panic!("state should install as a binary");
        };
        std::fs::write(program, b"changed after installation").unwrap();
        let third = install_from_registry("state", &server.uri(), "state", "1.0.0", cache.path())
            .await
            .unwrap();

        assert_eq!(first.status, InstallStatus::Downloaded);
        assert_eq!(second.status, InstallStatus::Cached);
        assert_eq!(third.status, InstallStatus::Downloaded);
        assert_eq!(first.default_config, second.default_config);
        assert_eq!(std::fs::read(program).unwrap(), b"#!/bin/sh\nexit 0\n");
    }

    #[tokio::test]
    async fn same_name_and_version_with_a_different_digest_is_downloaded_again() {
        let server = MockServer::start().await;
        let first_archive = executable_archive(b"#!/bin/sh\nexit 0\n");
        let second_archive = executable_archive(b"#!/bin/sh\nexit 1\n");
        let first_digest = hex::encode(Sha256::digest(&first_archive));
        let second_digest = hex::encode(Sha256::digest(&second_archive));
        let first_url = format!("{}/artifact-first", server.uri());
        let second_url = format!("{}/artifact-second", server.uri());
        let target = host_target();
        let resolutions = Arc::new(AtomicUsize::new(0));
        let responder_resolutions = Arc::clone(&resolutions);

        Mock::given(matchers::method("POST"))
            .and(matchers::path("/resolve"))
            .respond_with(move |_: &wiremock::Request| {
                let (digest, url) = if responder_resolutions.fetch_add(1, Ordering::SeqCst) == 0 {
                    (&first_digest, &first_url)
                } else {
                    (&second_digest, &second_url)
                };
                ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "graph": [{
                        "name": "state",
                        "version": "1.0.0",
                        "type": "binary",
                        "binaries": {
                            (target): {
                                "sha256": digest,
                                "url": url,
                            }
                        }
                    }]
                }))
            })
            .expect(2)
            .mount(&server)
            .await;
        Mock::given(matchers::method("GET"))
            .and(matchers::path("/artifact-first"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(first_archive))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(matchers::method("GET"))
            .and(matchers::path("/artifact-second"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(second_archive))
            .expect(1)
            .mount(&server)
            .await;

        let cache = tempfile::tempdir().unwrap();
        let first = install_from_registry("state", &server.uri(), "state", "1.0.0", cache.path())
            .await
            .unwrap();
        let second = install_from_registry("state", &server.uri(), "state", "1.0.0", cache.path())
            .await
            .unwrap();

        assert_eq!(
            [first.status, second.status],
            [InstallStatus::Downloaded, InstallStatus::Downloaded]
        );
    }

    #[test]
    fn a_reference_with_a_host_uses_it() {
        let (registry, name) = split_reference("workers.iii.dev/state");
        assert_eq!(registry, "https://workers.iii.dev");
        assert_eq!(name, "state");
    }

    /// Scoped names keep their slashes: only the first segment is the host.
    #[test]
    fn only_the_first_segment_is_the_host() {
        let (registry, name) = split_reference("registry.example/team/worker");
        assert_eq!(registry, "https://registry.example");
        assert_eq!(name, "team/worker");
    }

    #[test]
    fn the_host_target_is_a_real_triple() {
        let target = host_target();
        assert_ne!(target, "unknown", "this platform needs a triple mapping");
        assert!(target.contains('-'), "not a triple: {target}");
        assert_eq!(
            host_targets()[0],
            target,
            "the native triple stays preferred"
        );
    }

    #[test]
    fn only_the_musl_build_on_a_glibc_machine_also_runs_glibc_builds() {
        const MUSL: &str = "x86_64-unknown-linux-musl";
        const GNU: &str = "x86_64-unknown-linux-gnu";
        assert_eq!(runnable_targets(MUSL, true), [MUSL, GNU]);
        assert_eq!(runnable_targets(MUSL, false), [MUSL]);
        assert_eq!(runnable_targets(GNU, true), [GNU]);
        assert_eq!(
            runnable_targets("aarch64-apple-darwin", true),
            ["aarch64-apple-darwin"]
        );
    }

    #[test]
    fn an_arm_musl_build_on_a_glibc_machine_also_runs_glibc_builds() {
        const MUSL: &str = "aarch64-unknown-linux-musl";
        const GNU: &str = "aarch64-unknown-linux-gnu";
        assert_eq!(runnable_targets(MUSL, true), [MUSL, GNU]);
        assert_eq!(runnable_targets(MUSL, false), [MUSL]);
        assert_eq!(runnable_targets(GNU, true), [GNU]);
    }

    #[test]
    fn a_static_linux_build_names_its_musl_triple_on_every_architecture() {
        assert_eq!(
            target_triple("x86_64", "linux", true),
            "x86_64-unknown-linux-musl"
        );
        assert_eq!(
            target_triple("x86_64", "linux", false),
            "x86_64-unknown-linux-gnu"
        );
        assert_eq!(
            target_triple("aarch64", "linux", true),
            "aarch64-unknown-linux-musl"
        );
        assert_eq!(
            target_triple("aarch64", "linux", false),
            "aarch64-unknown-linux-gnu"
        );
        assert_eq!(
            target_triple("aarch64", "macos", false),
            "aarch64-apple-darwin"
        );
        assert_eq!(
            target_triple("x86_64", "macos", false),
            "x86_64-apple-darwin"
        );
        assert_eq!(
            target_triple("x86_64", "windows", false),
            "x86_64-pc-windows-msvc"
        );
        assert_eq!(
            target_triple("aarch64", "windows", false),
            "aarch64-pc-windows-msvc"
        );
        assert_eq!(target_triple("riscv64", "linux", false), "unknown");
    }

    #[test]
    fn the_glibc_probe_reads_the_loaders_of_the_host_architecture() {
        assert_eq!(
            linux_loaders("x86_64"),
            Some(("/lib64/ld-linux-x86-64.so.2", "/lib/ld-musl-x86_64.so.1"))
        );
        assert_eq!(
            linux_loaders("aarch64"),
            Some(("/lib/ld-linux-aarch64.so.1", "/lib/ld-musl-aarch64.so.1"))
        );
        assert_eq!(linux_loaders("riscv64"), None);
    }

    #[test]
    fn a_published_native_build_wins_over_the_glibc_fallback() {
        let candidates = ["x86_64-unknown-linux-musl", "x86_64-unknown-linux-gnu"];
        let both = std::collections::BTreeMap::from([
            ("x86_64-unknown-linux-gnu".to_string(), ()),
            ("x86_64-unknown-linux-musl".to_string(), ()),
        ]);
        let glibc_only =
            std::collections::BTreeMap::from([("x86_64-unknown-linux-gnu".to_string(), ())]);
        let darwin_only =
            std::collections::BTreeMap::from([("aarch64-apple-darwin".to_string(), ())]);
        assert_eq!(
            first_available(&candidates, &both),
            Some("x86_64-unknown-linux-musl")
        );
        assert_eq!(
            first_available(&candidates, &glibc_only),
            Some("x86_64-unknown-linux-gnu")
        );
        assert_eq!(first_available(&candidates, &darwin_only), None);
    }

    #[test]
    fn only_an_unsupported_platform_listing_a_later_candidate_falls_back() {
        let candidates = ["x86_64-unknown-linux-musl", "x86_64-unknown-linux-gnu"];
        let refused = |code: &str, available: &[&str]| {
            serde_json::json!({ "error": {
                "code": code,
                "message": "Worker 'w' does not support platform 'x86_64-unknown-linux-musl'.",
                "available": available,
            }})
            .to_string()
        };
        let glibc = ["x86_64-unknown-linux-gnu", "aarch64-apple-darwin"];
        assert_eq!(
            fallback_index(&refused("unsupported_platform", &glibc), &candidates, 0),
            Some(1)
        );
        // The last candidate has nothing after it.
        assert_eq!(
            fallback_index(&refused("unsupported_platform", &glibc), &candidates, 1),
            None
        );
        // Not a platform refusal, or the fallback is not published either.
        assert_eq!(
            fallback_index(&refused("version_not_found", &glibc), &candidates, 0),
            None
        );
        assert_eq!(
            fallback_index(
                &refused("unsupported_platform", &["aarch64-apple-darwin"]),
                &candidates,
                0
            ),
            None
        );
        assert_eq!(fallback_index("not json", &candidates, 0), None);
    }

    #[tokio::test]
    async fn a_worker_without_a_musl_build_resolves_for_glibc_on_a_glibc_machine() {
        let server = MockServer::start().await;
        Mock::given(matchers::method("POST"))
            .and(matchers::path("/resolve"))
            .and(matchers::body_partial_json(
                serde_json::json!({ "target": "x86_64-unknown-linux-musl" }),
            ))
            .respond_with(ResponseTemplate::new(422).set_body_json(serde_json::json!({ "error": {
                "code": "unsupported_platform",
                "message": "Worker 'judge-laya' does not support platform 'x86_64-unknown-linux-musl'.",
                "available": ["x86_64-apple-darwin", "x86_64-unknown-linux-gnu"],
            }})))
            .expect(2)
            .mount(&server)
            .await;
        Mock::given(matchers::method("POST"))
            .and(matchers::path("/resolve"))
            .and(matchers::body_partial_json(
                serde_json::json!({ "target": "x86_64-unknown-linux-gnu" }),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "graph": [{
                    "name": "judge-laya",
                    "version": "0.2.0",
                    "type": "binary",
                    "binaries": { "x86_64-unknown-linux-gnu": {
                        "sha256": "a".repeat(64),
                        "url": "https://example.com/judge-laya.tar.gz",
                    }},
                }]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let candidates = ["x86_64-unknown-linux-musl", "x86_64-unknown-linux-gnu"];
        let resolved =
            resolve_response("judge-laya", &server.uri(), "judge-laya", "*", &candidates)
                .await
                .unwrap();
        assert_eq!(resolved.graph[0].version, "0.2.0");

        // Without the glibc candidate the registry's refusal is the answer.
        let error = resolve_response(
            "judge-laya",
            &server.uri(),
            "judge-laya",
            "*",
            &candidates[..1],
        )
        .await
        .unwrap_err();
        assert_eq!(error.code(), "UNSUPPORTED_PLATFORM");
        assert!(
            error
                .to_string()
                .contains("x86_64-apple-darwin, x86_64-unknown-linux-gnu"),
            "{error}"
        );
    }

    #[test]
    fn the_binary_search_prefers_a_root_file_and_is_deterministic() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("README"), "not executable").unwrap();
        // Executable means the mode bit on unix and the extension on windows,
        // so the fixture has to be executable in the sense of the platform the
        // test runs on — a bare name here found nothing there.
        let binary = tmp
            .path()
            .join(format!("state{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&binary, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        assert_eq!(installed_binary(tmp.path()), Some(binary));
    }

    #[cfg(unix)]
    #[test]
    fn executable_integrity_marker_does_not_hide_worker_binary() {
        use std::os::unix::fs::PermissionsExt;

        for layout in ["", "bin"] {
            let tmp = tempfile::tempdir().unwrap();
            let binary_dir = tmp.path().join(layout);
            std::fs::create_dir_all(&binary_dir).unwrap();
            let binary = binary_dir.join("state");
            std::fs::write(&binary, "#!/bin/sh\nexit 0\n").unwrap();
            std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
            let cancel = crate::shutdown::BlockingCancellation::default();
            let digest = "a".repeat(64);
            write_integrity_marker(tmp.path(), &digest, &cancel).unwrap();

            // Simulate inherited execute permission without requiring ZFS.
            let marker = tmp.path().join(INTEGRITY_FILE);
            std::fs::set_permissions(&marker, std::fs::Permissions::from_mode(0o770)).unwrap();

            assert!(cache_matches(tmp.path(), &digest, &cancel).unwrap());
            assert_eq!(
                installed_binary(tmp.path()),
                Some(binary),
                "layout={layout:?}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn executable_integrity_marker_alone_is_not_a_worker_binary() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir().unwrap();
        let cancel = crate::shutdown::BlockingCancellation::default();
        write_integrity_marker(tmp.path(), &"a".repeat(64), &cancel).unwrap();
        let marker = tmp.path().join(INTEGRITY_FILE);
        std::fs::set_permissions(&marker, std::fs::Permissions::from_mode(0o770)).unwrap();

        assert_eq!(installed_binary(tmp.path()), None);
    }

    #[test]
    fn an_empty_install_directory_has_no_binary() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(installed_binary(tmp.path()), None);
    }

    // The bodies below are what api.workers.iii.dev actually answers.

    #[test]
    fn a_fix_already_in_the_message_is_not_repeated() {
        let body = r#"{"error":{"code":"incompatible_engine","message":"state requires iii >=0.26.0, running 0.25.0; run: iii update","fix":"Run: iii update"}}"#;
        assert_eq!(
            registry_message(409, body)
                .to_lowercase()
                .matches("iii update")
                .count(),
            1
        );
    }

    #[test]
    fn an_unknown_worker_reads_as_a_sentence() {
        let body = r#"{"error":{"available":[],"code":"worker_not_found",
            "fix":"Publish 'ghost' or remove it from the dependency list.",
            "message":"Worker 'ghost' was not found in the registry."}}"#;

        assert_eq!(
            registry_message(404, body),
            "Worker 'ghost' was not found in the registry. \
             Publish 'ghost' or remove it from the dependency list."
        );
    }

    #[test]
    fn an_unsatisfiable_range_keeps_the_versions_that_exist() {
        let body = r#"{"error":{"available":["0.21.4","0.21.3"],"code":"version_not_found",
            "fix":"Publish 'state' with a compatible version or request one of: 0.21.4, 0.21.3.",
            "message":"No version of 'state' satisfies ^99.0.0."}}"#;

        let message = registry_message(422, body);
        assert!(message.contains("satisfies ^99.0.0"), "{message}");
        assert!(message.contains("0.21.4, 0.21.3"), "{message}");
        // `fix` already recited them; the parenthetical would say it twice.
        assert_eq!(message.matches("0.21.4").count(), 1, "{message}");
    }

    #[test]
    fn versions_are_appended_when_the_fix_does_not_recite_them() {
        let body = r#"{"error":{"available":["1.0.0","0.9.0"],
            "message":"No version of 'state' satisfies ^99.0.0."}}"#;

        assert_eq!(
            registry_message(422, body),
            "No version of 'state' satisfies ^99.0.0. (available: 1.0.0, 0.9.0)"
        );
    }

    #[test]
    fn a_body_we_do_not_recognise_survives_verbatim() {
        assert_eq!(
            registry_message(502, "<html>bad gateway</html>"),
            "HTTP 502: <html>bad gateway</html>"
        );
        assert_eq!(
            registry_message(500, r#"{"oops":1}"#),
            r#"HTTP 500: {"oops":1}"#
        );
    }
    #[test]
    fn a_package_name_that_would_leave_the_cache_is_refused() {
        // What compose does with the name: `cache_root.join(format!(...))`.
        // Anything here that holds a separator or `..` writes outside it.
        for escape in [
            "../../etc/cron.d/x",
            "..",
            "/etc/passwd",
            "state/../../..",
            ".ssh",
            "a\\b",
        ] {
            assert!(!is_path_safe(escape), "{escape} must be refused");
        }
    }

    #[test]
    fn ordinary_names_and_versions_are_allowed() {
        for ok in [
            "state",
            "llm-router",
            "context_manager",
            "0.21.4-alpha.4",
            "x86_64-unknown-linux-gnu",
        ] {
            assert!(is_path_safe(ok), "{ok} must be allowed");
        }
    }

    #[test]
    fn a_dependency_is_checked_too_not_only_the_worker_asked_for() {
        // `compose::add` declares the whole graph, and each node is installed
        // under its own name later.
        let response: ResolveResponse = serde_json::from_value(serde_json::json!({
            "graph": [
                {"name": "state", "version": "1.0.0", "type": "binary"},
                {"name": "../escape", "version": "1.0.0", "type": "binary"},
            ]
        }))
        .unwrap();

        let err = check_names("api", "https://workers.iii.dev", &response).unwrap_err();
        assert_eq!(err.code(), "REGISTRY_NAME_REFUSED");
        assert!(err.to_string().contains("../escape"));
    }

    /// A package archive: an executable `worker` (`worker.exe` on Windows, the only name
    /// [`installed_binary`] takes there), `lib/data.txt` holding `body`, and on Unix a symlink
    /// `current -> lib`.
    fn package_archive(body: &str) -> Vec<u8> {
        let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut archive = tar::Builder::new(encoder);
        let worker = format!("worker{}", std::env::consts::EXE_SUFFIX);
        for (name, mode) in [(worker.as_str(), 0o755), ("lib/data.txt", 0o644)] {
            let mut header = tar::Header::new_gnu();
            header.set_size(body.len() as u64);
            header.set_mode(mode);
            header.set_cksum();
            archive
                .append_data(&mut header, name, body.as_bytes())
                .unwrap();
        }
        #[cfg(unix)]
        {
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Symlink);
            header.set_size(0);
            header.set_mode(0o777);
            archive.append_link(&mut header, "current", "lib").unwrap();
        }
        archive.into_inner().unwrap().finish().unwrap()
    }

    /// [`locked_from`] a registry that cannot be reached.
    async fn locked(
        project: &Path,
        cache: &Path,
        packages: &[(&str, &str, &[u8])],
    ) -> PreparedLock {
        locked_from(project, cache, packages, "http://127.0.0.1:9").await
    }

    /// Writes `worker-compose.yaml` and its lock for `packages` (container, version, archive)
    /// beside a `path://` container, puts each archive in `cache` as an install would, and
    /// prepares the lock as `--frozen` does: from the cache, without a registry. Each artifact's
    /// URL is `<server>/<container>-<version>`.
    async fn locked_from(
        project: &Path,
        cache: &Path,
        packages: &[(&str, &str, &[u8])],
        server: &str,
    ) -> PreparedLock {
        let target = host_target();
        let mut compose = String::from("containers:\n  api:\n    worker: path://./api\n");
        let mut containers = serde_json::Map::new();
        for (container, version, archive) in packages {
            let digest = hex::encode(Sha256::digest(archive));
            let resolved = ResolvedPackage {
                name: container.to_string(),
                alias_of: None,
                registry: DEFAULT_REGISTRY.to_string(),
                version: version.to_string(),
                kind: "binary".to_string(),
                artifacts: BTreeMap::from([(
                    target.to_string(),
                    RegistryArtifact {
                        sha256: digest.clone(),
                        url: format!("{server}/{container}-{version}"),
                    },
                )]),
                default_config: None,
            };
            let install = cache_install_dir(cache, &resolved, target, &digest);
            if !install.exists() {
                let cancel = crate::shutdown::BlockingCancellation::default();
                extract_and_publish(archive.to_vec(), &install, &digest, &cancel).unwrap();
            }
            let worker = format!("package://api.workers.iii.dev/{container}");
            compose += &format!("  {container}:\n    worker: {worker}\n    version: '{version}'\n");
            containers.insert(
                container.to_string(),
                serde_json::json!({ "worker": worker, "requested": version, "resolved": resolved }),
            );
        }
        let lock = serde_json::json!({ "version": 1, "containers": containers });
        std::fs::write(project.join("worker-compose.yaml"), compose).unwrap();
        std::fs::write(
            project.join("worker-compose.lock"),
            serde_yaml::to_string(&lock).unwrap(),
        )
        .unwrap();
        let mut file = crate::ComposeFile::load(project.join("worker-compose.yaml")).unwrap();
        crate::lockfile::prepare_frozen(&mut file, cache)
            .await
            .unwrap()
    }

    /// The names in `dir`, sorted.
    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// Every path under `dir` with its size, sorted: what a layout comparison needs.
    fn tree(dir: &Path) -> Vec<(String, u64)> {
        fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, u64)>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                let metadata = std::fs::symlink_metadata(&path).unwrap();
                let name = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                out.push((
                    name,
                    if metadata.is_file() {
                        metadata.len()
                    } else {
                        0
                    },
                ));
                if metadata.is_dir() {
                    walk(root, &path, out);
                }
            }
        }
        let mut out = Vec::new();
        walk(dir, dir, &mut out);
        out.sort();
        out
    }

    fn recorded(project: &Path) -> serde_json::Value {
        serde_json::from_slice(&std::fs::read(project.join(".workers/.packages.json")).unwrap())
            .unwrap()
    }

    fn sha256_of(archive: &[u8]) -> String {
        format!("sha256:{}", hex::encode(Sha256::digest(archive)))
    }

    #[tokio::test]
    async fn materialize_copies_the_locked_packages_and_records_them() {
        let project = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let (state, queue) = (package_archive("state 1"), package_archive("queue 2"));
        let lock = locked(
            project.path(),
            cache.path(),
            &[("state", "1.0.0", &state), ("queue", "2.0.0", &queue)],
        )
        .await;
        let cache_before = tree(cache.path());

        materialize_project(project.path(), cache.path(), &lock)
            .await
            .unwrap();

        let workers = project.path().join(".workers");
        assert_eq!(
            names(&workers),
            [".gitignore", ".lock", ".packages.json", "queue", "state"]
        );
        assert_eq!(
            std::fs::read_to_string(workers.join(".gitignore")).unwrap(),
            "*\n"
        );
        assert_eq!(
            recorded(project.path()),
            serde_json::json!({
                "schema": 1,
                "containers": {
                    "queue": { "name": "queue", "version": "2.0.0", "sha256": sha256_of(&queue) },
                    "state": { "name": "state", "version": "1.0.0", "sha256": sha256_of(&state) }
                }
            })
        );
        // The extracted package, without the cache's integrity marker.
        let state_dir = workers.join("state");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(names(&state_dir), ["current", "lib", "worker"]);
            assert_eq!(
                std::fs::read_link(state_dir.join("current")).unwrap(),
                Path::new("lib")
            );
            let mode = std::fs::metadata(state_dir.join("worker"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o755);
        }
        assert_eq!(
            std::fs::read_to_string(state_dir.join("lib/data.txt")).unwrap(),
            "state 1"
        );
        assert_eq!(
            tree(cache.path()),
            cache_before,
            "the global cache is read, never changed"
        );
    }

    #[tokio::test]
    async fn materialize_keeps_what_a_build_wrote_until_the_package_changes() {
        let project = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let workers = project.path().join(".workers");
        let (state, queue) = (package_archive("state 1"), package_archive("queue 2"));
        let packages = [
            ("state", "1.0.0", &state[..]),
            ("queue", "2.0.0", &queue[..]),
        ];
        let lock = locked(project.path(), cache.path(), &packages).await;
        materialize_project(project.path(), cache.path(), &lock)
            .await
            .unwrap();
        for container in ["state", "queue"] {
            std::fs::create_dir(workers.join(container).join("node_modules")).unwrap();
        }

        materialize_project(project.path(), cache.path(), &lock)
            .await
            .unwrap();
        assert!(workers.join("state/node_modules").is_dir());
        assert!(workers.join("queue/node_modules").is_dir());

        let updated = package_archive("state 2");
        let lock = locked(
            project.path(),
            cache.path(),
            &[("state", "1.1.0", &updated), ("queue", "2.0.0", &queue)],
        )
        .await;
        materialize_project(project.path(), cache.path(), &lock)
            .await
            .unwrap();

        assert!(
            !workers.join("state/node_modules").exists(),
            "replaced whole"
        );
        assert_eq!(
            std::fs::read_to_string(workers.join("state/lib/data.txt")).unwrap(),
            "state 2"
        );
        assert!(workers.join("queue/node_modules").is_dir(), "unchanged");
        assert_eq!(
            recorded(project.path())["containers"]["state"],
            serde_json::json!({ "name": "state", "version": "1.1.0", "sha256": sha256_of(&updated) })
        );
        let names: Vec<String> = names(&workers)
            .into_iter()
            .filter(|name| !name.starts_with(".state.replaced-"))
            .collect();
        assert_eq!(
            names,
            [".gitignore", ".lock", ".packages.json", "queue", "state"],
            "no staging directory is left"
        );
    }

    /// The tree a package update replaced stays aside, whole, for a host process that still runs
    /// from it; the next run prunes it.
    #[tokio::test]
    async fn a_replaced_tree_stays_aside_until_the_next_run() {
        let project = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let workers = project.path().join(".workers");
        let (state, updated) = (package_archive("state 1"), package_archive("state 2"));
        let lock = locked(project.path(), cache.path(), &[("state", "1.0.0", &state)]).await;
        materialize_project(project.path(), cache.path(), &lock)
            .await
            .unwrap();
        std::fs::write(workers.join("state/run.pid"), "42").unwrap();

        let lock = locked(
            project.path(),
            cache.path(),
            &[("state", "1.1.0", &updated)],
        )
        .await;
        materialize_project(project.path(), cache.path(), &lock)
            .await
            .unwrap();

        assert_eq!(
            std::fs::read_to_string(workers.join("state/lib/data.txt")).unwrap(),
            "state 2"
        );
        let aside: Vec<String> = names(&workers)
            .into_iter()
            .filter(|name| name.starts_with(".state.replaced-"))
            .collect();
        let [aside] = &aside[..] else {
            panic!("one replaced tree: {:?}", names(&workers));
        };
        assert_eq!(
            std::fs::read_to_string(workers.join(aside).join("lib/data.txt")).unwrap(),
            "state 1"
        );
        assert_eq!(
            std::fs::read_to_string(workers.join(aside).join("run.pid")).unwrap(),
            "42"
        );

        materialize_project(project.path(), cache.path(), &lock)
            .await
            .unwrap();
        assert_eq!(
            names(&workers),
            [".gitignore", ".lock", ".packages.json", "state"]
        );
    }

    /// A directory about to be replaced has no `.packages.json` entry until its copy is in
    /// place, so a run stopped in between leaves nothing the next one would skip: neither when
    /// it held the previous package, nor when it was gone and only its entry was left.
    #[tokio::test]
    async fn a_directory_being_replaced_has_no_entry_until_its_copy_is_in_place() {
        use wiremock::{Mock, ResponseTemplate, matchers};

        for gone in [false, true] {
            let server = wiremock::MockServer::start().await;
            let project = tempfile::tempdir().unwrap();
            let cache = tempfile::tempdir().unwrap();
            let workers = project.path().join(".workers");
            let (state, updated) = (package_archive("state 1"), package_archive("state 2"));
            let lock = locked_from(
                project.path(),
                cache.path(),
                &[("state", "1.0.0", &state)],
                &server.uri(),
            )
            .await;
            materialize_project(project.path(), cache.path(), &lock)
                .await
                .unwrap();
            let lock = locked_from(
                project.path(),
                cache.path(),
                &[("state", "1.1.0", &updated)],
                &server.uri(),
            )
            .await;
            if gone {
                // Gone, as a manual delete or an older version can leave it: only its entry is left.
                std::fs::remove_dir_all(workers.join("state")).unwrap();
            }
            // Damaged in the cache, so the copy downloads it again: the download sees the project
            // as a run stopped there would leave it.
            std::fs::write(
                cache_entry(cache.path(), &lock, "state").join("lib/data.txt"),
                "damaged",
            )
            .unwrap();
            // What the project holds while the copy downloads: the entries, and the directory's data.
            let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
            let (record, data, seen_in, body) = (
                workers.join(".packages.json"),
                workers.join("state/lib/data.txt"),
                std::sync::Arc::clone(&seen),
                updated.clone(),
            );
            Mock::given(matchers::method("GET"))
                .and(matchers::path("/state-1.1.0"))
                .respond_with(move |_: &wiremock::Request| {
                    let entries: serde_json::Value =
                        serde_json::from_slice(&std::fs::read(&record).unwrap()).unwrap();
                    let held = std::fs::read_to_string(&data).unwrap_or_default();
                    seen_in.lock().unwrap().push((entries, held));
                    ResponseTemplate::new(200).set_body_bytes(body.clone())
                })
                .mount(&server)
                .await;

            materialize_project(project.path(), cache.path(), &lock)
                .await
                .unwrap();

            let seen = seen.lock().unwrap();
            assert_eq!(seen.len(), 1, "the copy downloaded once");
            let held = if gone { "" } else { "state 1" };
            assert_eq!(seen[0].1, held, "the old directory is as it was");
            assert_eq!(
                seen[0].0["containers"].get("state"),
                None,
                "{:?}",
                seen[0].0
            );
            assert_eq!(
                recorded(project.path())["containers"]["state"],
                serde_json::json!({ "name": "state", "version": "1.1.0", "sha256": sha256_of(&updated) })
            );
        }
    }

    #[tokio::test]
    async fn materialize_removes_what_the_lock_no_longer_holds() {
        let project = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let workers = project.path().join(".workers");
        let (state, queue) = (package_archive("state 1"), package_archive("queue 2"));
        let lock = locked(
            project.path(),
            cache.path(),
            &[("state", "1.0.0", &state), ("queue", "2.0.0", &queue)],
        )
        .await;
        materialize_project(project.path(), cache.path(), &lock)
            .await
            .unwrap();
        // What an interrupted copy leaves, and what is not Compose's.
        std::fs::create_dir(workers.join(".state.staging-interrupted")).unwrap();
        std::fs::create_dir(workers.join(".queue.replaced-interrupted")).unwrap();
        std::fs::create_dir(workers.join("mine")).unwrap();
        std::fs::write(workers.join("notes.txt"), "mine").unwrap();
        std::fs::write(workers.join(".packages.json.tmp-interrupted"), "{").unwrap();
        std::fs::write(workers.join(".gitignore.tmp-interrupted"), "").unwrap();
        // A record edited by hand that names Compose's own lock file: not a package directory.
        let mut edited = recorded(project.path());
        edited["containers"][".lock"] = edited["containers"]["queue"].clone();
        std::fs::write(workers.join(".packages.json"), edited.to_string()).unwrap();

        let lock = locked(project.path(), cache.path(), &[("state", "1.0.0", &state)]).await;
        materialize_project(project.path(), cache.path(), &lock)
            .await
            .unwrap();

        assert_eq!(
            names(&workers),
            [
                ".gitignore",
                ".lock",
                ".packages.json",
                "mine",
                "notes.txt",
                "state"
            ]
        );
        assert_eq!(
            recorded(project.path())["containers"],
            serde_json::json!({ "state": { "name": "state", "version": "1.0.0", "sha256": sha256_of(&state) } })
        );

        // The last package removed: `.workers/` stays, empty of packages.
        let lock = locked(project.path(), cache.path(), &[]).await;
        materialize_project(project.path(), cache.path(), &lock)
            .await
            .unwrap();
        assert_eq!(
            names(&workers),
            [".gitignore", ".lock", ".packages.json", "mine", "notes.txt"]
        );
        assert_eq!(
            recorded(project.path()),
            serde_json::json!({ "schema": 1, "containers": {} })
        );
    }

    #[tokio::test]
    async fn materialize_copies_again_a_missing_directory_or_an_unreadable_record() {
        let project = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let workers = project.path().join(".workers");
        let state = package_archive("state 1");
        let lock = locked(project.path(), cache.path(), &[("state", "1.0.0", &state)]).await;
        materialize_project(project.path(), cache.path(), &lock)
            .await
            .unwrap();

        std::fs::remove_dir_all(workers.join("state")).unwrap();
        materialize_project(project.path(), cache.path(), &lock)
            .await
            .unwrap();
        assert!(
            workers
                .join(format!("state/worker{}", std::env::consts::EXE_SUFFIX))
                .is_file()
        );

        std::fs::write(workers.join("state/stale"), "").unwrap();
        std::fs::write(workers.join(".packages.json"), "{\"schema\":").unwrap();
        materialize_project(project.path(), cache.path(), &lock)
            .await
            .unwrap();
        assert!(!workers.join("state/stale").exists());
        assert_eq!(
            recorded(project.path())["containers"]["state"]["version"],
            "1.0.0"
        );
    }

    #[tokio::test]
    async fn a_project_without_packages_gets_no_workers_directory() {
        let project = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let lock = locked(project.path(), cache.path(), &[]).await;
        materialize_project(project.path(), cache.path(), &lock)
            .await
            .unwrap();
        assert!(!project.path().join(".workers").exists());
    }

    /// The cache entry `lock` installs for `container`.
    fn cache_entry(cache: &Path, lock: &PreparedLock, container: &str) -> PathBuf {
        let (_, resolved) = lock.packages().find(|(key, _)| *key == container).unwrap();
        let target = runnable_target(&resolved.artifacts);
        let digest = resolved.artifacts[target].sha256.clone();
        cache_install_dir(
            &registry_cache_root(cache, resolved),
            resolved,
            target,
            &digest,
        )
    }

    /// The source `.packages.json` records for `container`, as a build made from it records it.
    fn recorded_source(project: &Path, container: &str) -> crate::build_record::BuildSource {
        let entry = &recorded(project)["containers"][container];
        crate::build_record::BuildSource::Package {
            name: entry["name"].as_str().unwrap().to_string(),
            version: entry["version"].as_str().unwrap().to_string(),
            sha256: entry["sha256"].as_str().unwrap().to_string(),
        }
    }

    /// A `build.json` made from `source`: a VM build when `vm`, else a host build.
    fn build_record(
        made_from: crate::build_record::BuildSource,
        vm: bool,
    ) -> crate::build_record::BuildRecord {
        use crate::build_record::*;
        BuildRecord {
            schema: SCHEMA,
            container: "any".to_string(),
            source: made_from,
            command: "npm ci".to_string(),
            dependency_files: BTreeMap::new(),
            image: vm.then(|| BuildImage {
                file: "image.erofs".to_string(),
                build_id: "sha256:7d41".to_string(),
                base_image: BuildBaseImage {
                    declared: None,
                    reference: "docker.io/iiidev/node:latest".to_string(),
                    digest: None,
                },
                adapter: BuildAdapter {
                    name: "sandbox-libkrun".to_string(),
                    version: "0.4.0".to_string(),
                },
                contract: BuildContract { major: 1, minor: 3 },
            }),
            built_at: "2026-11-10T14:03:22Z".to_string(),
        }
    }

    #[tokio::test]
    async fn a_failed_check_in_a_new_project_leaves_workers_ignored() {
        let project = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let workers = project.path().join(".workers");
        let state = package_archive("state 1");
        let lock = locked(project.path(), cache.path(), &[("state", "1.0.0", &state)]).await;
        assert!(!workers.exists());

        let error = materialize_project_checked(project.path(), cache.path(), &lock, || {
            Err(ComposeError::BuildPlanStale {
                path: project.path().join("worker-compose.yaml"),
            })
        })
        .await
        .unwrap_err();

        assert_eq!(error.code(), "BUILD_PLAN_STALE");
        if workers.exists() {
            assert_eq!(
                std::fs::read_to_string(workers.join(".gitignore")).unwrap(),
                "*\n",
                "the lock file it leaves stays out of git"
            );
            assert_eq!(
                names(&workers),
                [".gitignore", ".lock"],
                "nothing else made"
            );
        }
    }

    #[tokio::test]
    async fn a_package_copied_again_drops_the_host_build_made_from_it() {
        let project = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let workers = project.path().join(".workers");
        let (state, queue) = (package_archive("state 1"), package_archive("queue 2"));
        let lock = locked(
            project.path(),
            cache.path(),
            &[("state", "1.0.0", &state), ("queue", "2.0.0", &queue)],
        )
        .await;
        materialize_project(project.path(), cache.path(), &lock)
            .await
            .unwrap();
        let build = |container: &str| crate::build_record::build_dir(project.path(), container);
        // `state` runs on the host, `queue` in a VM.
        let host = build_record(recorded_source(project.path(), "state"), false);
        let vm = build_record(recorded_source(project.path(), "queue"), true);
        crate::build_record::write(&build("state"), &host).unwrap();
        crate::build_record::write(&build("queue"), &vm).unwrap();

        materialize_project(project.path(), cache.path(), &lock)
            .await
            .unwrap();
        assert!(
            build("state").join("build.json").is_file(),
            "nothing copied"
        );

        for container in ["state", "queue"] {
            std::fs::remove_dir_all(workers.join(container)).unwrap();
        }
        materialize_project(project.path(), cache.path(), &lock)
            .await
            .unwrap();
        assert_eq!(
            crate::build_record::read(&build("state")).unwrap(),
            None,
            "the host build ran in the directory that is gone"
        );
        assert_eq!(
            crate::build_record::read(&build("queue")).unwrap(),
            Some(vm),
            "the image holds its own copy"
        );
    }

    #[tokio::test]
    async fn a_package_update_keeps_the_host_build_for_the_rebuild_to_report() {
        let project = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let (state, updated) = (package_archive("state 1"), package_archive("state 2"));
        let lock = locked(project.path(), cache.path(), &[("state", "1.0.0", &state)]).await;
        materialize_project(project.path(), cache.path(), &lock)
            .await
            .unwrap();
        let build = crate::build_record::build_dir(project.path(), "state");
        let host = build_record(recorded_source(project.path(), "state"), false);
        crate::build_record::write(&build, &host).unwrap();

        let lock = locked(
            project.path(),
            cache.path(),
            &[("state", "1.1.0", &updated)],
        )
        .await;
        materialize_project(project.path(), cache.path(), &lock)
            .await
            .unwrap();

        assert_eq!(
            crate::build_record::read(&build).unwrap(),
            Some(host.clone())
        );
        let changes = crate::build_record::changes(
            &host,
            &project.path().join("worker-compose.yaml"),
            "state",
            &project.path().join(".workers/state"),
        )
        .unwrap();
        assert_eq!(
            changes,
            [crate::build_record::Change::Source {
                old: host.source.clone(),
                new: recorded_source(project.path(), "state"),
            }],
            "spec 5.3: a package update, which rebuilds"
        );
    }

    /// Every `.packages.json` entry names a directory holding that package: what one run at a
    /// time guarantees, whichever run went last.
    fn assert_consistent(project: &Path, bodies: &[(&str, &str)]) {
        let workers = project.join(".workers");
        let record = recorded(project);
        let containers = record["containers"].as_object().unwrap();
        // A tree a replacement set aside stays until the next run; a staging copy never does.
        let mut directories: Vec<String> = names(&workers)
            .into_iter()
            .filter(|name| workers.join(name).is_dir())
            .filter(|name| !(is_temporary_copy(name) && name.contains(".replaced-")))
            .collect();
        directories.sort();
        assert_eq!(
            directories,
            containers.keys().cloned().collect::<Vec<_>>(),
            "no staging copy is left, and no directory goes unrecorded"
        );
        for (container, entry) in containers {
            let version = entry["version"].as_str().unwrap();
            let body = bodies
                .iter()
                .find(|(key, _)| *key == format!("{container}@{version}"))
                .unwrap()
                .1;
            assert_eq!(
                std::fs::read_to_string(workers.join(container).join("lib/data.txt")).unwrap(),
                body,
                "{container}"
            );
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_materializations_of_one_project_take_turns() {
        let project = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let (state, updated, queue) = (
            package_archive("state 1"),
            package_archive("state 2"),
            package_archive("queue 2"),
        );
        let first = locked(
            project.path(),
            cache.path(),
            &[("state", "1.0.0", &state), ("queue", "2.0.0", &queue)],
        )
        .await;
        let second = locked(
            project.path(),
            cache.path(),
            &[("state", "1.1.0", &updated)],
        )
        .await;
        let bodies = [
            ("state@1.0.0", "state 1"),
            ("state@1.1.0", "state 2"),
            ("queue@2.0.0", "queue 2"),
        ];

        for _ in 0..3 {
            let runs = futures::future::join_all(
                [&first, &second, &first, &second, &first]
                    .map(|lock| materialize_project(project.path(), cache.path(), lock)),
            )
            .await;
            for run in runs {
                run.unwrap();
            }
            assert_consistent(project.path(), &bodies);
        }
    }

    #[tokio::test]
    async fn a_failed_check_leaves_workers_as_it_was() {
        let project = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let workers = project.path().join(".workers");
        let (state, updated) = (package_archive("state 1"), package_archive("state 2"));
        let lock = locked(project.path(), cache.path(), &[("state", "1.0.0", &state)]).await;
        materialize_project(project.path(), cache.path(), &lock)
            .await
            .unwrap();
        let before = tree(&workers);

        let lock = locked(
            project.path(),
            cache.path(),
            &[("state", "1.1.0", &updated)],
        )
        .await;
        let error = materialize_project_checked(project.path(), cache.path(), &lock, || {
            Err(ComposeError::BuildPlanStale {
                path: project.path().join("worker-compose.yaml"),
            })
        })
        .await
        .unwrap_err();

        assert_eq!(error.code(), "BUILD_PLAN_STALE");
        assert_eq!(tree(&workers), before);
    }

    #[tokio::test]
    async fn a_damaged_cache_entry_that_cannot_be_fetched_keeps_the_previous_directory() {
        let project = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let workers = project.path().join(".workers");
        let (queue, updated, state) = (
            package_archive("queue 1"),
            package_archive("queue 2"),
            package_archive("state 1"),
        );
        let lock = locked(
            project.path(),
            cache.path(),
            &[("queue", "1.0.0", &queue), ("state", "1.0.0", &state)],
        )
        .await;
        materialize_project(project.path(), cache.path(), &lock)
            .await
            .unwrap();
        // `state` loses its directory, and `queue` comes before it.
        std::fs::remove_dir_all(workers.join("state")).unwrap();

        let lock = locked(
            project.path(),
            cache.path(),
            &[("queue", "1.1.0", &updated), ("state", "1.0.0", &state)],
        )
        .await;
        std::fs::write(
            cache_entry(cache.path(), &lock, "queue").join("lib/data.txt"),
            "damaged",
        )
        .unwrap();
        let error = materialize_project(project.path(), cache.path(), &lock)
            .await
            .unwrap_err();

        assert_eq!(error.code(), "PACKAGE_DOWNLOAD_FAILED", "{error}");
        assert_eq!(
            std::fs::read_to_string(workers.join("queue/lib/data.txt")).unwrap(),
            "queue 1"
        );
        assert_eq!(
            recorded(project.path())["containers"],
            serde_json::json!({ "queue": { "name": "queue", "version": "1.0.0", "sha256": sha256_of(&queue) } }),
            "the entry of the kept directory stays; none names the missing one"
        );
    }

    #[tokio::test]
    async fn a_damaged_cache_entry_is_fetched_again_and_copied() {
        let project = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let server = MockServer::start().await;
        let state = package_archive("state 1");
        Mock::given(matchers::method("GET"))
            .and(matchers::path("/state-1.0.0"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(state.clone()))
            .expect(1)
            .mount(&server)
            .await;
        let lock = locked_from(
            project.path(),
            cache.path(),
            &[("state", "1.0.0", &state)],
            &server.uri(),
        )
        .await;
        let entry = cache_entry(cache.path(), &lock, "state");
        std::fs::write(entry.join("lib/data.txt"), "damaged").unwrap();

        materialize_project(project.path(), cache.path(), &lock)
            .await
            .unwrap();

        for copy in [project.path().join(".workers/state"), entry] {
            assert_eq!(
                std::fs::read_to_string(copy.join("lib/data.txt")).unwrap(),
                "state 1"
            );
        }
    }

    #[tokio::test]
    async fn a_container_key_that_leaves_workers_is_refused_before_anything_changes() {
        let project = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let state = package_archive("state 1");
        locked(project.path(), cache.path(), &[("state", "1.0.0", &state)]).await;
        // The same package, under a key that names a directory beside the project's. Loading
        // refuses that key (INVALID_CONTAINER_KEY), so it is renamed after the load.
        let lock_path = project.path().join("worker-compose.lock");
        let text = std::fs::read_to_string(&lock_path).unwrap();
        assert_eq!(text.matches("\n  state:\n").count(), 1, "{text}");
        std::fs::write(&lock_path, text.replace("\n  state:\n", "\n  ../x:\n")).unwrap();
        let mut file =
            crate::ComposeFile::load(project.path().join("worker-compose.yaml")).unwrap();
        let state = file.containers.shift_remove("state").unwrap();
        file.containers.insert("../x".to_string(), state);
        let lock = crate::lockfile::prepare_frozen(&mut file, cache.path())
            .await
            .unwrap();
        assert_eq!(
            lock.packages().map(|(key, _)| key).collect::<Vec<_>>(),
            ["../x"]
        );

        let error = materialize_project(project.path(), cache.path(), &lock)
            .await
            .unwrap_err();

        assert_eq!(error.code(), "IO_ERROR");
        assert!(
            error.to_string().contains("cannot name a directory"),
            "{error}"
        );
        assert!(!project.path().join(".workers").exists());
        assert!(!project.path().join("x").exists());
    }
}

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
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{ComposeError, Result};

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
    // `cfg!` rather than a runtime probe: the daemon and its children share a
    // machine, so the triple compose was built for is the triple it needs.
    if cfg!(all(
        target_arch = "x86_64",
        target_os = "linux",
        target_env = "musl"
    )) {
        "x86_64-unknown-linux-musl"
    } else if cfg!(all(target_arch = "x86_64", target_os = "linux")) {
        "x86_64-unknown-linux-gnu"
    } else if cfg!(all(target_arch = "aarch64", target_os = "linux")) {
        "aarch64-unknown-linux-gnu"
    } else if cfg!(all(target_arch = "x86_64", target_os = "macos")) {
        "x86_64-apple-darwin"
    } else if cfg!(all(target_arch = "aarch64", target_os = "macos")) {
        "aarch64-apple-darwin"
    } else if cfg!(all(target_arch = "x86_64", target_os = "windows")) {
        "x86_64-pc-windows-msvc"
    } else if cfg!(all(target_arch = "aarch64", target_os = "windows")) {
        "aarch64-pc-windows-msvc"
    } else {
        "unknown"
    }
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
    let target = host_target();
    let worker = resolve(container, registry, name, version_range, target).await?;
    let resolved = into_resolved_package(container, registry, worker, target)?;
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
    let target = host_target();
    let worker = resolve(container, &registry, &name, version_range, target).await?;
    into_resolved_package(container, &registry, worker, target)
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
    let target = host_target();
    let cache_root = if resolved.registry == DEFAULT_REGISTRY {
        cache_root.to_path_buf()
    } else {
        cache_root.join(hex::encode(Sha256::digest(resolved.registry.as_bytes())))
    };

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
                .get(host_target())
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
                .get(host_target())
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
    resolve(container, &registry, &name, range, host_target())
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

/// Everything a worker needs, resolved in one request.
///
/// The registry answers `/resolve` with the whole graph, not just the worker
/// named: its dependencies come back already pinned to versions that satisfy
/// each other. Asking again per dependency would be slower and could resolve a
/// different set, because each answer is computed on its own.
pub async fn resolve_graph(container: &str, reference: &str, version_range: &str) -> Result<Graph> {
    let (registry, name) = split_reference(reference);
    let target = host_target();
    let response = resolve_response(container, &registry, &name, version_range, target).await?;
    Ok(Graph {
        nodes: response.graph.into_iter().map(Node::from).collect(),
        edges: response
            .edges
            .into_iter()
            .map(|edge| (edge.from, edge.to))
            .collect(),
    })
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
    let install_dir = cache_root.join(format!(
        "{}-{}-{}-{digest}",
        resolved.alias_of.as_deref().unwrap_or(&resolved.name),
        resolved.version,
        target
    ));
    let _lock = lock_artifact(&install_dir).await?;
    if cache_matches(&install_dir, &digest)?
        && let Some(existing) = installed_binary(&install_dir)
    {
        return Ok((existing, InstallStatus::Cached));
    }
    download_and_extract(container, artifact, &install_dir, &digest).await?;
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
/// The install is compose's own, not the machine-wide `~/.iii/workers-bundle/`
/// that `iii add` keeps. That one is keyed by name alone and is replaced on
/// every install, so two compose projects pinning different versions of the
/// same bundle would overwrite each other between one `up` and the next.
#[cfg(unix)]
async fn install_bundle(
    container: &str,
    resolved: &ResolvedPackage,
    target: &str,
    cache_root: &Path,
) -> Result<(PathBuf, InstallStatus)> {
    let artifact = artifact_for_target(container, resolved, target)?;

    let digest = validated_cache_digest(container, resolved, &artifact.sha256)?;
    let install_dir = cache_root.join(format!(
        "{}-{}-bundle-{digest}",
        resolved.alias_of.as_deref().unwrap_or(&resolved.name),
        resolved.version
    ));
    let _lock = lock_artifact(&install_dir).await?;
    // The manifest is the bundle's entry point, so its presence is what makes
    // an install dir a cache hit — not the first executable, which a bundle
    // need not have at all.
    if cache_matches(&install_dir, &digest)? && install_dir.join(BUNDLE_MANIFEST).is_file() {
        return Ok((install_dir, InstallStatus::Cached));
    }
    download_and_extract(container, artifact, &install_dir, &digest).await?;

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
    let path = PathBuf::from(path);
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

/// Verifies both the archive identity and the extracted files in one cache entry.
fn cache_matches(install_dir: &Path, archive_sha256: &str) -> Result<bool> {
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
    Ok(tree_digest(install_dir)? == integrity.tree_sha256)
}

/// Hashes the extracted tree in stable path order, excluding its own marker.
fn tree_digest(root: &Path) -> Result<String> {
    fn visit(root: &Path, directory: &Path, hasher: &mut Sha256) -> Result<()> {
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
                visit(root, &path, hasher)?;
            } else if metadata.file_type().is_file() {
                hasher.update(b"file");
                let mut file = std::fs::File::open(&path).map_err(|source| ComposeError::Io {
                    path: path.clone(),
                    source,
                })?;
                let mut buffer = [0_u8; 64 * 1024];
                loop {
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
    visit(root, root, &mut hasher)?;
    Ok(hex::encode(hasher.finalize()))
}

fn write_integrity_marker(install_dir: &Path, archive_sha256: &str) -> Result<()> {
    let marker = install_dir.join(INTEGRITY_FILE);
    let integrity = CacheIntegrity {
        archive_sha256: archive_sha256.to_ascii_lowercase(),
        tree_sha256: tree_digest(install_dir)?,
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
async fn resolve_response(
    container: &str,
    registry: &str,
    name: &str,
    version_range: &str,
    target: &str,
) -> Result<ResolveResponse> {
    let client = reqwest::Client::builder()
        .timeout(RESOLVE_ATTEMPT_TIMEOUT)
        .build()
        .map_err(|err| registry_error(container, registry, &err.to_string()))?;

    let endpoint = format!("{registry}/resolve");
    let request = serde_json::json!({
        "worker": name,
        "version": version_range,
        "target": target,
    });
    let response = send_resolve_request(&client, &endpoint, &request)
        .await
        .map_err(|err| registry_error(container, registry, &err.to_string()))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(ComposeError::PackageNotResolved {
            container: container.to_string(),
            name: name.to_string(),
            range: version_range.to_string(),
            message: registry_message(status.as_u16(), &body),
        });
    }

    let resolved: ResolveResponse = response
        .json()
        .await
        .map_err(|err| registry_error(container, registry, &err.to_string()))?;

    check_names(container, registry, &resolved)?;
    Ok(resolved)
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
    target: &str,
) -> Result<ResolvedWorker> {
    let resolved = resolve_response(container, registry, name, version_range, target).await?;
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
        sentence.push(' ');
        sentence.push_str(&fix);
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

    let decoder = flate2::read::GzDecoder::new(std::io::Cursor::new(bytes));
    if let Err(source) = tar::Archive::new(decoder).unpack(&staging) {
        let error = ComposeError::Io {
            path: staging.clone(),
            source,
        };
        let _ = std::fs::remove_dir_all(&staging);
        return Err(error);
    }
    if let Err(error) = write_integrity_marker(&staging, archive_sha256) {
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
    let result = publish(&staging, install_dir, archive_sha256);
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
fn publish(staging: &Path, install_dir: &Path, archive_sha256: &str) -> Result<()> {
    match std::fs::rename(staging, install_dir) {
        Ok(()) => return Ok(()),
        Err(_) if cache_matches(install_dir, archive_sha256)? => {
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
            "x86_64-unknown-linux-gnu",
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
            "x86_64-unknown-linux-gnu",
        )
        .await
        .unwrap_err();

        assert_eq!(error.code(), "PACKAGE_NOT_RESOLVED");
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

    #[test]
    fn an_empty_install_directory_has_no_binary() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(installed_binary(tmp.path()), None);
    }

    // The bodies below are what api.workers.iii.dev actually answers.

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
}

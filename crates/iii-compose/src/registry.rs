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
//! 3. **Cache.** Installs are keyed by `(name, version, target)`, which is the
//!    artefact's whole identity, so a second `up` reuses the first one's work
//!    and two projects on one machine share it.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::error::{ComposeError, Result};

/// Registry used when a `package://` reference names no host.
pub const DEFAULT_REGISTRY: &str = "https://api.workers.iii.dev";

/// A bundle archive carries its manifest at the root; the start command lives
/// there rather than in the compose file.
pub const BUNDLE_MANIFEST: &str = "iii.worker.yaml";

/// How long the registry has to answer. Resolution is a single small request;
/// a minute means something is wrong, not slow.
const RESOLVE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// Downloads get their own budget: an artefact is megabytes over a link we do
/// not control.
const DOWNLOAD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

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
    version: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    binaries: std::collections::HashMap<String, Artifact>,
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

#[derive(Debug, Deserialize)]
struct Artifact {
    sha256: String,
    url: String,
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

/// A package resolved and installed locally.
#[derive(Debug, Clone)]
pub struct InstalledPackage {
    pub name: String,
    pub version: String,
    pub payload: Payload,
    /// Configuration the worker ships with, to be merged under anything the
    /// compose file overrides.
    pub default_config: Option<serde_yaml::Value>,
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
fn split_reference(reference: &str) -> (String, String) {
    match reference.split_once('/') {
        Some((host, name)) => (format!("https://{host}"), name.to_string()),
        None => (DEFAULT_REGISTRY.to_string(), reference.to_string()),
    }
}

/// Resolves a package reference and makes sure its binary is on disk.
///
/// `cache_root` holds installs keyed by artefact identity; an already-installed
/// version is reused without touching the network.
pub async fn install(
    container: &str,
    reference: &str,
    version_range: &str,
    cache_root: &Path,
) -> Result<InstalledPackage> {
    let (registry, name) = split_reference(reference);
    let target = host_target();

    let resolved = resolve(container, &registry, &name, version_range, target).await?;

    let payload = match resolved.kind.as_str() {
        "binary" => {
            Payload::Binary(install_binary(container, &resolved, target, cache_root).await?)
        }
        // Refused before the download on a platform that could not start it:
        // the archive is megabytes, and an operator learns nothing from having
        // fetched it.
        #[cfg(not(unix))]
        "bundle" => {
            return Err(ComposeError::BundleNeedsAVm {
                container: container.to_string(),
                name: resolved.name,
            });
        }
        #[cfg(unix)]
        "bundle" => Payload::Bundle(install_bundle(container, &resolved, cache_root).await?),
        // `engine` workers are compiled into the engine itself: there is no
        // artefact to install and nothing for compose to start. `image` workers
        // have one, but running it needs the OCI runtime.
        _ => {
            return Err(ComposeError::UnsupportedPackageKind {
                container: container.to_string(),
                name: resolved.name,
                kind: resolved.kind,
            });
        }
    };

    let default_config = match resolved.config {
        Some(config) if !config.is_null() => serde_yaml::to_value(config).ok(),
        _ => None,
    };

    Ok(InstalledPackage {
        name: resolved.name,
        version: resolved.version,
        payload,
        default_config,
    })
}

/// The version the registry hands back for `*`, so `compose::add` can pin what
/// it just resolved rather than writing a range that drifts under the operator.
pub async fn latest_version(container: &str, reference: &str) -> Result<String> {
    let (registry, name) = split_reference(reference);
    let resolved = resolve(container, &registry, &name, "*", host_target()).await?;
    Ok(resolved.version)
}

/// One worker in a resolved graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub name: String,
    pub version: String,
    /// `binary`, `bundle`, `engine`, `image` — what it is, and therefore
    /// whether compose can run it at all.
    pub kind: String,
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
        nodes: response
            .graph
            .into_iter()
            .map(|worker| Node {
                name: worker.name,
                version: worker.version,
                kind: worker.kind,
            })
            .collect(),
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
    resolved: &ResolvedWorker,
    target: &str,
    cache_root: &Path,
) -> Result<PathBuf> {
    let artifact =
        resolved
            .binaries
            .get(target)
            .ok_or_else(|| ComposeError::UnsupportedPlatform {
                container: container.to_string(),
                name: resolved.name.clone(),
                version: resolved.version.clone(),
                target: target.to_string(),
                available: {
                    let mut targets: Vec<String> = resolved.binaries.keys().cloned().collect();
                    targets.sort();
                    targets.join(", ")
                },
            })?;

    let install_dir = cache_root.join(format!("{}-{}-{}", resolved.name, resolved.version, target));
    if let Some(existing) = installed_binary(&install_dir) {
        return Ok(existing);
    }
    download_and_extract(container, artifact, &install_dir).await?;
    installed_binary(&install_dir).ok_or_else(|| ComposeError::PackageArtifactEmpty {
        container: container.to_string(),
        name: resolved.name.clone(),
        path: install_dir.clone(),
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
    resolved: &ResolvedWorker,
    cache_root: &Path,
) -> Result<PathBuf> {
    // The registry sends both or neither. Without the digest the archive cannot
    // be verified, and an unverifiable artefact is not installed.
    let (url, sha256) = match (&resolved.archive_url, &resolved.sha256) {
        (Some(url), Some(sha256)) => (url, sha256),
        _ => {
            return Err(ComposeError::PackageNotResolved {
                container: container.to_string(),
                name: resolved.name.clone(),
                range: resolved.version.clone(),
                message: "the registry resolved it as a bundle but sent no archive_url + sha256 \
                          pair, so the archive could not be verified"
                    .to_string(),
            });
        }
    };

    let install_dir = cache_root.join(format!("{}-{}-bundle", resolved.name, resolved.version));
    // The manifest is the bundle's entry point, so its presence is what makes
    // an install dir a cache hit — not the first executable, which a bundle
    // need not have at all.
    if install_dir.join(BUNDLE_MANIFEST).is_file() {
        return Ok(install_dir);
    }

    let artifact = Artifact {
        sha256: sha256.clone(),
        url: url.clone(),
    };
    download_and_extract(container, &artifact, &install_dir).await?;

    if !install_dir.join(BUNDLE_MANIFEST).is_file() {
        return Err(ComposeError::PackageArtifactEmpty {
            container: container.to_string(),
            name: resolved.name.clone(),
            path: install_dir.clone(),
        });
    }
    Ok(install_dir)
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
        .timeout(RESOLVE_TIMEOUT)
        .build()
        .map_err(|err| registry_error(container, registry, &err.to_string()))?;

    let response = client
        .post(format!("{registry}/resolve"))
        .json(&serde_json::json!({
            "worker": name,
            "version": version_range,
            "target": target,
        }))
        .send()
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

/// Whether a value from the registry may be used to build a path.
///
/// Compose installs into a directory named after the package, so a name or
/// version holding a separator or `..` would place the download outside the
/// cache. The registry is whatever `package://<host>/…` names, so this is not a
/// check on our own service: it is a check on wherever compose was pointed.
fn is_path_safe(value: &str) -> bool {
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
/// turns every node into a declaration, and each one can later be installed
/// under its own name.
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
    artifact: &Artifact,
    install_dir: &Path,
) -> Result<()> {
    let client = reqwest::Client::builder()
        .timeout(DOWNLOAD_TIMEOUT)
        .build()
        .map_err(|err| download_error(container, &artifact.url, &err.to_string()))?;

    let bytes = client
        .get(&artifact.url)
        .send()
        .await
        .and_then(|response| response.error_for_status())
        .map_err(|err| download_error(container, &artifact.url, &err.to_string()))?
        .bytes()
        .await
        .map_err(|err| download_error(container, &artifact.url, &err.to_string()))?;

    let digest = hex::encode(Sha256::digest(&bytes));
    if !digest.eq_ignore_ascii_case(&artifact.sha256) {
        return Err(ComposeError::PackageDigestMismatch {
            container: container.to_string(),
            url: artifact.url.clone(),
            expected: artifact.sha256.clone(),
            actual: digest,
        });
    }

    // Extract beside the destination and rename: a crash mid-extraction must
    // not leave a half-unpacked directory that the next run treats as a cache
    // hit.
    let staging = install_dir.with_extension("unpacking");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|source| ComposeError::Io {
        path: staging.clone(),
        source,
    })?;

    let decoder = flate2::read::GzDecoder::new(std::io::Cursor::new(bytes));
    tar::Archive::new(decoder)
        .unpack(&staging)
        .map_err(|source| ComposeError::Io {
            path: staging.clone(),
            source,
        })?;

    if let Some(parent) = install_dir.parent() {
        std::fs::create_dir_all(parent).map_err(|source| ComposeError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    publish(&staging, install_dir)
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
fn publish(staging: &Path, install_dir: &Path) -> Result<()> {
    match std::fs::rename(staging, install_dir) {
        Ok(()) => return Ok(()),
        Err(_) if is_populated(install_dir) => {
            let _ = std::fs::remove_dir_all(staging);
            return Ok(());
        }
        Err(_) => {}
    }

    let _ = std::fs::remove_dir_all(install_dir);
    std::fs::rename(staging, install_dir).map_err(|source| ComposeError::Io {
        path: install_dir.to_path_buf(),
        source,
    })
}

fn is_populated(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|mut entries| entries.next().is_some())
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
    use super::*;

    #[test]
    fn a_reference_without_a_host_uses_the_default_registry() {
        let (registry, name) = split_reference("state");
        assert_eq!(registry, DEFAULT_REGISTRY);
        assert_eq!(name, "state");
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

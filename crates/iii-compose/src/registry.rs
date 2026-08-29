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
    path::{Path, PathBuf},
    time::Duration,
};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::error::{ComposeError, Result};

/// Registry used when a `package://` reference names no host.
pub const DEFAULT_REGISTRY: &str = "https://api.workers.iii.dev";

/// Historical bundle packages published before package descriptors keep their
/// public worker manifest at the archive root. Release descriptors never read
/// this file; it is used only by the interactive Compose runtime while an old
/// Registry response remains reachable.
pub const LEGACY_BUNDLE_MANIFEST: &str = "iii.worker.yaml";

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
    #[serde(default)]
    package_descriptor: Option<crate::descriptor::PackageDescriptor>,
    #[serde(default)]
    descriptor_sha256: Option<String>,
    #[serde(default)]
    artifacts: Option<ResolvedArtifacts>,
    // Public Registry compatibility projection. A descriptor-native response
    // may include these fields too; descriptor fields always win.
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default, rename = "type")]
    legacy_kind: Option<String>,
    #[serde(default)]
    binaries: std::collections::HashMap<String, Artifact>,
    #[serde(default)]
    archive_url: Option<String>,
    #[serde(default)]
    sha256: Option<String>,
    #[serde(default)]
    image: Option<String>,
    #[serde(default)]
    config: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum ResolvedArtifacts {
    RustBinary {
        binaries: std::collections::HashMap<String, Artifact>,
    },
    JavascriptBundle {
        archive_url: String,
        sha256: String,
    },
    PythonBundle {
        archive_url: String,
        sha256: String,
    },
    OciImage {
        image_tag: String,
    },
}

impl ResolvedWorker {
    fn name(&self) -> &str {
        self.package_descriptor
            .as_ref()
            .map(|descriptor| descriptor.name.as_str())
            .or(self.name.as_deref())
            .unwrap_or_default()
    }

    fn version(&self) -> &str {
        self.package_descriptor
            .as_ref()
            .map(|descriptor| descriptor.version.as_str())
            .or(self.version.as_deref())
            .unwrap_or_default()
    }

    fn binaries(&self) -> Option<&std::collections::HashMap<String, Artifact>> {
        match self.artifacts.as_ref() {
            Some(ResolvedArtifacts::RustBinary { binaries }) => Some(binaries),
            Some(_) => None,
            None if self.legacy_kind.as_deref() == Some("binary") => Some(&self.binaries),
            None => None,
        }
    }

    fn bundle(&self) -> Option<(&str, &str)> {
        match self.artifacts.as_ref() {
            Some(
                ResolvedArtifacts::JavascriptBundle {
                    archive_url,
                    sha256,
                }
                | ResolvedArtifacts::PythonBundle {
                    archive_url,
                    sha256,
                },
            ) => Some((archive_url, sha256)),
            Some(_) => None,
            None if self.legacy_kind.as_deref() == Some("bundle") => {
                self.archive_url.as_deref().zip(self.sha256.as_deref())
            }
            None => None,
        }
    }

    fn image(&self) -> Option<&str> {
        match self.artifacts.as_ref() {
            Some(ResolvedArtifacts::OciImage { image_tag }) => Some(image_tag),
            Some(_) => None,
            None if self.legacy_kind.as_deref() == Some("image") => self.image.as_deref(),
            None => None,
        }
    }

    fn kind(&self) -> &str {
        match self.artifacts.as_ref() {
            Some(ResolvedArtifacts::RustBinary { .. }) => "binary",
            Some(ResolvedArtifacts::JavascriptBundle { .. })
            | Some(ResolvedArtifacts::PythonBundle { .. }) => "bundle",
            Some(ResolvedArtifacts::OciImage { .. }) => "image",
            None => self.legacy_kind.as_deref().unwrap_or_default(),
        }
    }

    fn is_descriptor_native(&self) -> bool {
        self.package_descriptor.is_some()
            && self.descriptor_sha256.is_some()
            && self.artifacts.is_some()
    }

    fn default_config(&self) -> Option<serde_yaml::Value> {
        if let Some(descriptor) = self.package_descriptor.as_ref() {
            return descriptor
                .registry
                .config
                .as_ref()
                .and_then(|config| serde_yaml::to_value(&config.defaults).ok());
        }
        self.config
            .as_ref()
            .filter(|config| !config.is_null())
            .and_then(|config| serde_yaml::to_value(config).ok())
    }
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
    /// An extracted immutable bundle. Its PackageDescriptor is
    /// publisher-controlled, so it runs in a VM rather than on the host.
    Bundle(PathBuf),
    /// A registry OCI image, always digest-pinned.
    Oci(String),
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
    pub version: String,
    pub payload: Payload,
    /// Configuration the worker ships with, to be merged under anything the
    /// compose file overrides.
    pub default_config: Option<serde_yaml::Value>,
    pub descriptor: Option<crate::descriptor::PackageDescriptor>,
    pub descriptor_sha256: Option<String>,
    pub artifact: InstalledArtifact,
    pub status: InstallStatus,
}

#[derive(Debug, Clone)]
pub enum InstalledArtifact {
    RustBinary {
        artifacts: std::collections::BTreeMap<String, InstalledArtifactFile>,
    },
    Bundle {
        archive_url: String,
        sha256: String,
    },
    Oci {
        image: String,
    },
}

#[derive(Debug, Clone)]
pub struct InstalledArtifactFile {
    pub url: String,
    pub sha256: String,
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
    install_from_registry(container, &registry, &name, version_range, cache_root).await
}

/// Resolves and installs a package from an explicit Registry endpoint.
///
/// The CLI uses [`DEFAULT_REGISTRY`]. Keeping the endpoint explicit here lets
/// embedders and tests supply a descriptor-native Registry without changing
/// process-global state.
pub async fn install_from_registry(
    container: &str,
    registry: &str,
    name: &str,
    version_range: &str,
    cache_root: &Path,
) -> Result<InstalledPackage> {
    let target = host_target();

    let resolved = resolve(container, registry, name, version_range, target).await?;

    let (payload, status) = match resolved.kind() {
        "binary" => {
            let (program, status) =
                install_binary(container, &resolved, target, cache_root).await?;
            (Payload::Binary(program), status)
        }
        // Refused before the download on a platform that could not start it:
        // the archive is megabytes, and an operator learns nothing from having
        // fetched it.
        #[cfg(not(unix))]
        "bundle" => {
            return Err(ComposeError::BundleNeedsAVm {
                container: container.to_string(),
                name: resolved.name().to_string(),
            });
        }
        #[cfg(unix)]
        "bundle" => {
            let (install_dir, status) = install_bundle(container, &resolved, cache_root).await?;
            (Payload::Bundle(install_dir), status)
        }
        "image" => {
            let image = resolved
                .image()
                .ok_or_else(|| ComposeError::PackageNotResolved {
                    container: container.to_string(),
                    name: resolved.name().to_string(),
                    range: resolved.version().to_string(),
                    message: "the registry resolved an OCI package without an image".into(),
                })?;
            if !digest_pinned_image(image) {
                return Err(ComposeError::PackageNotResolved {
                    container: container.to_string(),
                    name: resolved.name().to_string(),
                    range: resolved.version().to_string(),
                    message: "the registry OCI artifact is not digest-pinned".into(),
                });
            }
            (Payload::Oci(image.to_string()), InstallStatus::Cached)
        }
        kind => {
            return Err(ComposeError::UnsupportedPackageKind {
                container: container.to_string(),
                name: resolved.name().to_string(),
                kind: kind.to_string(),
            });
        }
    };

    let default_config = resolved.default_config();
    let package_name = resolved.name().to_string();
    let package_version = resolved.version().to_string();

    let artifact = match resolved.kind() {
        "binary" => InstalledArtifact::RustBinary {
            artifacts: resolved
                .binaries()
                .expect("validated binary response has binaries")
                .iter()
                .map(|(target, artifact)| {
                    (
                        target.clone(),
                        InstalledArtifactFile {
                            url: artifact.url.clone(),
                            sha256: artifact.sha256.clone(),
                        },
                    )
                })
                .collect(),
        },
        "bundle" => {
            let (archive_url, sha256) = resolved
                .bundle()
                .expect("validated bundle response has archive identity");
            InstalledArtifact::Bundle {
                archive_url: archive_url.to_string(),
                sha256: sha256.to_string(),
            }
        }
        "image" => InstalledArtifact::Oci {
            image: resolved
                .image()
                .expect("validated image response has an image")
                .to_string(),
        },
        _ => unreachable!("unsupported package kinds return before artifact projection"),
    };

    Ok(InstalledPackage {
        name: package_name,
        version: package_version,
        payload,
        default_config,
        descriptor: resolved.package_descriptor,
        descriptor_sha256: resolved.descriptor_sha256,
        artifact,
        status,
    })
}

fn digest_pinned_image(image: &str) -> bool {
    let Some((_, digest)) = image.rsplit_once("@sha256:") else {
        return false;
    };
    digest.len() == 64
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

/// The version the registry hands back for `*`, so the lock can pin what was
/// resolved rather than retaining a range that drifts under the operator.
pub async fn latest_version(container: &str, reference: &str) -> Result<String> {
    let (registry, name) = split_reference(reference);
    let resolved = resolve(container, &registry, &name, "*", host_target()).await?;
    Ok(resolved.version().to_string())
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
    resolve_graph_from_registry(container, &registry, &name, version_range).await
}

/// Resolves a package graph from an explicit Registry endpoint.
pub async fn resolve_graph_from_registry(
    container: &str,
    registry: &str,
    name: &str,
    version_range: &str,
) -> Result<Graph> {
    let target = host_target();
    let response = resolve_response(container, &registry, &name, version_range, target).await?;
    Ok(Graph {
        nodes: response
            .graph
            .into_iter()
            .map(|worker| Node {
                name: worker.name().to_string(),
                version: worker.version().to_string(),
                kind: worker.kind().to_string(),
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
) -> Result<(PathBuf, InstallStatus)> {
    let binaries = resolved.binaries().expect("binary artifact variant");
    let artifact = binaries
        .get(target)
        .ok_or_else(|| ComposeError::UnsupportedPlatform {
            container: container.to_string(),
            name: resolved.name().to_string(),
            version: resolved.version().to_string(),
            target: target.to_string(),
            available: {
                let mut targets: Vec<String> = binaries.keys().cloned().collect();
                targets.sort();
                targets.join(", ")
            },
        })?;

    let digest = validated_cache_digest(container, resolved, &artifact.sha256)?;
    let install_dir = cache_root.join(format!(
        "{}-{}-{}-{digest}",
        resolved.name(),
        resolved.version(),
        target
    ));
    if let Some(existing) = installed_binary(&install_dir) {
        return Ok((existing, InstallStatus::Cached));
    }
    remove_invalid_install(&install_dir)?;
    download_and_extract(container, artifact, &install_dir).await?;
    let program =
        installed_binary(&install_dir).ok_or_else(|| ComposeError::PackageArtifactEmpty {
            container: container.to_string(),
            name: resolved.name().to_string(),
            path: install_dir.clone(),
        })?;
    Ok((program, InstallStatus::Downloaded))
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
) -> Result<(PathBuf, InstallStatus)> {
    // The registry sends both or neither. Without the digest the archive cannot
    // be verified, and an unverifiable artefact is not installed.
    let (url, sha256) = resolved.bundle().expect("bundle artifact variant");

    let digest = validated_cache_digest(container, resolved, sha256)?;
    let install_dir = cache_root.join(format!(
        "{}-{}-bundle-{digest}",
        resolved.name(),
        resolved.version()
    ));
    let installed = if resolved.is_descriptor_native() {
        is_populated(&install_dir)
    } else {
        install_dir.join(LEGACY_BUNDLE_MANIFEST).is_file()
    };
    if installed {
        return Ok((install_dir, InstallStatus::Cached));
    }
    remove_invalid_install(&install_dir)?;

    let artifact = Artifact {
        sha256: sha256.to_string(),
        url: url.to_string(),
    };
    download_and_extract(container, &artifact, &install_dir).await?;

    let installed = if resolved.is_descriptor_native() {
        is_populated(&install_dir)
    } else {
        install_dir.join(LEGACY_BUNDLE_MANIFEST).is_file()
    };
    if !installed {
        return Err(ComposeError::PackageArtifactEmpty {
            container: container.to_string(),
            name: resolved.name().to_string(),
            path: install_dir.clone(),
        });
    }
    Ok((install_dir, InstallStatus::Downloaded))
}

fn validate_registry_contract(container: &str, resolved: &ResolvedWorker) -> Result<()> {
    let descriptor_fields = [
        resolved.package_descriptor.is_some(),
        resolved.descriptor_sha256.is_some(),
        resolved.artifacts.is_some(),
    ];
    let descriptor_field_count = descriptor_fields
        .into_iter()
        .filter(|present| *present)
        .count();

    if descriptor_field_count == 0 {
        return validate_legacy_contract(container, resolved);
    }
    if descriptor_field_count != descriptor_fields.len() {
        return Err(invalid_registry_contract(
            container,
            resolved,
            "the registry returned an incomplete package descriptor contract",
        ));
    }

    let descriptor = resolved
        .package_descriptor
        .as_ref()
        .expect("all descriptor fields were checked");
    let descriptor_sha256 = resolved
        .descriptor_sha256
        .as_deref()
        .expect("all descriptor fields were checked");
    if descriptor.sha256() != descriptor_sha256 {
        return Err(invalid_registry_contract(
            container,
            resolved,
            "the registry package descriptor identity or SHA-256 is invalid",
        ));
    }

    // During the coordinated Registry rollout both projections are returned.
    // Refuse a response where the public compatibility fields disagree rather
    // than silently choosing whichever one is more convenient to an attacker.
    if resolved
        .name
        .as_deref()
        .is_some_and(|name| name != descriptor.name)
        || resolved
            .version
            .as_deref()
            .is_some_and(|version| version != descriptor.version)
        || resolved
            .legacy_kind
            .as_deref()
            .is_some_and(|kind| kind != resolved.kind())
    {
        return Err(invalid_registry_contract(
            container,
            resolved,
            "the registry descriptor and compatibility projection disagree",
        ));
    }
    Ok(())
}

fn validate_legacy_contract(container: &str, resolved: &ResolvedWorker) -> Result<()> {
    match resolved.kind() {
        "binary" if resolved.binaries().is_some() => Ok(()),
        "bundle" if resolved.bundle().is_some() => Ok(()),
        "image" if resolved.image().is_some() => Ok(()),
        "engine" => Ok(()),
        "binary" => Err(invalid_registry_contract(
            container,
            resolved,
            "the legacy Registry response contains no binaries",
        )),
        "bundle" => Err(invalid_registry_contract(
            container,
            resolved,
            "the legacy Registry response contains no archive_url + sha256 pair",
        )),
        "image" => Err(invalid_registry_contract(
            container,
            resolved,
            "the legacy Registry response contains no image",
        )),
        _ => Err(invalid_registry_contract(
            container,
            resolved,
            "the Registry response contains no supported package contract",
        )),
    }
}

fn invalid_registry_contract(
    container: &str,
    resolved: &ResolvedWorker,
    message: &str,
) -> ComposeError {
    ComposeError::PackageNotResolved {
        container: container.to_string(),
        name: resolved.name().to_string(),
        range: resolved.version().to_string(),
        message: message.to_string(),
    }
}

/// Returns the normalized digest used as the immutable part of a cache key.
fn validated_cache_digest(
    container: &str,
    resolved: &ResolvedWorker,
    sha256: &str,
) -> Result<String> {
    if sha256.len() == 64 && sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Ok(sha256.to_ascii_lowercase());
    }

    Err(ComposeError::PackageNotResolved {
        container: container.to_string(),
        name: resolved.name().to_string(),
        range: resolved.version().to_string(),
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

    for worker in &resolved.graph {
        validate_registry_contract(container, worker)?;
    }
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
/// The whole graph is checked, not only the requested worker: every node can
/// later be installed independently under its own name.
fn check_names(container: &str, registry: &str, resolved: &ResolveResponse) -> Result<()> {
    let refuse = |field: &str, value: &str| ComposeError::RegistryNameRefused {
        container: container.to_string(),
        registry: registry.to_string(),
        field: field.to_string(),
        value: value.to_string(),
    };
    for worker in &resolved.graph {
        if !is_path_safe(worker.name()) {
            return Err(refuse("name", worker.name()));
        }
        if !is_path_safe(worker.version()) {
            return Err(refuse("version", worker.version()));
        }
    }
    Ok(())
}

/// The named worker out of its own graph.
///
/// Installing takes only the root: what a project runs is what its Compose file
/// declares. The rest of the graph is retained as immutable lock data.
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
        .find(|worker| worker.name() == name)
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
    let result = publish(&staging, install_dir);
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

    fn legacy_bundle_archive() -> Vec<u8> {
        let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut archive = tar::Builder::new(encoder);
        let body = b"name: public-worker\nscripts:\n  start: node index.js\n";
        let mut header = tar::Header::new_gnu();
        header.set_size(body.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive
            .append_data(&mut header, LEGACY_BUNDLE_MANIFEST, &body[..])
            .unwrap();
        let encoder = archive.into_inner().unwrap();
        encoder.finish().unwrap()
    }

    fn resolved_binary(
        name: &str,
        version: &str,
        binaries: serde_json::Value,
    ) -> serde_json::Value {
        let mut descriptor: crate::descriptor::PackageDescriptor = serde_json::from_str(
            include_str!("../tests/fixtures/package-descriptor-jcs.json"),
        )
        .unwrap();
        descriptor.name = name.to_string();
        descriptor.version = version.to_string();
        let descriptor_sha256 = descriptor.sha256();
        serde_json::json!({
            "package_descriptor": descriptor,
            "descriptor_sha256": descriptor_sha256,
            "artifacts": {
                "kind": "rust-binary",
                "binaries": binaries,
            }
        })
    }

    fn legacy_binary(name: &str, version: &str, binaries: serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "name": name,
            "version": version,
            "type": "binary",
            "repo": "iii-hq/workers",
            "config": {"adapter": {"name": "local"}},
            "dependencies": {},
            "binaries": binaries,
        })
    }

    #[test]
    fn a_reference_without_a_host_uses_the_default_registry() {
        let (registry, name) = split_reference("state");
        assert_eq!(registry, DEFAULT_REGISTRY);
        assert_eq!(name, "state");
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
                        "graph": [resolved_binary("state", "1.0.0", serde_json::json!({}))]
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

        assert_eq!(resolved.graph[0].name(), "state");
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
    async fn a_second_install_reuses_the_downloaded_artefact() {
        let server = MockServer::start().await;
        let archive = executable_archive(b"#!/bin/sh\nexit 0\n");
        let digest = hex::encode(Sha256::digest(&archive));
        let target = host_target();
        let artifact_url = format!("{}/artifact", server.uri());

        Mock::given(matchers::method("POST"))
            .and(matchers::path("/resolve"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "graph": [resolved_binary("state", "1.0.0", serde_json::json!({
                        (target): {
                            "sha256": digest,
                            "url": artifact_url,
                        }
                    }))]
            })))
            .expect(2)
            .mount(&server)
            .await;
        Mock::given(matchers::method("GET"))
            .and(matchers::path("/artifact"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(archive))
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

        assert_eq!(first.status, InstallStatus::Downloaded);
        assert_eq!(second.status, InstallStatus::Cached);
        assert_eq!(first.default_config, second.default_config);
    }

    #[tokio::test]
    async fn public_compose_runtime_accepts_a_historical_registry_binary() {
        let server = MockServer::start().await;
        let archive = executable_archive(b"#!/bin/sh\nexit 0\n");
        let digest = hex::encode(Sha256::digest(&archive));
        let target = host_target();
        let artifact_url = format!("{}/artifact", server.uri());

        Mock::given(matchers::method("POST"))
            .and(matchers::path("/resolve"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "graph": [legacy_binary("state", "0.22.2", serde_json::json!({
                    (target): {"sha256": digest, "url": artifact_url}
                }))]
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(matchers::method("GET"))
            .and(matchers::path("/artifact"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(archive))
            .expect(1)
            .mount(&server)
            .await;

        let cache = tempfile::tempdir().unwrap();
        let installed =
            install_from_registry("state", &server.uri(), "state", "0.22.2", cache.path())
                .await
                .unwrap();

        assert_eq!(installed.name, "state");
        assert_eq!(installed.version, "0.22.2");
        assert!(matches!(installed.payload, Payload::Binary(_)));
        assert!(installed.descriptor.is_none());
        assert!(installed.descriptor_sha256.is_none());
        assert!(installed.default_config.is_some());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn public_compose_runtime_keeps_historical_manifest_bundles() {
        let server = MockServer::start().await;
        let archive = legacy_bundle_archive();
        let digest = hex::encode(Sha256::digest(&archive));
        let artifact_url = format!("{}/artifact", server.uri());

        Mock::given(matchers::method("POST"))
            .and(matchers::path("/resolve"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "graph": [{
                    "name": "public-worker",
                    "version": "1.0.0",
                    "type": "bundle",
                    "repo": "example/public-worker",
                    "config": {},
                    "dependencies": {},
                    "archive_url": artifact_url,
                    "sha256": digest,
                }]
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(matchers::method("GET"))
            .and(matchers::path("/artifact"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(archive))
            .expect(1)
            .mount(&server)
            .await;

        let cache = tempfile::tempdir().unwrap();
        let installed = install_from_registry(
            "public-worker",
            &server.uri(),
            "public-worker",
            "1.0.0",
            cache.path(),
        )
        .await
        .unwrap();

        let Payload::Bundle(path) = installed.payload else {
            panic!("historical bundle resolved to the wrong payload kind");
        };
        assert!(path.join(LEGACY_BUNDLE_MANIFEST).is_file());
        assert!(installed.descriptor.is_none());
    }

    #[test]
    fn incomplete_descriptor_fields_never_downgrade_to_the_legacy_projection() {
        let mut value = resolved_binary("state", "1.0.0", serde_json::json!({}));
        let object = value.as_object_mut().unwrap();
        object.remove("descriptor_sha256");
        object.insert("name".into(), serde_json::json!("state"));
        object.insert("version".into(), serde_json::json!("1.0.0"));
        object.insert("type".into(), serde_json::json!("binary"));
        object.insert("binaries".into(), serde_json::json!({}));

        let resolved: ResolvedWorker = serde_json::from_value(value).unwrap();
        let error = validate_registry_contract("state", &resolved).unwrap_err();

        assert_eq!(error.code(), "PACKAGE_NOT_RESOLVED");
        assert!(error.to_string().contains("incomplete package descriptor"));
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
                    "graph": [resolved_binary("state", "1.0.0", serde_json::json!({
                            (target): {
                                "sha256": digest,
                                "url": url,
                            }
                        }))]
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
        // Every graph node may be installed under its own name later.
        let response: ResolveResponse = serde_json::from_value(serde_json::json!({
            "graph": [
                resolved_binary("state", "1.0.0", serde_json::json!({})),
                resolved_binary("../escape", "1.0.0", serde_json::json!({})),
            ]
        }))
        .unwrap();

        let err = check_names("api", "https://workers.iii.dev", &response).unwrap_err();
        assert_eq!(err.code(), "REGISTRY_NAME_REFUSED");
        assert!(err.to_string().contains("../escape"));
    }
}

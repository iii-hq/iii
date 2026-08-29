// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Read and write `iii.lock` for reproducible managed worker installs.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

const LOCKFILE_VERSION: u8 = 1;
const LOCKFILE_NAME: &str = "iii.lock";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerLockfile {
    pub version: u8,
    pub workers: BTreeMap<String, LockedWorker>,
}

#[cfg(test)]
mod descriptor_lock_tests {
    use super::*;
    use iii_compose::descriptor::{
        Artifact, FrontendTool, PackageDescriptor, PackageSource, RegistryMetadata, Runtime,
        Validation, ValidationMode,
    };

    fn descriptor() -> PackageDescriptor {
        PackageDescriptor {
            name: "probe".into(),
            version: "1.2.3-rc.1".into(),
            source: PackageSource {
                path: "probe".into(),
                package_manifest: "Cargo.toml".into(),
            },
            artifact: Artifact::RustBinary {
                binary: "probe".into(),
                targets: vec!["x86_64-unknown-linux-gnu".into()],
                toolchain: FrontendTool {
                    name: "rust".into(),
                    version: "1.97.1".into(),
                },
                frontends: vec![],
            },
            runtime: Runtime {
                exec: Some(vec!["probe".into()]),
                ..Runtime::default()
            },
            registry: RegistryMetadata {
                description: "descriptor lock test".into(),
                license: "Elastic-2.0".into(),
                tags: vec![],
                dependencies: BTreeMap::new(),
                config: None,
                publish: true,
            },
            validation: Validation {
                interface: ValidationMode::Skipped,
            },
        }
    }

    fn lock_with_descriptor(digest: String) -> WorkerLockfile {
        let descriptor = descriptor();
        let worker = LockedWorker {
            version: descriptor.version.clone(),
            package_descriptor: descriptor,
            descriptor_sha256: digest,
            worker_type: LockedWorkerType::Binary,
            dependencies: BTreeMap::new(),
            source: Some(LockedSource::Binary {
                artifacts: BTreeMap::from([(
                    "x86_64-unknown-linux-gnu".into(),
                    LockedBinaryArtifact {
                        url: "https://example.test/probe.tar.gz".into(),
                        sha256: "a".repeat(64),
                    },
                )]),
            }),
        };
        WorkerLockfile {
            workers: BTreeMap::from([("probe".into(), worker)]),
            ..WorkerLockfile::default()
        }
    }

    #[test]
    fn round_trip_preserves_descriptor_and_identical_digest() {
        let digest = descriptor().sha256();
        let yaml = lock_with_descriptor(digest.clone()).to_yaml().unwrap();
        let parsed = WorkerLockfile::from_yaml(&yaml).unwrap();
        let locked = &parsed.workers["probe"];
        assert_eq!(locked.descriptor_sha256, digest);
        assert_eq!(locked.package_descriptor.sha256(), digest);
    }

    #[test]
    fn digest_mismatch_is_rejected() {
        let error = lock_with_descriptor("b".repeat(64)).to_yaml().unwrap_err();
        assert!(error.contains("descriptor digest mismatch"), "{error}");
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockedWorker {
    pub version: String,
    pub package_descriptor: iii_compose::descriptor::PackageDescriptor,
    pub descriptor_sha256: String,
    #[serde(rename = "type")]
    pub worker_type: LockedWorkerType,
    #[serde(default)]
    pub dependencies: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<LockedSource>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LockedWorkerType {
    Binary,
    Image,
    Bundle,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockedBinaryArtifact {
    pub url: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum LockedSource {
    Binary {
        artifacts: BTreeMap<String, LockedBinaryArtifact>,
    },
    Image {
        image: String,
    },
    Bundle {
        archive_url: String,
        sha256: String,
    },
}

impl Default for WorkerLockfile {
    fn default() -> Self {
        Self {
            version: LOCKFILE_VERSION,
            workers: BTreeMap::new(),
        }
    }
}

impl WorkerLockfile {
    pub fn from_yaml(input: &str) -> Result<Self, String> {
        let lockfile: Self = serde_yaml::from_str(input)
            .map_err(|e| format!("failed to parse {LOCKFILE_NAME}: {e}"))?;
        lockfile.validate()?;
        Ok(lockfile)
    }

    pub fn to_yaml(&self) -> Result<String, String> {
        self.validate()?;
        serde_yaml::to_string(self)
            .map(|yaml| yaml.strip_prefix("---\n").unwrap_or(&yaml).to_string())
            .map_err(|e| format!("failed to serialize {LOCKFILE_NAME}: {e}"))
    }

    pub fn read_from(path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
        Self::from_yaml(&content)
    }

    /// Write the lockfile atomically: serialize, write to an adjacent temp
    /// file in the same directory, fsync, then `rename(2)` over the dest.
    /// On POSIX rename is atomic on the same filesystem, so a concurrent
    /// reader sees either the previous content or the new content, never
    /// a partial mixture. On rename failure the temp file is cleaned up;
    /// the destination is untouched.
    pub fn write_to(&self, path: &Path) -> Result<(), String> {
        use std::io::Write;

        let yaml = self.to_yaml()?;
        let parent = path.parent().filter(|p| !p.as_os_str().is_empty());
        let dir = parent.unwrap_or_else(|| Path::new("."));
        let file_name = path
            .file_name()
            .ok_or_else(|| format!("invalid lockfile path: {}", path.display()))?
            .to_string_lossy();

        // PID + nanosecond timestamp + counter keeps the temp name unique
        // across concurrent writers within this process and across forks.
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let nonce = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let tmp_name = format!(".{file_name}.tmp.{}.{nanos}.{nonce}", std::process::id());
        let tmp_path = dir.join(&tmp_name);

        let cleanup = |tmp: &Path| {
            let _ = std::fs::remove_file(tmp);
        };

        let mut file = std::fs::File::create(&tmp_path).map_err(|e| {
            format!(
                "failed to create temp lockfile adjacent to {}: {e}",
                path.display()
            )
        })?;
        if let Err(e) = file.write_all(yaml.as_bytes()) {
            cleanup(&tmp_path);
            return Err(format!("failed to write {}: {e}", path.display()));
        }
        if let Err(e) = file.sync_all() {
            cleanup(&tmp_path);
            return Err(format!("failed to fsync {}: {e}", path.display()));
        }
        drop(file);

        if let Err(e) = std::fs::rename(&tmp_path, path) {
            cleanup(&tmp_path);
            return Err(format!("failed to write {}: {e}", path.display()));
        }
        Ok(())
    }

    fn validate(&self) -> Result<(), String> {
        if self.version != LOCKFILE_VERSION {
            return Err(format!(
                "unsupported {LOCKFILE_NAME} version {} (expected {})",
                self.version, LOCKFILE_VERSION
            ));
        }

        for (name, worker) in &self.workers {
            super::registry::validate_worker_name(name)
                .map_err(|e| format!("{LOCKFILE_NAME} worker {name} has invalid name: {e}"))?;
            if worker.package_descriptor.name != *name
                || worker.package_descriptor.version != worker.version
            {
                return Err(format!(
                    "{LOCKFILE_NAME} worker {name} descriptor identity/version does not match its lock entry"
                ));
            }
            let calculated_descriptor_sha256 = worker.package_descriptor.sha256();
            if worker.descriptor_sha256 != calculated_descriptor_sha256 {
                return Err(format!(
                    "{LOCKFILE_NAME} worker {name} descriptor digest mismatch: expected {}, calculated {calculated_descriptor_sha256}",
                    worker.descriptor_sha256
                ));
            }
            let descriptor_type = match &worker.package_descriptor.artifact {
                iii_compose::descriptor::Artifact::RustBinary { .. } => LockedWorkerType::Binary,
                iii_compose::descriptor::Artifact::JavascriptBundle { .. }
                | iii_compose::descriptor::Artifact::PythonBundle { .. } => {
                    LockedWorkerType::Bundle
                }
                iii_compose::descriptor::Artifact::OciImage { .. } => LockedWorkerType::Image,
            };
            if worker.worker_type != descriptor_type {
                return Err(format!(
                    "{LOCKFILE_NAME} worker {name} source type does not match package_descriptor.artifact"
                ));
            }
            for dependency in worker.dependencies.keys() {
                super::registry::validate_worker_name(dependency).map_err(|e| {
                    format!(
                        "{LOCKFILE_NAME} worker {name} has invalid dependency {dependency}: {e}"
                    )
                })?;
            }

            match (&worker.worker_type, &worker.source) {
                (_, None) => {
                    return Err(format!(
                        "{LOCKFILE_NAME} worker {name} is missing required source field"
                    ));
                }
                (LockedWorkerType::Binary, Some(LockedSource::Binary { artifacts })) => {
                    if artifacts.is_empty() {
                        return Err(format!(
                            "{LOCKFILE_NAME} worker {name} has no binary artifacts"
                        ));
                    }
                    for (target, artifact) in artifacts {
                        if target.trim().is_empty() {
                            return Err(format!(
                                "{LOCKFILE_NAME} worker {name} has an empty binary target"
                            ));
                        }
                        if artifact.url.trim().is_empty() {
                            return Err(format!(
                                "{LOCKFILE_NAME} worker {name} artifact {target} has an empty url"
                            ));
                        }
                        if !is_sha256_hex(&artifact.sha256) {
                            return Err(format!(
                                "{LOCKFILE_NAME} worker {name} artifact {target} has invalid binary sha256"
                            ));
                        }
                    }
                }
                (LockedWorkerType::Image, Some(LockedSource::Image { image })) => {
                    if !is_digest_pinned_image(image) {
                        return Err(format!(
                            "{LOCKFILE_NAME} worker {name} image must be pinned by digest"
                        ));
                    }
                }
                (
                    LockedWorkerType::Bundle,
                    Some(LockedSource::Bundle {
                        archive_url,
                        sha256,
                    }),
                ) => {
                    if archive_url.trim().is_empty() {
                        return Err(format!(
                            "{LOCKFILE_NAME} worker {name} bundle has an empty archive_url"
                        ));
                    }
                    if !is_sha256_hex(sha256) {
                        return Err(format!(
                            "{LOCKFILE_NAME} worker {name} bundle has invalid sha256"
                        ));
                    }
                }
                _ => {
                    return Err(format!(
                        "{LOCKFILE_NAME} worker {name} has mismatched type and source kind"
                    ));
                }
            }
        }

        Ok(())
    }

    pub fn verify_config_workers(&self, worker_names: &[String]) -> Result<(), String> {
        let missing: Vec<&String> = worker_names
            .iter()
            .filter(|name| !self.workers.contains_key(*name))
            .collect();

        if missing.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "{LOCKFILE_NAME} is missing worker(s): {}",
                missing
                    .iter()
                    .map(|name| name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        }
    }

    pub fn verify_config_workers_for_target(
        &self,
        worker_names: &[String],
        current_target: &str,
    ) -> Result<(), String> {
        self.verify_config_workers(worker_names)?;

        let missing_artifacts: Vec<String> = worker_names
            .iter()
            .filter_map(|name| {
                let worker = self.workers.get(name)?;
                match &worker.source {
                    Some(LockedSource::Binary { artifacts }) => {
                        if artifacts.contains_key(current_target) {
                            return None;
                        }
                        let available = artifacts.keys().cloned().collect::<Vec<_>>().join(", ");
                        Some(format!("{name} (available: {available})"))
                    }
                    _ => None,
                }
            })
            .collect();

        if missing_artifacts.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "{LOCKFILE_NAME} is missing binary artifact(s) for target {current_target}: {}",
                missing_artifacts.join(", ")
            ))
        }
    }
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn is_digest_pinned_image(value: &str) -> bool {
    value
        .rsplit_once("@sha256:")
        .is_some_and(|(repository, digest)| !repository.is_empty() && is_sha256_hex(digest))
}

pub fn lockfile_path() -> &'static Path {
    Path::new(LOCKFILE_NAME)
}

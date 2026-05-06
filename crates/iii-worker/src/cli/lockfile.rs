// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Read and write `iii.lock` for reproducible managed worker installs.

use serde::{Deserialize, Deserializer, Serialize, de};
use std::collections::BTreeMap;
use std::path::Path;

const LOCKFILE_VERSION: u8 = 1;
const LOCKFILE_NAME: &str = "iii.lock";

/// Prefix for the manifest_hash header value: "sha256:v1:<64-hex>".
/// The "v1" segment is the hash ALGORITHM version, independent of the
/// lockfile FORMAT version. Changing the hash scheme in the future bumps
/// this to "v2", and readers detect and recompute rather than silently
/// accept a mismatched hash. Keeping it adjacent to the hex makes the
/// shape greppable and the intent obvious in diffs.
pub const MANIFEST_HASH_PREFIX: &str = "sha256:v1:";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerLockfile {
    pub version: u8,
    /// SHA-256 of the canonical manifest_dependencies serialization, prefixed
    /// with [`MANIFEST_HASH_PREFIX`]. Optional for backward compat with locks
    /// written before Lane A; absence means "drift detection unavailable for
    /// this lock, treat all syncs as potentially drifted and re-resolve."
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest_hash: Option<String>,
    /// The project's declared `iii.worker.yaml` dependencies at lock-write
    /// time. `Some(map)` (even empty) means the lock was written by Lane A or
    /// later and drift reports can name the exact added/removed/changed deps.
    /// `None` means legacy lock — drift detection falls back to hash-only
    /// compare without structured attribution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declared_dependencies: Option<BTreeMap<String, String>>,
    pub workers: BTreeMap<String, LockedWorker>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockedWorker {
    pub version: String,
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
    Engine,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockedBinaryArtifact {
    pub url: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum LockedSource {
    Binary {
        artifacts: BTreeMap<String, LockedBinaryArtifact>,
    },
    Image {
        image: String,
    },
}

impl<'de> Deserialize<'de> for LockedSource {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(tag = "kind", rename_all = "lowercase")]
        enum RawLockedSource {
            Binary {
                #[serde(default)]
                artifacts: BTreeMap<String, LockedBinaryArtifact>,
                #[serde(default)]
                target: Option<String>,
                #[serde(default)]
                url: Option<String>,
                #[serde(default)]
                sha256: Option<String>,
            },
            Image {
                image: String,
            },
        }

        match RawLockedSource::deserialize(deserializer)? {
            RawLockedSource::Binary {
                mut artifacts,
                target,
                url,
                sha256,
            } => {
                if artifacts.is_empty() {
                    match (target, url, sha256) {
                        (Some(target), Some(url), Some(sha256)) => {
                            artifacts.insert(target, LockedBinaryArtifact { url, sha256 });
                        }
                        _ => {
                            return Err(de::Error::custom(
                                "binary source must include artifacts or legacy target/url/sha256 fields",
                            ));
                        }
                    }
                }

                Ok(LockedSource::Binary { artifacts })
            }
            RawLockedSource::Image { image } => Ok(LockedSource::Image { image }),
        }
    }
}

impl Default for WorkerLockfile {
    fn default() -> Self {
        Self {
            version: LOCKFILE_VERSION,
            manifest_hash: None,
            declared_dependencies: None,
            workers: BTreeMap::new(),
        }
    }
}

/// Returns `true` if `s` matches the `"sha256:v1:<64-lowercase-hex>"`
/// manifest-hash shape. Used by lockfile validation and drift detection.
///
/// Comparison against `compute_manifest_hash` output is byte-exact, and
/// `hex::encode` always emits lowercase — so an uppercase-hex hash
/// would silently never match and trigger false-positive drift on every
/// sync. We reject uppercase here so the validator catches that
/// hand-edit immediately.
pub fn is_valid_manifest_hash(s: &str) -> bool {
    let Some(rest) = s.strip_prefix(MANIFEST_HASH_PREFIX) else {
        return false;
    };
    rest.len() == 64
        && rest
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
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

        if let Some(hash) = &self.manifest_hash
            && !is_valid_manifest_hash(hash)
        {
            return Err(format!(
                "{LOCKFILE_NAME} manifest_hash must match \
                 `{MANIFEST_HASH_PREFIX}<64-lowercase-hex>`; got `{hash}`"
            ));
        }

        // `manifest_hash` and `declared_dependencies` must be paired:
        // `(None, None)` is a legacy lock (drift detection skipped),
        // `(Some, None)` is hash-only attribution, and `(Some, Some)` is
        // the full Lane-A shape. `(None, Some)` is the dangerous combo
        // — it means the deps are recorded but drift detection is
        // disabled, so stripping the hash from a Lane-A lock would
        // silently bypass `--frozen`.
        if self.declared_dependencies.is_some() && self.manifest_hash.is_none() {
            return Err(format!(
                "{LOCKFILE_NAME}: declared_dependencies is set without manifest_hash; \
                 the lock is internally inconsistent (stripping `manifest_hash` would \
                 silently bypass drift detection)"
            ));
        }

        if let Some(declared) = &self.declared_dependencies {
            for (name, range) in declared {
                super::registry::validate_worker_name(name).map_err(|e| {
                    format!("{LOCKFILE_NAME} declared dependency `{name}` is invalid: {e}")
                })?;
                let trimmed = range.trim();
                if trimmed.is_empty() {
                    return Err(format!(
                        "{LOCKFILE_NAME} declared dependency `{name}` has empty range"
                    ));
                }
                semver::VersionReq::parse(trimmed).map_err(|e| {
                    format!(
                        "{LOCKFILE_NAME} declared dependency `{name}` has invalid \
                         semver range `{range}`: {e}"
                    )
                })?;
            }

            // Cross-check: when both fields are present they must be
            // mutually consistent. Compute the canonical hash from
            // `declared_dependencies` and reject any mismatch — silent
            // mismatches let drift detection produce empty,
            // unactionable error reports.
            if let Some(stored_hash) = &self.manifest_hash {
                let computed = super::sync::compute_manifest_hash(declared);
                if &computed != stored_hash {
                    return Err(format!(
                        "{LOCKFILE_NAME}: manifest_hash does not match \
                         declared_dependencies; the lock is internally inconsistent"
                    ));
                }
            }
        }

        for (name, worker) in &self.workers {
            super::registry::validate_worker_name(name)
                .map_err(|e| format!("{LOCKFILE_NAME} worker {name} has invalid name: {e}"))?;
            for dependency in worker.dependencies.keys() {
                super::registry::validate_worker_name(dependency).map_err(|e| {
                    format!(
                        "{LOCKFILE_NAME} worker {name} has invalid dependency {dependency}: {e}"
                    )
                })?;
            }

            match (&worker.worker_type, &worker.source) {
                (LockedWorkerType::Engine, None) => {}
                (LockedWorkerType::Engine, Some(_)) => {
                    return Err(format!(
                        "{LOCKFILE_NAME} worker {name} is type engine but has a source field"
                    ));
                }
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
                    if !image.contains("@sha256:") {
                        return Err(format!(
                            "{LOCKFILE_NAME} worker {name} image must be pinned by digest"
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
    value.len() == 64 && value.chars().all(|c| c.is_ascii_hexdigit())
}

pub fn lockfile_path() -> &'static Path {
    Path::new(LOCKFILE_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn binary_source(target: &str, url: &str, sha256: String) -> LockedSource {
        LockedSource::Binary {
            artifacts: BTreeMap::from([(
                target.to_string(),
                LockedBinaryArtifact {
                    url: url.to_string(),
                    sha256,
                },
            )]),
        }
    }

    #[test]
    fn lockfile_round_trips_with_sorted_workers() {
        let mut lock = WorkerLockfile::default();
        lock.workers.insert(
            "z-worker".to_string(),
            LockedWorker {
                version: "1.0.0".to_string(),
                worker_type: LockedWorkerType::Binary,
                dependencies: BTreeMap::new(),
                source: Some(binary_source(
                    "aarch64-apple-darwin",
                    "https://example.com/z.tar.gz",
                    "f".repeat(64),
                )),
            },
        );
        lock.workers.insert(
            "a-worker".to_string(),
            LockedWorker {
                version: "1.0.0".to_string(),
                worker_type: LockedWorkerType::Image,
                dependencies: BTreeMap::from([("z-worker".to_string(), "^1.0.0".to_string())]),
                source: Some(LockedSource::Image {
                    image: "ghcr.io/iii-hq/a-worker@sha256:abc".to_string(),
                }),
            },
        );

        let yaml = lock.to_yaml().unwrap();

        assert!(yaml.find("a-worker:").unwrap() < yaml.find("z-worker:").unwrap());
        let parsed = WorkerLockfile::from_yaml(&yaml).unwrap();
        assert_eq!(parsed.workers["a-worker"].version, "1.0.0");
        assert_eq!(parsed.workers["z-worker"].version, "1.0.0");
    }

    #[test]
    fn stale_lock_detects_config_worker_missing_from_lock() {
        let lock = WorkerLockfile::default();

        let result = lock.verify_config_workers(&["hello-worker".to_string()]);

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("hello-worker"));
    }

    #[test]
    fn verify_config_workers_passes_when_every_config_worker_is_locked() {
        let mut lock = WorkerLockfile::default();
        lock.workers.insert(
            "hello-worker".to_string(),
            LockedWorker {
                version: "1.0.0".to_string(),
                worker_type: LockedWorkerType::Binary,
                dependencies: BTreeMap::new(),
                source: Some(binary_source(
                    "aarch64-apple-darwin",
                    "https://example.com/h.tar.gz",
                    "a".repeat(64),
                )),
            },
        );

        assert!(
            lock.verify_config_workers(&["hello-worker".to_string()])
                .is_ok()
        );
    }

    #[test]
    fn verify_config_workers_for_target_rejects_missing_binary_artifact() {
        let mut lock = WorkerLockfile::default();
        lock.workers.insert(
            "hello-worker".to_string(),
            LockedWorker {
                version: "1.0.0".to_string(),
                worker_type: LockedWorkerType::Binary,
                dependencies: BTreeMap::new(),
                source: Some(binary_source(
                    "aarch64-apple-darwin",
                    "https://example.com/h.tar.gz",
                    "a".repeat(64),
                )),
            },
        );

        let err = lock
            .verify_config_workers_for_target(
                &["hello-worker".to_string()],
                "x86_64-unknown-linux-gnu",
            )
            .unwrap_err();

        assert!(err.contains("hello-worker"));
        assert!(err.contains("aarch64-apple-darwin"));
        assert!(err.contains("x86_64-unknown-linux-gnu"));
    }

    #[test]
    fn verify_config_workers_for_target_accepts_matching_binary_target() {
        let mut lock = WorkerLockfile::default();
        lock.workers.insert(
            "hello-worker".to_string(),
            LockedWorker {
                version: "1.0.0".to_string(),
                worker_type: LockedWorkerType::Binary,
                dependencies: BTreeMap::new(),
                source: Some(LockedSource::Binary {
                    artifacts: BTreeMap::from([
                        (
                            "aarch64-apple-darwin".to_string(),
                            LockedBinaryArtifact {
                                url: "https://example.com/h-darwin.tar.gz".to_string(),
                                sha256: "a".repeat(64),
                            },
                        ),
                        (
                            "x86_64-unknown-linux-gnu".to_string(),
                            LockedBinaryArtifact {
                                url: "https://example.com/h-linux.tar.gz".to_string(),
                                sha256: "b".repeat(64),
                            },
                        ),
                    ]),
                }),
            },
        );

        assert!(
            lock.verify_config_workers_for_target(
                &["hello-worker".to_string()],
                "x86_64-unknown-linux-gnu"
            )
            .is_ok()
        );
    }

    #[test]
    fn verify_config_workers_for_target_ignores_image_workers() {
        let mut lock = WorkerLockfile::default();
        lock.workers.insert(
            "image-worker".to_string(),
            LockedWorker {
                version: "1.0.0".to_string(),
                worker_type: LockedWorkerType::Image,
                dependencies: BTreeMap::new(),
                source: Some(LockedSource::Image {
                    image: "ghcr.io/iii-hq/image@sha256:abc".to_string(),
                }),
            },
        );

        assert!(
            lock.verify_config_workers_for_target(
                &["image-worker".to_string()],
                "x86_64-unknown-linux-gnu"
            )
            .is_ok()
        );
    }

    #[test]
    fn verify_config_workers_is_intentionally_asymmetric() {
        // Lock has extras that config does not mention. The current design
        // only flags workers present in config.yaml but missing from the
        // lockfile, not the inverse. Encoding that as a test so future
        // changes to symmetry require an intentional update.
        let mut lock = WorkerLockfile::default();
        lock.workers.insert(
            "extra-worker".to_string(),
            LockedWorker {
                version: "1.0.0".to_string(),
                worker_type: LockedWorkerType::Image,
                dependencies: BTreeMap::new(),
                source: Some(LockedSource::Image {
                    image: "ghcr.io/iii-hq/extra@sha256:abc".to_string(),
                }),
            },
        );

        assert!(lock.verify_config_workers(&[]).is_ok());
    }

    #[test]
    fn verify_config_workers_lists_every_missing_name() {
        let lock = WorkerLockfile::default();

        let err = lock
            .verify_config_workers(&["a".to_string(), "b".to_string()])
            .unwrap_err();

        assert!(err.contains("a"));
        assert!(err.contains("b"));
    }

    #[test]
    fn from_yaml_rejects_garbage_input() {
        let err = WorkerLockfile::from_yaml("this is not yaml: : :").unwrap_err();
        assert!(err.contains("iii.lock"));
    }

    #[test]
    fn from_yaml_rejects_unsupported_lockfile_version() {
        let err = WorkerLockfile::from_yaml("version: 2\nworkers: {}\n").unwrap_err();

        assert!(err.contains("unsupported iii.lock version 2"));
    }

    #[test]
    fn from_yaml_rejects_invalid_binary_sha256() {
        let err = WorkerLockfile::from_yaml(
            r#"
version: 1
workers:
  hello:
    version: 1.0.0
    type: binary
    dependencies: {}
    source:
      kind: binary
      artifacts:
        aarch64-apple-darwin:
          url: https://example.com/h.tar.gz
          sha256: nope
"#,
        )
        .unwrap_err();

        assert!(err.contains("hello"));
        assert!(err.contains("invalid binary sha256"));
    }

    #[test]
    fn from_yaml_rejects_unpinned_image_source() {
        let err = WorkerLockfile::from_yaml(
            r#"
version: 1
workers:
  image-worker:
    version: 1.0.0
    type: image
    dependencies: {}
    source:
      kind: image
      image: ghcr.io/iii-hq/image-worker:latest
"#,
        )
        .unwrap_err();

        assert!(err.contains("image-worker"));
        assert!(err.contains("pinned by digest"));
    }

    #[test]
    fn from_yaml_rejects_mismatched_worker_type_and_source_kind() {
        let err = WorkerLockfile::from_yaml(
            r#"
version: 1
workers:
  image-worker:
    version: 1.0.0
    type: image
    dependencies: {}
    source:
      kind: binary
      artifacts:
        aarch64-apple-darwin:
          url: https://example.com/h.tar.gz
          sha256: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
"#,
        )
        .unwrap_err();

        assert!(err.contains("image-worker"));
        assert!(err.contains("mismatched type"));
    }

    #[test]
    fn write_to_reports_unwritable_paths() {
        // A path whose parent does not exist is always unwritable.
        let dir = tempfile::tempdir().unwrap();
        let bogus = dir.path().join("does").join("not").join("exist.lock");

        let lock = WorkerLockfile::default();
        let err = lock.write_to(&bogus).unwrap_err();

        assert!(err.contains(bogus.to_string_lossy().as_ref()));
    }

    #[test]
    fn write_to_does_not_leak_temp_files_on_success() {
        // Atomicity requires writing through a temp file adjacent to the
        // destination. That temp file must be cleaned up on success; a stale
        // `.iii.lock.XXXXX.tmp` accumulating next to the lock every run
        // would be a resource leak.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("iii.lock");
        let lock = WorkerLockfile::default();

        lock.write_to(&path).unwrap();

        assert!(path.exists());
        let stragglers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|name| name != "iii.lock")
            .collect();
        assert!(
            stragglers.is_empty(),
            "expected only iii.lock after successful write; found stragglers: {stragglers:?}"
        );
    }

    #[test]
    fn write_to_preserves_existing_file_when_serialization_fails() {
        // If the lock fails validation during to_yaml(), the on-disk file
        // must not be truncated. This is the atomic-rename contract: we
        // only touch the destination after the new content is fully ready.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("iii.lock");

        // Seed a valid lockfile.
        let mut good = WorkerLockfile::default();
        good.workers.insert(
            "first".to_string(),
            LockedWorker {
                version: "1.0.0".to_string(),
                worker_type: LockedWorkerType::Image,
                dependencies: BTreeMap::new(),
                source: Some(LockedSource::Image {
                    image: "ghcr.io/iii-hq/first@sha256:abc".to_string(),
                }),
            },
        );
        good.write_to(&path).unwrap();
        let seeded = std::fs::read_to_string(&path).unwrap();

        // Craft a broken lockfile (unpinned image fails validate()).
        let mut bad = WorkerLockfile::default();
        bad.workers.insert(
            "broken".to_string(),
            LockedWorker {
                version: "1.0.0".to_string(),
                worker_type: LockedWorkerType::Image,
                dependencies: BTreeMap::new(),
                source: Some(LockedSource::Image {
                    image: "ghcr.io/iii-hq/broken:latest".to_string(),
                }),
            },
        );
        let _ = bad.write_to(&path).unwrap_err();

        // Failed write leaves seeded content intact.
        let after = std::fs::read_to_string(&path).unwrap();
        assert_eq!(after, seeded);
    }

    #[test]
    fn read_from_roundtrips_via_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("iii.lock");

        let mut lock = WorkerLockfile::default();
        lock.workers.insert(
            "hello".to_string(),
            LockedWorker {
                version: "1.0.0".to_string(),
                worker_type: LockedWorkerType::Binary,
                dependencies: BTreeMap::new(),
                source: Some(binary_source(
                    "aarch64-apple-darwin",
                    "https://example.com/h.tar.gz",
                    "a".repeat(64),
                )),
            },
        );
        lock.write_to(&path).unwrap();

        let parsed = WorkerLockfile::read_from(&path).unwrap();
        assert_eq!(parsed, lock);
    }

    #[test]
    fn engine_worker_round_trips_without_source() {
        let mut lock = WorkerLockfile::default();
        lock.workers.insert(
            "iii-http".to_string(),
            LockedWorker {
                version: "1.0.0".to_string(),
                worker_type: LockedWorkerType::Engine,
                dependencies: BTreeMap::new(),
                source: None,
            },
        );

        let yaml = lock.to_yaml().unwrap();
        assert!(
            !yaml.contains("source:"),
            "engine entry must not emit source"
        );

        let parsed = WorkerLockfile::from_yaml(&yaml).unwrap();
        let worker = parsed.workers.get("iii-http").unwrap();
        assert_eq!(worker.worker_type, LockedWorkerType::Engine);
        assert!(worker.source.is_none());
    }

    #[test]
    fn engine_worker_with_dependencies_round_trips() {
        let mut lock = WorkerLockfile::default();
        lock.workers.insert(
            "iii-http".to_string(),
            LockedWorker {
                version: "2.0.0".to_string(),
                worker_type: LockedWorkerType::Engine,
                dependencies: BTreeMap::from([("iii-stream".to_string(), "2.0.0".to_string())]),
                source: None,
            },
        );

        let yaml = lock.to_yaml().unwrap();
        let parsed = WorkerLockfile::from_yaml(&yaml).unwrap();
        let worker = parsed.workers.get("iii-http").unwrap();
        assert_eq!(worker.dependencies["iii-stream"], "2.0.0");
    }

    #[test]
    fn engine_worker_with_source_field_is_rejected() {
        let err = WorkerLockfile::from_yaml(
            r#"
version: 1
workers:
  iii-http:
    version: 1.0.0
    type: engine
    dependencies: {}
    source:
      kind: binary
      artifacts:
        x86_64-unknown-linux-gnu:
          url: https://example.com/h.tar.gz
          sha256: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
"#,
        )
        .unwrap_err();

        assert!(err.contains("iii-http"));
        assert!(err.contains("engine"));
        assert!(err.contains("source"));
    }

    #[test]
    fn manifest_hash_absent_roundtrips() {
        // Legacy locks written before Lane A don't carry the header.
        // Reading and writing one must leave it absent, not materialize
        // an empty string.
        let yaml = r#"version: 1
workers:
  hello:
    version: 1.0.0
    type: image
    dependencies: {}
    source:
      kind: image
      image: ghcr.io/iii-hq/hello@sha256:abc
"#;
        let parsed = WorkerLockfile::from_yaml(yaml).unwrap();
        assert_eq!(parsed.manifest_hash, None);

        let serialized = parsed.to_yaml().unwrap();
        assert!(
            !serialized.contains("manifest_hash"),
            "absent hash must stay absent in output; got: {serialized}"
        );
    }

    #[test]
    fn manifest_hash_present_roundtrips() {
        let hash = format!("{MANIFEST_HASH_PREFIX}{}", "a".repeat(64));
        let mut lock = WorkerLockfile {
            manifest_hash: Some(hash.clone()),
            ..Default::default()
        };
        lock.workers.insert(
            "hello".to_string(),
            LockedWorker {
                version: "1.0.0".to_string(),
                worker_type: LockedWorkerType::Image,
                dependencies: BTreeMap::new(),
                source: Some(LockedSource::Image {
                    image: "ghcr.io/iii-hq/hello@sha256:abc".to_string(),
                }),
            },
        );
        let serialized = lock.to_yaml().unwrap();
        assert!(serialized.contains(&hash));
        let parsed = WorkerLockfile::from_yaml(&serialized).unwrap();
        assert_eq!(parsed.manifest_hash, Some(hash));
    }

    #[test]
    fn declared_dependencies_absent_roundtrips() {
        let yaml = r#"version: 1
workers:
  hello:
    version: 1.0.0
    type: image
    dependencies: {}
    source:
      kind: image
      image: ghcr.io/iii-hq/hello@sha256:abc
"#;
        let parsed = WorkerLockfile::from_yaml(yaml).unwrap();
        assert_eq!(parsed.declared_dependencies, None);
        let serialized = parsed.to_yaml().unwrap();
        assert!(
            !serialized.contains("declared_dependencies"),
            "absent declared_dependencies must stay absent; got: {serialized}"
        );
    }

    #[test]
    fn declared_dependencies_roundtrip_with_entries() {
        let declared = BTreeMap::from([
            ("alpha".to_string(), "^1.0".to_string()),
            ("beta".to_string(), "~2.0".to_string()),
        ]);
        // Validation requires manifest_hash and declared_dependencies
        // to be paired and consistent. Compute the hash from the
        // declared deps so the roundtrip stays valid.
        let manifest_hash = Some(super::super::sync::compute_manifest_hash(&declared));
        let lock = WorkerLockfile {
            manifest_hash,
            declared_dependencies: Some(declared.clone()),
            ..Default::default()
        };
        let serialized = lock.to_yaml().unwrap();
        let parsed = WorkerLockfile::from_yaml(&serialized).unwrap();
        assert_eq!(parsed.declared_dependencies, Some(declared));
    }

    #[test]
    fn declared_dependencies_rejects_empty_range() {
        // Pair the empty range with a manifest_hash so the (None,Some)
        // pairing check doesn't fire first — we want to assert the
        // empty-range check specifically.
        let yaml = format!(
            "version: 1\nmanifest_hash: \"{prefix}{hex}\"\ndeclared_dependencies:\n  alpha: \"\"\nworkers: {{}}\n",
            prefix = MANIFEST_HASH_PREFIX,
            hex = "0".repeat(64),
        );
        let err = WorkerLockfile::from_yaml(&yaml).unwrap_err();
        assert!(err.contains("alpha"), "got: {err}");
        assert!(err.contains("empty range"), "got: {err}");
    }

    #[test]
    fn from_yaml_rejects_malformed_manifest_hash() {
        let yaml = r#"version: 1
manifest_hash: "not-even-close"
workers: {}
"#;
        let err = WorkerLockfile::from_yaml(yaml).unwrap_err();
        assert!(err.contains("manifest_hash"), "got: {err}");
        assert!(err.contains(MANIFEST_HASH_PREFIX), "got: {err}");
    }

    #[test]
    fn from_yaml_rejects_manifest_hash_with_wrong_algo_prefix() {
        // Algorithm version bump guard: a lock written by future iii with a
        // different hash algorithm must not be silently accepted by current
        // iii. The prefix is the whole point of this check.
        let yaml = format!(
            "version: 1\nmanifest_hash: \"sha256:v2:{}\"\nworkers: {{}}\n",
            "a".repeat(64)
        );
        let err = WorkerLockfile::from_yaml(&yaml).unwrap_err();
        assert!(err.contains("manifest_hash"), "got: {err}");
    }

    #[test]
    fn is_valid_manifest_hash_checks_prefix_and_hex_length() {
        assert!(is_valid_manifest_hash(&format!(
            "{MANIFEST_HASH_PREFIX}{}",
            "0".repeat(64)
        )));
        assert!(is_valid_manifest_hash(&format!(
            "{MANIFEST_HASH_PREFIX}{}",
            "abcdef0123456789".repeat(4)
        )));
        // Missing prefix.
        assert!(!is_valid_manifest_hash(&"0".repeat(64)));
        // Wrong prefix.
        assert!(!is_valid_manifest_hash(&format!(
            "sha512:v1:{}",
            "0".repeat(64)
        )));
        // Short hex.
        assert!(!is_valid_manifest_hash(&format!(
            "{MANIFEST_HASH_PREFIX}{}",
            "0".repeat(63)
        )));
        // Non-hex.
        assert!(!is_valid_manifest_hash(&format!(
            "{MANIFEST_HASH_PREFIX}{}",
            "z".repeat(64)
        )));
    }

    #[test]
    fn from_yaml_accepts_legacy_single_target_binary_source() {
        let parsed = WorkerLockfile::from_yaml(
            r#"
version: 1
workers:
  hello:
    version: 1.0.0
    type: binary
    dependencies: {}
    source:
      kind: binary
      target: aarch64-apple-darwin
      url: https://example.com/h.tar.gz
      sha256: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
"#,
        )
        .unwrap();

        let worker = parsed.workers.get("hello").unwrap();
        match worker.source.as_ref().unwrap() {
            LockedSource::Binary { artifacts } => {
                let artifact = artifacts.get("aarch64-apple-darwin").unwrap();
                assert_eq!(artifact.url, "https://example.com/h.tar.gz");
                assert_eq!(artifact.sha256, "a".repeat(64));
            }
            other => panic!("expected binary source, got {:?}", other),
        }
    }
}

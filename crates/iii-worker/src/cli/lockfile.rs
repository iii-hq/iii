// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

//! Read and write `iii.lock` for reproducible managed worker installs.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

const LOCKFILE_VERSION: u8 = 1;
const LOCKFILE_NAME: &str = "iii.lock";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerLockfile {
    pub version: u8,
    pub workers: BTreeMap<String, LockedWorker>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockedWorker {
    pub version: String,
    #[serde(rename = "type")]
    pub worker_type: LockedWorkerType,
    #[serde(default)]
    pub dependencies: BTreeMap<String, String>,
    pub source: LockedSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LockedWorkerType {
    Binary,
    Image,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum LockedSource {
    Binary {
        target: String,
        url: String,
        sha256: String,
    },
    Image {
        image: String,
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
        serde_yaml::from_str(input).map_err(|e| format!("failed to parse {LOCKFILE_NAME}: {e}"))
    }

    pub fn to_yaml(&self) -> Result<String, String> {
        serde_yaml::to_string(self)
            .map(|yaml| yaml.strip_prefix("---\n").unwrap_or(&yaml).to_string())
            .map_err(|e| format!("failed to serialize {LOCKFILE_NAME}: {e}"))
    }

    pub fn read_from(path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
        Self::from_yaml(&content)
    }

    pub fn write_to(&self, path: &Path) -> Result<(), String> {
        let yaml = self.to_yaml()?;
        std::fs::write(path, yaml).map_err(|e| format!("failed to write {}: {e}", path.display()))
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

        let mismatched: Vec<String> = worker_names
            .iter()
            .filter_map(|name| {
                let worker = self.workers.get(name)?;
                match &worker.source {
                    LockedSource::Binary { target, .. } if target != current_target => {
                        Some(format!("{name} locked for {target}"))
                    }
                    _ => None,
                }
            })
            .collect();

        if mismatched.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "{LOCKFILE_NAME} has binary worker(s) for a different target: {}. Current target: {current_target}",
                mismatched.join(", ")
            ))
        }
    }
}

pub fn lockfile_path() -> &'static Path {
    Path::new(LOCKFILE_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn lockfile_round_trips_with_sorted_workers() {
        let mut lock = WorkerLockfile::default();
        lock.workers.insert(
            "z-worker".to_string(),
            LockedWorker {
                version: "1.0.0".to_string(),
                worker_type: LockedWorkerType::Binary,
                dependencies: BTreeMap::new(),
                source: LockedSource::Binary {
                    target: "aarch64-apple-darwin".to_string(),
                    url: "https://example.com/z.tar.gz".to_string(),
                    sha256: "z".repeat(64),
                },
            },
        );
        lock.workers.insert(
            "a-worker".to_string(),
            LockedWorker {
                version: "1.0.0".to_string(),
                worker_type: LockedWorkerType::Image,
                dependencies: BTreeMap::from([("z-worker".to_string(), "^1.0.0".to_string())]),
                source: LockedSource::Image {
                    image: "ghcr.io/iii-hq/a-worker@sha256:abc".to_string(),
                },
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
                source: LockedSource::Binary {
                    target: "aarch64-apple-darwin".to_string(),
                    url: "https://example.com/h.tar.gz".to_string(),
                    sha256: "a".repeat(64),
                },
            },
        );

        assert!(
            lock.verify_config_workers(&["hello-worker".to_string()])
                .is_ok()
        );
    }

    #[test]
    fn verify_config_workers_for_target_rejects_binary_target_mismatch() {
        let mut lock = WorkerLockfile::default();
        lock.workers.insert(
            "hello-worker".to_string(),
            LockedWorker {
                version: "1.0.0".to_string(),
                worker_type: LockedWorkerType::Binary,
                dependencies: BTreeMap::new(),
                source: LockedSource::Binary {
                    target: "aarch64-apple-darwin".to_string(),
                    url: "https://example.com/h.tar.gz".to_string(),
                    sha256: "a".repeat(64),
                },
            },
        );

        let err = lock
            .verify_config_workers_for_target(
                &["hello-worker".to_string()],
                "x86_64-unknown-linux-gnu",
            )
            .unwrap_err();

        assert!(err.contains("hello-worker locked for aarch64-apple-darwin"));
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
                source: LockedSource::Binary {
                    target: "aarch64-apple-darwin".to_string(),
                    url: "https://example.com/h.tar.gz".to_string(),
                    sha256: "a".repeat(64),
                },
            },
        );

        assert!(
            lock.verify_config_workers_for_target(
                &["hello-worker".to_string()],
                "aarch64-apple-darwin"
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
                source: LockedSource::Image {
                    image: "ghcr.io/iii-hq/image@sha256:abc".to_string(),
                },
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
                source: LockedSource::Image {
                    image: "ghcr.io/iii-hq/extra@sha256:abc".to_string(),
                },
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
    fn write_to_reports_unwritable_paths() {
        // A path whose parent does not exist is always unwritable.
        let dir = tempfile::tempdir().unwrap();
        let bogus = dir.path().join("does").join("not").join("exist.lock");

        let lock = WorkerLockfile::default();
        let err = lock.write_to(&bogus).unwrap_err();

        assert!(err.contains(bogus.to_string_lossy().as_ref()));
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
                source: LockedSource::Binary {
                    target: "aarch64-apple-darwin".to_string(),
                    url: "https://example.com/h.tar.gz".to_string(),
                    sha256: "a".repeat(64),
                },
            },
        );
        lock.write_to(&path).unwrap();

        let parsed = WorkerLockfile::read_from(&path).unwrap();
        assert_eq!(parsed, lock);
    }
}

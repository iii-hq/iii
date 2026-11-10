// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! `.build/<container>/build.json`: what a container's last build was made from (spec 5.4), and
//! the comparison that tells whether that build is out of date (spec 5.5).

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::error::Result;

/// The `schema` of the `build.json` this version writes and reads.
pub const SCHEMA: u32 = 1;

/// A container's last successful build, as `build.json` holds it.
// Written by KAN-280 (host builds) and KAN-282 (`iii sandbox build`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildRecord {
    /// Version of the file's format: [`SCHEMA`].
    pub schema: u32,
    /// The container key.
    pub container: String,
    /// What the build was made from.
    pub source: BuildSource,
    /// The `scripts.build` that ran.
    pub command: String,
    /// `sha256:<hex>` of each dependency file at the root of the worker directory, by file name.
    pub dependency_files: BTreeMap<String, String>,
    /// The image a VM build wrote; a host build has none, and its `build.json` omits the key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<BuildImage>,
    /// When the build finished, RFC 3339 UTC to the second, e.g. `2026-11-10T14:03:22Z`:
    /// `Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)`.
    pub built_at: String,
}

/// The image a VM build wrote, and what it was built from and with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildImage {
    /// The image's file name in the build directory, `image.erofs`.
    pub file: String,
    /// `sha256:<hex>` of the image, the `build_id` `worker::build` returned.
    pub build_id: String,
    /// The base image the build started from.
    pub base_image: BuildBaseImage,
    /// The adapter that built the image.
    pub adapter: BuildAdapter,
    /// The sandbox contract version the adapter implemented.
    pub contract: BuildContract,
}

/// The base image of a VM build; `declared` and `digest` serialize as `null` when unknown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildBaseImage {
    /// The manifest's `runtime.base_image`; `None` when the adapter chose the image.
    pub declared: Option<String>,
    /// The image the adapter reports it started from.
    pub reference: String,
    /// Its resolved digest; `None` when the adapter does not know it.
    pub digest: Option<String>,
}

/// The adapter that built an image, as `adapter::info` names it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildAdapter {
    /// The adapter's name, e.g. `sandbox-libkrun`.
    pub name: String,
    /// The adapter's release.
    pub version: String,
}

/// The sandbox contract version an adapter implemented when it built an image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildContract {
    /// Contract major version.
    pub major: u32,
    /// Contract minor version.
    pub minor: u32,
}

/// What a build was made from: a locked package or a `path://` directory.
// Built by `source` (KAN-281); read by KAN-280 and KAN-282.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BuildSource {
    /// A registry package as locked; `sha256` is written `sha256:<hex>`.
    Package {
        name: String,
        version: String,
        sha256: String,
    },
    /// A `path://` worker.
    Path {
        /// As declared, relative to the compose file.
        dir: String,
    },
}

/// One difference between a build's record and the container's current inputs (spec 5.5).
// Read by KAN-280: the first-build and out-of-date checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// The package or directory changed: a package update, which rebuilds.
    Source { old: BuildSource, new: BuildSource },
    /// Dependency files added, removed or changed, by file name.
    Dependencies(Vec<String>),
    /// `runtime.base_image` changed.
    BaseImage {
        old: Option<String>,
        new: Option<String>,
    },
}

/// Reads `<build_dir>/build.json`; `None` when the container has no build yet.
// Wired by KAN-280 (build checks) and KAN-282 (`iii sandbox run`).
#[expect(unused_variables, reason = "KAN-281 fills this stub")]
pub fn read(build_dir: &Path) -> Result<Option<BuildRecord>> {
    todo!("KAN-281: read and parse <build_dir>/build.json")
}

/// Writes `<build_dir>/build.json` atomically: `build.json.tmp`, then a rename.
// Wired by KAN-280 (host builds) and KAN-282 (`iii sandbox build`).
#[expect(unused_variables, reason = "KAN-281 fills this stub")]
pub fn write(build_dir: &Path, record: &BuildRecord) -> Result<()> {
    todo!("KAN-281: write build.json through build.json.tmp and a rename")
}

/// The container's current source: `.workers/.packages.json` beside `compose_file`, or the file.
// Wired by KAN-280 (host builds) and KAN-282 (`iii sandbox build`).
#[expect(unused_variables, reason = "KAN-281 fills this stub")]
pub fn source(compose_file: &Path, container: &str) -> Result<BuildSource> {
    todo!("KAN-281: read the source from .workers/.packages.json or the compose file")
}

/// The worker manifest's `runtime.base_image`, or `None` when it sets none.
// Wired by KAN-282: `iii sandbox build`.
#[expect(unused_variables, reason = "KAN-281 fills this stub")]
pub fn declared_base_image(worker_dir: &Path) -> Result<Option<String>> {
    todo!("KAN-281: read runtime.base_image from the worker manifest")
}

/// `sha256:<hex>` of each dependency file of spec 5.4 present at the root of `worker_dir`.
// Wired by KAN-280 (host builds) and KAN-282 (`iii sandbox build`).
#[expect(unused_variables, reason = "KAN-281 fills this stub")]
pub fn dependency_files(worker_dir: &Path) -> Result<BTreeMap<String, String>> {
    todo!("KAN-281: hash the dependency files at the root of the worker directory")
}

/// Every difference of section 5.5 (`compose_file` as for [`source`]); empty when up to date.
// Wired by KAN-280: the first-build and out-of-date checks.
#[expect(unused_variables, reason = "KAN-281 fills this stub")]
pub fn changes(
    record: &BuildRecord,
    compose_file: &Path,
    container: &str,
    worker_dir: &Path,
) -> Result<Vec<Change>> {
    todo!("KAN-281: compare the record with the container's current source and inputs")
}

/// `<compose dir>/.build/<container>`, the container's build directory.
// Wired by KAN-280 (`III_BUILD_DIR`, `compose::build`).
pub fn build_dir(compose_dir: &Path, container: &str) -> PathBuf {
    compose_dir.join(".build").join(container)
}

/// `<compose dir>/.workers/<container>`, the container's package directory.
// Wired by KAN-281 (`materialize_project`) and KAN-280 (`III_PACKAGE_DIR`).
pub fn package_dir(compose_dir: &Path, container: &str) -> PathBuf {
    compose_dir.join(".workers").join(container)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_round_trips_the_spec_example() {
        let example = serde_json::json!({
            "schema": 1,
            "container": "onboarding",
            "source": { "kind": "package", "name": "onboarding", "version": "0.2.0", "sha256": "sha256:8c3406" },
            "command": "iii sandbox build",
            "dependency_files": { "package.json": "sha256:2f1c", "package-lock.json": "sha256:9a07" },
            "image": {
                "file": "image.erofs",
                "build_id": "sha256:7d41",
                "base_image": { "declared": null, "reference": "docker.io/iiidev/node:latest", "digest": "sha256:b1e2" },
                "adapter": { "name": "sandbox-libkrun", "version": "0.4.0" },
                "contract": { "major": 1, "minor": 3 }
            },
            "built_at": "2026-11-10T14:03:22Z"
        });
        let record: BuildRecord = serde_json::from_value(example.clone()).expect("parses");
        assert_eq!(record.schema, SCHEMA);
        assert_eq!(serde_json::to_value(&record).expect("serializes"), example);
    }

    #[test]
    fn host_build_omits_the_image() {
        let record = BuildRecord {
            schema: SCHEMA,
            container: "api".to_string(),
            source: BuildSource::Path {
                dir: "./api".to_string(),
            },
            command: "npm ci".to_string(),
            dependency_files: BTreeMap::new(),
            image: None,
            built_at: "2026-11-10T14:03:22Z".to_string(),
        };
        let mut json = serde_json::to_value(&record).expect("serializes");
        assert_eq!(json.get("image"), None);
        assert_eq!(
            serde_json::from_value::<BuildRecord>(json.clone()).expect("parses"),
            record
        );
        json["image"] = serde_json::Value::Null;
        assert_eq!(
            serde_json::from_value::<BuildRecord>(json).expect("parses a null image"),
            record
        );
    }

    #[test]
    fn path_source_matches_the_spec() {
        let source = BuildSource::Path {
            dir: "./link".to_string(),
        };
        assert_eq!(
            serde_json::to_value(&source).expect("serializes"),
            serde_json::json!({ "kind": "path", "dir": "./link" })
        );
    }

    #[test]
    fn directories_sit_beside_the_compose_file() {
        let project = Path::new("/project");
        assert_eq!(
            build_dir(project, "link"),
            Path::new("/project/.build/link")
        );
        assert_eq!(
            package_dir(project, "onboarding"),
            Path::new("/project/.workers/onboarding")
        );
    }
}

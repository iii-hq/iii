// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! `.build/<container>/build.json`: what a container's last build was made from (spec 5.4), and
//! the comparison that tells whether that build is out of date (spec 5.5).
//!
//! Both project directories sit beside the compose file (spec 3.2): `.build/<container>/` holds
//! a container's build, `.workers/<container>/` the copy of its package. Each gets a
//! `.gitignore` of `*` from Compose, so a project stays clean without editing its own.

use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{ComposeError, Result};

/// The `schema` of the `build.json` this version writes and reads.
pub const SCHEMA: u32 = 1;

/// The record's file name in a build directory.
pub(crate) const BUILD_FILE: &str = "build.json";
/// Beside the compose file: one package directory per container (spec 3.2).
pub(crate) const WORKERS_DIR: &str = ".workers";
/// Beside the compose file: one build directory per container (spec 3.2).
pub(crate) const BUILDS_DIR: &str = ".build";
/// What Compose writes to the `.gitignore` of [`WORKERS_DIR`] and [`BUILDS_DIR`].
const IGNORE_ALL: &[u8] = b"*\n";

/// The dependency files of spec 5.4, looked up at the root of a worker directory.
pub(crate) const DEPENDENCY_FILES: [&str; 13] = [
    "package.json",
    "package-lock.json",
    "pnpm-lock.yaml",
    "yarn.lock",
    "bun.lockb",
    "requirements.txt",
    "pyproject.toml",
    "uv.lock",
    "poetry.lock",
    "Cargo.toml",
    "Cargo.lock",
    "go.mod",
    "go.sum",
];

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
///
/// A `build.json.tmp` left by an interrupted write is not a build. A `build.json` that does not
/// parse, or has another [`SCHEMA`], is an error that asks for a rebuild, which replaces it.
// Wired by KAN-280 (build checks) and KAN-282 (`iii sandbox run`).
pub fn read(build_dir: &Path) -> Result<Option<BuildRecord>> {
    let path = build_dir.join(BUILD_FILE);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(ComposeError::Io { path, source }),
    };
    let record: BuildRecord = serde_json::from_slice(&bytes).map_err(|error| {
        invalid_data(
            &path,
            format!("not a valid build.json ({error}); rebuild the container to replace it"),
        )
    })?;
    if record.schema != SCHEMA {
        return Err(invalid_data(
            &path,
            format!(
                "build.json has schema {}, and this iii reads schema {SCHEMA}; rebuild the \
                 container to replace it",
                record.schema
            ),
        ));
    }
    Ok(Some(record))
}

/// Writes `<build_dir>/build.json` atomically: `build.json.tmp`, then a rename.
///
/// Creates `build_dir`, and the `.gitignore` of its `.build/` when there is none yet (spec
/// 3.2). Only a parent named `.build` gets one: a `build_dir` elsewhere leaves its parent alone.
// Wired by KAN-280 (host builds) and KAN-282 (`iii sandbox build`).
pub fn write(build_dir: &Path, record: &BuildRecord) -> Result<()> {
    if let Some(builds) = build_dir
        .parent()
        .filter(|parent| parent.file_name() == Some(std::ffi::OsStr::new(BUILDS_DIR)))
    {
        ignore_all(builds)?;
    }
    std::fs::create_dir_all(build_dir).map_err(|source| ComposeError::Io {
        path: build_dir.to_path_buf(),
        source,
    })?;
    let path = build_dir.join(BUILD_FILE);
    let mut json = serde_json::to_vec_pretty(record).map_err(|error| ComposeError::Io {
        path: path.clone(),
        source: std::io::Error::other(error),
    })?;
    json.push(b'\n');
    replace_file(&path, &json)
}

/// The container's current source: `.workers/.packages.json` beside `compose_file`, or the file.
///
/// The compose file says which kind the container is. A `path://` gives its directory as
/// declared (`${VAR}` expanded, as Compose reads it). A `package://` gives the package that
/// `.workers/<container>/` holds, as `.workers/.packages.json` records it, and is
/// `PACKAGE_NOT_INSTALLED` until `materialize_project` copied it there.
// Wired by KAN-280 (host builds) and KAN-282 (`iii sandbox build`).
pub fn source(compose_file: &Path, container: &str) -> Result<BuildSource> {
    let worker = declared_worker(compose_file, container)?;
    if let Some(dir) = worker.strip_prefix("path://").filter(|dir| !dir.is_empty()) {
        return Ok(BuildSource::Path {
            dir: dir.to_string(),
        });
    }
    if worker
        .strip_prefix("package://")
        .is_some_and(|reference| !reference.is_empty())
    {
        return crate::registry::read_project_packages(compose_dir(compose_file))?
            .and_then(|mut packages| packages.containers.remove(container))
            .map(|package| BuildSource::Package {
                name: package.name,
                version: package.version,
                sha256: package.sha256,
            })
            .ok_or_else(|| ComposeError::PackageNotInstalled {
                container: container.to_string(),
            });
    }
    Err(ComposeError::UnsupportedWorkerSource {
        container: container.to_string(),
        source_uri: worker,
    })
}

/// The worker manifest's `runtime.base_image`, or `None` when it sets none.
///
/// A directory without `iii.worker.yaml` sets none; a manifest that does not parse is
/// `INVALID_MANIFEST`, as everywhere else Compose reads one.
// Wired by KAN-282: `iii sandbox build`.
pub fn declared_base_image(worker_dir: &Path) -> Result<Option<String>> {
    Ok(crate::manifest::read_manifest(worker_dir)?.and_then(|manifest| manifest.base_image))
}

/// `sha256:<hex>` of each dependency file of spec 5.4 present at the root of `worker_dir`.
///
/// Only regular files count, a symlink to one included; a missing file is left out.
// Wired by KAN-280 (host builds) and KAN-282 (`iii sandbox build`).
pub fn dependency_files(worker_dir: &Path) -> Result<BTreeMap<String, String>> {
    let mut hashes = BTreeMap::new();
    for name in DEPENDENCY_FILES {
        let path = worker_dir.join(name);
        match std::fs::metadata(&path) {
            Ok(metadata) if metadata.is_file() => {}
            Ok(_) => continue,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => continue,
            Err(source) => return Err(ComposeError::Io { path, source }),
        }
        hashes.insert(name.to_string(), file_sha256(&path)?);
    }
    Ok(hashes)
}

/// Every difference of section 5.5 (`compose_file` as for [`source`]); empty when up to date.
///
/// In order: the source, the dependency files (every name added, removed or changed, sorted),
/// and, for a VM build, the declared base image. A host build records no image, so its base
/// image is not compared.
// Wired by KAN-280: the first-build and out-of-date checks.
pub fn changes(
    record: &BuildRecord,
    compose_file: &Path,
    container: &str,
    worker_dir: &Path,
) -> Result<Vec<Change>> {
    let mut changes = Vec::new();

    let current = source(compose_file, container)?;
    if current != record.source {
        changes.push(Change::Source {
            old: record.source.clone(),
            new: current,
        });
    }

    let files = dependency_files(worker_dir)?;
    let changed: Vec<String> = record
        .dependency_files
        .keys()
        .chain(files.keys())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|name| record.dependency_files.get(*name) != files.get(*name))
        .cloned()
        .collect();
    if !changed.is_empty() {
        changes.push(Change::Dependencies(changed));
    }

    if let Some(image) = &record.image {
        let declared = declared_base_image(worker_dir)?;
        if declared != image.base_image.declared {
            changes.push(Change::BaseImage {
                old: image.base_image.declared.clone(),
                new: declared,
            });
        }
    }
    Ok(changes)
}

/// `<compose dir>/.build/<container>`, the container's build directory.
// Wired by KAN-280 (`III_BUILD_DIR`, `compose::build`).
pub fn build_dir(compose_dir: &Path, container: &str) -> PathBuf {
    compose_dir.join(BUILDS_DIR).join(container)
}

/// `<compose dir>/.workers/<container>`, the container's package directory.
// Wired by KAN-281 (`materialize_project`) and KAN-280 (`III_PACKAGE_DIR`).
pub fn package_dir(compose_dir: &Path, container: &str) -> PathBuf {
    compose_dir.join(WORKERS_DIR).join(container)
}

/// The directory of `compose_file`, where `.build/` and `.workers/` sit.
pub(crate) fn compose_dir(compose_file: &Path) -> &Path {
    compose_file
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
}

/// Creates `dir` and its `.gitignore` of `*` (spec 3.2). A `.gitignore` already there is the
/// operator's, or an earlier run's, and is kept as it is.
pub(crate) fn ignore_all(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir).map_err(|source| ComposeError::Io {
        path: dir.to_path_buf(),
        source,
    })?;
    let path = dir.join(".gitignore");
    match std::fs::symlink_metadata(&path) {
        Ok(_) => Ok(()),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            replace_file(&path, IGNORE_ALL)
        }
        Err(source) => Err(ComposeError::Io { path, source }),
    }
}

/// Writes `path` through `<path>.tmp` and a rename, so a reader sees the old file or the new
/// one, never a part. A crash leaves at most the `.tmp`, which the next write replaces.
pub(crate) fn replace_file(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".tmp");
    let temporary = PathBuf::from(temporary);
    let written = std::fs::File::create(&temporary).and_then(|mut file| {
        file.write_all(bytes)?;
        file.sync_all()
    });
    written.map_err(|source| ComposeError::Io {
        path: temporary.clone(),
        source,
    })?;
    std::fs::rename(&temporary, path).map_err(|source| ComposeError::Io {
        path: path.to_path_buf(),
        source,
    })
}

/// A file Compose wrote that it cannot read back, reported as an I/O error on `path`.
pub(crate) fn invalid_data(path: &Path, message: String) -> ComposeError {
    ComposeError::Io {
        path: path.to_path_buf(),
        source: std::io::Error::new(std::io::ErrorKind::InvalidData, message),
    }
}

/// The `worker:` of `container` in `compose_file`, `${VAR}` expanded as Compose reads it.
///
/// Only that value is read: the rest of the file may reference variables the caller's
/// environment does not hold (`iii sandbox build` runs with one container's env).
fn declared_worker(compose_file: &Path, container: &str) -> Result<String> {
    let text = std::fs::read_to_string(compose_file).map_err(|source| ComposeError::Io {
        path: compose_file.to_path_buf(),
        source,
    })?;
    let document: serde_yaml::Value =
        serde_yaml::from_str(&text).map_err(|error| ComposeError::Yaml {
            path: compose_file.to_path_buf(),
            message: error.to_string(),
        })?;
    let declaration = document
        .get("containers")
        .and_then(|containers| containers.get(container))
        .ok_or_else(|| ComposeError::UnknownContainer {
            container: container.to_string(),
        })?;
    let worker = declaration
        .get("worker")
        .and_then(serde_yaml::Value::as_str)
        .ok_or_else(|| ComposeError::Yaml {
            path: compose_file.to_path_buf(),
            message: format!("containers.{container}.worker must be a string"),
        })?;
    crate::interpolate::expand(worker, compose_file, &|name| std::env::var(name).ok())
}

/// `sha256:<hex>` of the file at `path`.
fn file_sha256(path: &Path) -> Result<String> {
    let io = |source| ComposeError::Io {
        path: path.to_path_buf(),
        source,
    };
    let mut file = std::fs::File::open(path).map_err(io)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(io)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("sha256:{}", hex::encode(hasher.finalize())))
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

    fn vm_record(source: BuildSource, declared: Option<&str>) -> BuildRecord {
        BuildRecord {
            schema: SCHEMA,
            container: "link".to_string(),
            source,
            command: "iii sandbox build".to_string(),
            dependency_files: BTreeMap::new(),
            image: Some(BuildImage {
                file: "image.erofs".to_string(),
                build_id: "sha256:7d41".to_string(),
                base_image: BuildBaseImage {
                    declared: declared.map(str::to_string),
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

    fn path_source(dir: &str) -> BuildSource {
        BuildSource::Path {
            dir: dir.to_string(),
        }
    }

    fn package_source(version: &str, sha256: &str) -> BuildSource {
        BuildSource::Package {
            name: "onboarding".to_string(),
            version: version.to_string(),
            sha256: sha256.to_string(),
        }
    }

    fn write_file(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    /// `.workers/.packages.json` holding `onboarding` at `version`.
    fn record_package(project: &Path, version: &str, sha256: &str) {
        write_file(
            &project.join(".workers/.packages.json"),
            &serde_json::json!({
                "schema": 1,
                "containers": {
                    "onboarding": { "name": "onboarding", "version": version, "sha256": sha256 }
                }
            })
            .to_string(),
        );
    }

    /// A compose file under another name than the default, with a `path://` and a package.
    fn project() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("custom.yaml");
        write_file(
            &file,
            "containers:\n  link:\n    worker: path://./link\n  onboarding:\n    worker: package://api.workers.iii.dev/onboarding\n    version: '0.2.0'\n",
        );
        write_file(
            &dir.path().join("link/iii.worker.yaml"),
            "runtime:\n  base_image: docker.io/iiidev/node\nscripts:\n  start: node index.js\n",
        );
        write_file(&dir.path().join("link/package.json"), "{}\n");
        write_file(&dir.path().join("link/package-lock.json"), "{\"v\":1}\n");
        (dir, file)
    }

    /// The record a build of `link` would write right now.
    fn current_link_record(file: &Path) -> BuildRecord {
        let worker = file.parent().unwrap().join("link");
        BuildRecord {
            dependency_files: dependency_files(&worker).unwrap(),
            ..vm_record(
                source(file, "link").unwrap(),
                declared_base_image(&worker).unwrap().as_deref(),
            )
        }
    }

    #[test]
    fn write_then_read_round_trips_and_leaves_no_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let build = build_dir(dir.path(), "link");
        let record = vm_record(path_source("./link"), Some("docker.io/iiidev/node"));

        assert_eq!(read(&build).unwrap(), None, "no build yet");
        write(&build, &record).unwrap();

        assert_eq!(read(&build).unwrap(), Some(record.clone()));
        assert!(!build.join("build.json.tmp").exists());
        let text = std::fs::read_to_string(build.join("build.json")).unwrap();
        assert!(text.ends_with("}\n"), "{text}");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&text).unwrap()["image"]["base_image"]["digest"],
            serde_json::Value::Null,
            "an unknown digest is written as null"
        );

        let host = BuildRecord {
            image: None,
            command: "npm ci".to_string(),
            ..record
        };
        write(&build, &host).unwrap();
        assert_eq!(read(&build).unwrap(), Some(host));
    }

    #[test]
    fn a_partial_temporary_file_is_not_a_build_and_the_next_write_replaces_it() {
        let dir = tempfile::tempdir().unwrap();
        let build = build_dir(dir.path(), "link");
        // What a write interrupted before its rename leaves behind.
        write_file(&build.join("build.json.tmp"), "{\"schema\": 1, \"conta");
        assert_eq!(read(&build).unwrap(), None);

        let record = vm_record(path_source("./link"), None);
        write(&build, &record).unwrap();
        assert_eq!(read(&build).unwrap(), Some(record));
        assert!(!build.join("build.json.tmp").exists());
    }

    #[test]
    fn a_truncated_or_newer_build_json_asks_for_a_rebuild() {
        let dir = tempfile::tempdir().unwrap();
        let build = build_dir(dir.path(), "link");
        let full = serde_json::to_string(&vm_record(path_source("./link"), None)).unwrap();

        write_file(&build.join("build.json"), &full[..full.len() / 2]);
        let error = read(&build).unwrap_err();
        assert_eq!(error.code(), "IO_ERROR");
        assert!(
            error.to_string().contains("not a valid build.json"),
            "{error}"
        );
        assert!(error.to_string().contains("rebuild"), "{error}");

        write_file(
            &build.join("build.json"),
            &full.replace("\"schema\":1", "\"schema\":2"),
        );
        let error = read(&build).unwrap_err();
        assert!(error.to_string().contains("schema 2"), "{error}");
    }

    #[test]
    fn write_adds_the_gitignore_of_an_existing_build_directory_only() {
        let dir = tempfile::tempdir().unwrap();
        // A project that already had builds, and no `.gitignore` in `.build/`.
        write_file(&dir.path().join(".build/api/old.txt"), "old");
        let record = vm_record(path_source("./link"), None);
        write(&build_dir(dir.path(), "link"), &record).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join(".build/.gitignore")).unwrap(),
            "*\n"
        );

        // One written by hand is kept.
        std::fs::write(dir.path().join(".build/.gitignore"), "image.erofs\n").unwrap();
        write(&build_dir(dir.path(), "api"), &record).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join(".build/.gitignore")).unwrap(),
            "image.erofs\n"
        );

        // A build directory outside `.build/` leaves its parent alone.
        write(&dir.path().join("elsewhere/link"), &record).unwrap();
        assert!(!dir.path().join("elsewhere/.gitignore").exists());
    }

    #[test]
    fn source_reads_a_path_worker_as_declared_in_the_named_file() {
        let (_dir, file) = project();
        assert_eq!(source(&file, "link").unwrap(), path_source("./link"));

        write_file(
            &file,
            "containers:\n  link:\n    worker: path://${KAN281_UNSET_LINK_DIR:-./other}\n    environment:\n      KEY: ${KAN281_UNSET_SECRET}\n",
        );
        assert_eq!(
            source(&file, "link").unwrap(),
            path_source("./other"),
            "the worker is expanded; a variable elsewhere in the file is not needed"
        );
        assert_eq!(
            source(&file, "missing").unwrap_err().code(),
            "UNKNOWN_CONTAINER"
        );
    }

    #[test]
    fn source_reads_a_package_from_the_packages_record() {
        let (dir, file) = project();
        assert_eq!(
            source(&file, "onboarding").unwrap_err().code(),
            "PACKAGE_NOT_INSTALLED",
            "no .workers/ yet"
        );

        record_package(dir.path(), "0.2.0", "sha256:8c3406");
        assert_eq!(
            source(&file, "onboarding").unwrap(),
            package_source("0.2.0", "sha256:8c3406")
        );

        write_file(
            &dir.path().join(".workers/.packages.json"),
            "{\"schema\": 1",
        );
        let error = source(&file, "onboarding").unwrap_err();
        assert!(error.to_string().contains(".packages.json"), "{error}");
    }

    #[test]
    fn declared_base_image_reads_the_manifest_of_a_bundle_and_of_a_path_worker() {
        let (dir, _file) = project();
        let bundle = package_dir(dir.path(), "onboarding");
        write_file(
            &bundle.join("iii.worker.yaml"),
            "name: onboarding\nscripts:\n  start: node index.js\n",
        );
        assert_eq!(declared_base_image(&bundle).unwrap(), None);
        write_file(
            &bundle.join("iii.worker.yaml"),
            "name: onboarding\nruntime:\n  kind: node\n  base_image: docker.io/iiidev/python\n",
        );
        assert_eq!(
            declared_base_image(&bundle).unwrap().as_deref(),
            Some("docker.io/iiidev/python")
        );

        let link = dir.path().join("link");
        assert_eq!(
            declared_base_image(&link).unwrap().as_deref(),
            Some("docker.io/iiidev/node")
        );
        assert_eq!(
            declared_base_image(&dir.path().join("no-manifest")).unwrap(),
            None
        );
        write_file(&link.join("iii.worker.yaml"), "runtime: {base_image: 5}\n");
        assert_eq!(
            declared_base_image(&link).unwrap_err().code(),
            "INVALID_MANIFEST"
        );
    }

    #[test]
    fn dependency_files_hash_exactly_the_spec_list_at_the_root() {
        let dir = tempfile::tempdir().unwrap();
        for name in DEPENDENCY_FILES {
            write_file(&dir.path().join(name), name);
        }
        write_file(&dir.path().join("README.md"), "not a dependency file");
        write_file(&dir.path().join("sub/package.json"), "not at the root");
        let hashes = dependency_files(dir.path()).unwrap();
        assert_eq!(
            hashes.keys().map(String::as_str).collect::<BTreeSet<_>>(),
            BTreeSet::from(DEPENDENCY_FILES)
        );
        for (name, hash) in &hashes {
            assert_eq!(
                *hash,
                format!("sha256:{}", hex::encode(Sha256::digest(name.as_bytes())))
            );
        }

        let sparse = tempfile::tempdir().unwrap();
        write_file(&sparse.path().join("go.mod"), "");
        std::fs::create_dir(sparse.path().join("go.sum")).unwrap();
        assert_eq!(
            dependency_files(sparse.path()).unwrap(),
            BTreeMap::from([(
                "go.mod".to_string(),
                "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
                    .to_string()
            )]),
            "an empty file counts; a directory does not"
        );
        assert!(
            dependency_files(&sparse.path().join("missing"))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn an_up_to_date_build_has_no_changes() {
        let (dir, file) = project();
        let worker = dir.path().join("link");
        let record = current_link_record(&file);
        assert_eq!(changes(&record, &file, "link", &worker).unwrap(), []);

        // Code outside the dependency files is not a change.
        write_file(&worker.join("index.js"), "console.log(1)");
        assert_eq!(changes(&record, &file, "link", &worker).unwrap(), []);

        let host = BuildRecord {
            image: None,
            ..record
        };
        assert_eq!(changes(&host, &file, "link", &worker).unwrap(), []);
    }

    #[test]
    fn a_new_package_or_directory_is_a_source_change() {
        let (dir, file) = project();
        let bundle = package_dir(dir.path(), "onboarding");
        record_package(dir.path(), "0.2.0", "sha256:8c3406");
        let record = BuildRecord {
            container: "onboarding".to_string(),
            ..vm_record(package_source("0.2.0", "sha256:8c3406"), None)
        };
        assert_eq!(changes(&record, &file, "onboarding", &bundle).unwrap(), []);

        record_package(dir.path(), "0.3.0", "sha256:aa11");
        assert_eq!(
            changes(&record, &file, "onboarding", &bundle).unwrap(),
            [Change::Source {
                old: package_source("0.2.0", "sha256:8c3406"),
                new: package_source("0.3.0", "sha256:aa11"),
            }]
        );

        let worker = dir.path().join("link");
        let record = current_link_record(&file);
        write_file(&file, "containers:\n  link:\n    worker: path://link\n");
        assert_eq!(
            changes(&record, &file, "link", &worker).unwrap(),
            [Change::Source {
                old: path_source("./link"),
                new: path_source("link"),
            }]
        );
    }

    #[test]
    fn added_changed_and_removed_dependency_files_are_named() {
        let (dir, file) = project();
        let worker = dir.path().join("link");
        let record = current_link_record(&file);

        write_file(&worker.join("package.json"), "{\"dependencies\":{}}\n");
        write_file(&worker.join("yarn.lock"), "");
        std::fs::remove_file(worker.join("package-lock.json")).unwrap();
        assert_eq!(
            changes(&record, &file, "link", &worker).unwrap(),
            [Change::Dependencies(vec![
                "package-lock.json".to_string(),
                "package.json".to_string(),
                "yarn.lock".to_string(),
            ])]
        );
    }

    #[test]
    fn a_new_declared_base_image_is_a_change_of_a_vm_build_only() {
        let (dir, file) = project();
        let worker = dir.path().join("link");
        let record = current_link_record(&file);

        write_file(
            &worker.join("iii.worker.yaml"),
            "runtime:\n  base_image: docker.io/iiidev/python\n",
        );
        assert_eq!(
            changes(&record, &file, "link", &worker).unwrap(),
            [Change::BaseImage {
                old: Some("docker.io/iiidev/node".to_string()),
                new: Some("docker.io/iiidev/python".to_string()),
            }]
        );
        std::fs::remove_file(worker.join("iii.worker.yaml")).unwrap();
        assert_eq!(
            changes(&record, &file, "link", &worker).unwrap(),
            [Change::BaseImage {
                old: Some("docker.io/iiidev/node".to_string()),
                new: None,
            }]
        );

        let host = BuildRecord {
            image: None,
            ..record
        };
        assert_eq!(changes(&host, &file, "link", &worker).unwrap(), []);
    }

    #[test]
    fn every_change_is_reported_together_in_order() {
        let (dir, file) = project();
        let worker = dir.path().join("link");
        let record = current_link_record(&file);

        write_file(&file, "containers:\n  link:\n    worker: path://./link/\n");
        write_file(&worker.join("package.json"), "{\"name\":\"link\"}\n");
        write_file(
            &worker.join("iii.worker.yaml"),
            "runtime:\n  base_image: docker.io/iiidev/python\n",
        );
        assert_eq!(
            changes(&record, &file, "link", &worker).unwrap(),
            [
                Change::Source {
                    old: path_source("./link"),
                    new: path_source("./link/"),
                },
                Change::Dependencies(vec!["package.json".to_string()]),
                Change::BaseImage {
                    old: Some("docker.io/iiidev/node".to_string()),
                    new: Some("docker.io/iiidev/python".to_string()),
                },
            ]
        );
    }
}

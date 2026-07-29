// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! `iii.worker.yaml` subset parser and start-command resolution.
//!
//! Compose reads only `name` and `scripts.start`. The manifest is another
//! tool's file, so unknown keys are tolerated here — the opposite of the
//! compose file's strictness. This parser is deliberately independent from
//! `crates/iii-worker`: compose must not inherit the legacy lifecycle system.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::{
    config::{ComposeFile, Container, WorkerSource},
    error::{ComposeError, Result},
};

pub const MANIFEST_FILE: &str = "iii.worker.yaml";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartSpec {
    /// A shell command: the compose `run` or the manifest's `scripts.start`.
    Shell(String),
    /// A resolved package binary. Package resolution is not implemented yet, so
    /// nothing produces this variant today.
    Exec { program: PathBuf, args: Vec<String> },
}

#[derive(Debug, Clone)]
pub struct Manifest {
    pub name: Option<String>,
    pub start: Option<String>,
}

/// Reads the manifest in `dir`, if there is one. A missing manifest is not an
/// error: identity then comes from the compose container key and the start
/// command from `run`.
pub fn read_manifest(dir: &Path) -> Result<Option<Manifest>> {
    let path = dir.join(MANIFEST_FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(ComposeError::Io { path, source }),
    };
    let raw: RawManifest =
        serde_yaml::from_str(&text).map_err(|err| ComposeError::InvalidManifest {
            path: path.clone(),
            message: err.to_string(),
        })?;
    Ok(Some(Manifest {
        name: raw.name,
        start: raw.scripts.and_then(|scripts| scripts.start),
    }))
}

/// Resolves how a container starts. Compose `run` wins over the manifest's
/// `scripts.start`: the compose file is the operator's file, the manifest is the
/// worker author's default.
pub fn resolve_start(key: &str, container: &Container) -> Result<StartSpec> {
    let dir = match &container.worker {
        WorkerSource::Package { .. } => {
            return Err(ComposeError::PackageResolutionUnimplemented {
                container: key.to_string(),
            });
        }
        WorkerSource::Path { dir, .. } => dir,
    };

    if !dir.is_dir() {
        return Err(ComposeError::MissingWorkerDirectory {
            container: key.to_string(),
            path: dir.clone(),
        });
    }

    let manifest = read_manifest(dir)?;
    if let Some(manifest) = &manifest {
        // A manifest that renames the container would split identity between
        // the compose graph and the engine registration.
        if let Some(name) = &manifest.name {
            if name != key {
                return Err(ComposeError::ManifestNameMismatch {
                    container: key.to_string(),
                    path: dir.join(MANIFEST_FILE),
                    manifest_name: name.clone(),
                });
            }
        }
    }

    if let Some(run) = &container.scripts.run {
        return Ok(StartSpec::Shell(run.clone()));
    }
    if let Some(start) = manifest.and_then(|manifest| manifest.start) {
        return Ok(StartSpec::Shell(start));
    }
    Err(ComposeError::MissingStartCommand {
        container: key.to_string(),
        manifest: dir.join(MANIFEST_FILE),
    })
}

/// Result of `iii compose validate`.
#[derive(Debug, Clone)]
pub struct ValidationReport {
    pub project: String,
    pub start_order: Vec<String>,
    /// Containers whose start command resolved, in start order.
    pub resolved: Vec<(String, StartSpec)>,
    /// `package://` containers, skipped because registry resolution is not
    /// implemented yet.
    pub deferred_packages: Vec<String>,
}

/// Validates everything that can be checked without an engine: schema, graph,
/// worker directories, manifests and start commands.
pub fn validate_offline(file: &ComposeFile) -> Result<ValidationReport> {
    let start_order = file.start_order()?;
    let mut resolved = Vec::new();
    let mut deferred_packages = Vec::new();

    for key in &start_order {
        let Some(container) = file.containers.get(key) else {
            continue;
        };
        if matches!(container.worker, WorkerSource::Package { .. }) {
            deferred_packages.push(key.clone());
            continue;
        }
        resolved.push((key.clone(), resolve_start(key, container)?));
    }

    Ok(ValidationReport {
        project: file.name.clone(),
        start_order,
        resolved,
        deferred_packages,
    })
}

#[derive(Debug, Deserialize)]
struct RawManifest {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    scripts: Option<RawManifestScripts>,
}

#[derive(Debug, Deserialize)]
struct RawManifestScripts {
    #[serde(default)]
    start: Option<String>,
}

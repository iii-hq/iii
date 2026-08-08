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
        // A package has no start command until its artefact is installed, and
        // installing needs the network. `lifecycle::start_one` does that and
        // builds the `Exec` itself; nothing else should reach here.
        WorkerSource::Package { .. } => {
            return Err(ComposeError::PackageNotInstalled {
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
    // A manifest that renames the container would split identity between the
    // compose graph and the engine registration.
    if let Some(name) = manifest
        .as_ref()
        .and_then(|manifest| manifest.name.as_ref())
        && name != key
    {
        return Err(ComposeError::ManifestNameMismatch {
            container: key.to_string(),
            path: dir.join(MANIFEST_FILE),
            manifest_name: name.clone(),
        });
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

/// What one container would do on `up`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerPlan {
    pub key: String,
    pub start: StartSpec,
    pub working_dir: PathBuf,
    /// Configuration entry the daemon would fetch before starting it.
    pub config_name: Option<String>,
    /// Names only. Values may be secrets and are never reported.
    pub environment: Vec<String>,
    pub env_file: Vec<PathBuf>,
    pub startup_timeout: std::time::Duration,
}

/// Result of `compose::validate`.
#[derive(Debug, Clone)]
pub struct ValidationReport {
    pub project: String,
    /// Namespace the project's workers would register under.
    pub namespace: String,
    pub start_order: Vec<String>,
    /// Containers whose start command resolved, in start order.
    pub resolved: Vec<ContainerPlan>,
    /// `package://` containers, skipped because registry resolution is not
    /// implemented yet.
    pub deferred_packages: Vec<String>,
}

/// Validates everything that can be checked without an engine: schema, graph,
/// worker directories, manifests and start commands.
pub fn validate_offline(file: &ComposeFile, namespace: &str) -> Result<ValidationReport> {
    let start_order = file.start_order()?;
    let mut resolved = Vec::new();
    let mut deferred_packages = Vec::new();

    for key in &start_order {
        let Some(container) = file.containers.get(key) else {
            continue;
        };
        let worker_dir = match &container.worker {
            WorkerSource::Package { .. } => {
                deferred_packages.push(key.clone());
                continue;
            }
            WorkerSource::Path { dir, .. } => dir,
        };
        // Env files are read at spawn time, but a missing one must fail here:
        // finding out at `up` means half the graph is already running.
        for env_file in &container.env_file {
            if !env_file.is_file() {
                return Err(ComposeError::MissingEnvFile {
                    container: key.clone(),
                    path: env_file.clone(),
                });
            }
        }

        resolved.push(ContainerPlan {
            key: key.clone(),
            start: resolve_start(key, container)?,
            working_dir: crate::spawn::resolve_working_dir(
                container.working_dir.as_deref(),
                Some(worker_dir),
                &file.base_dir,
            ),
            config_name: container.config_name.clone(),
            environment: container.environment.keys().cloned().collect(),
            env_file: container.env_file.clone(),
            startup_timeout: container.startup_timeout,
        });
    }

    Ok(ValidationReport {
        project: file.name.clone().unwrap_or_else(|| namespace.to_string()),
        namespace: namespace.to_string(),
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

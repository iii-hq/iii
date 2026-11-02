// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! `iii.worker.yaml` subset parser and start-command resolution.
//!
//! Compose reads `name`, `scripts.start`, `runtime.base_image` and
//! `iii_engine_version`. The manifest is another tool's file, so unknown keys
//! are tolerated here — the opposite of the compose file's strictness.

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
    /// A worker started in a VM. The source decides which validation and
    /// workspace rules the sandbox adapter applies before it boots the VM.
    Vm(VmSpec),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VmSpec {
    /// An installed registry bundle. Its manifest command is publisher-owned
    /// and is always read and run inside the guest.
    Bundle {
        /// The bundle manifest's name, which for an alias is the package it
        /// stands for, not the container key.
        package: String,
        install_dir: PathBuf,
    },
    /// A local project whose manifest selected an OCI rootfs. The compose
    /// command may replace the manifest's `scripts.start`, but still runs in
    /// the guest.
    Local {
        worker_dir: PathBuf,
        run_override: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub struct Manifest {
    /// The worker's own name, when it is a string. Only the sandbox adapter
    /// lookup reads it: a container is still known by its compose key.
    pub name: Option<String>,
    pub start: Option<String>,
    pub base_image: Option<String>,
    /// `iii_engine_version`: the iii versions the worker runs on. `None` when
    /// the manifest predates the field, which reads as `>=0.25.0` and so
    /// excludes no engine that checks it.
    pub iii_engine_version: Option<semver::VersionReq>,
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
    let base_image = raw
        .runtime
        .as_ref()
        .and_then(serde_yaml::Value::as_mapping)
        .and_then(|runtime| runtime.get("base_image"))
        .map(|value| {
            value.as_str().ok_or_else(|| ComposeError::InvalidManifest {
                path: path.clone(),
                message: "`runtime.base_image` must be a string".to_string(),
            })
        })
        .transpose()?
        .map(str::trim)
        .filter(|image| !image.is_empty())
        .map(str::to_string);
    let iii_engine_version = raw
        .iii_engine_version
        .as_ref()
        .map(|value| parse_engine_requirement(value, &path))
        .transpose()?;

    Ok(Some(Manifest {
        name: raw
            .name
            .as_ref()
            .and_then(serde_yaml::Value::as_str)
            .map(str::to_string),
        start: raw.scripts.and_then(|scripts| scripts.start),
        base_image,
        iii_engine_version,
    }))
}

/// Resolves how a container starts.
///
/// The rule is general, not a decision per field: **where the compose file and
/// the manifest describe the same thing, `worker-compose.yaml` wins and the
/// manifest is the default.** The compose file is the operator's, the manifest
/// is the worker author's, and an operator deploying a worker they did not
/// write has to be able to override the author without editing a vendored
/// directory. So `run` wins over `scripts.start`, and the container key wins
/// over the manifest's `name` — the key is what reaches the child as
/// `III_WORKER_NAME`, so a worker honouring the reserved contract registers
/// under it whatever its manifest says.
///
/// A manifest that names itself differently used to be refused outright, which
/// rejected a configuration that works. What the manifest cannot predict — a
/// worker that hardcodes its name in code and ignores `III_WORKER_NAME` — is
/// caught at readiness by `WORKER_NAME_MISMATCH`, which reports the name it
/// actually took.
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
    if let Some(required) = manifest
        .as_ref()
        .and_then(|manifest| manifest.iii_engine_version.as_ref())
    {
        check_engine_version(
            key,
            &dir.join(MANIFEST_FILE),
            required,
            crate::host_version(),
        )?;
    }
    let run_override = container.scripts.run.clone();
    let Some(start) = run_override.clone().or_else(|| {
        manifest
            .as_ref()
            .and_then(|manifest| manifest.start.clone())
    }) else {
        return Err(ComposeError::MissingStartCommand {
            container: key.to_string(),
            manifest: dir.join(MANIFEST_FILE),
        });
    };

    if manifest
        .as_ref()
        .and_then(|manifest| manifest.base_image.as_ref())
        .is_some()
    {
        return Ok(StartSpec::Vm(VmSpec::Local {
            worker_dir: dir.clone(),
            run_override,
        }));
    }

    Ok(StartSpec::Shell(start))
}

/// Refuses a worker whose `iii_engine_version` the running iii does not
/// satisfy. `running` is [`crate::host_version`]: library callers and tests
/// record none, and a version that is not semver cannot be compared, so
/// neither refuses anything.
///
/// The running version's pre-release and build suffixes are dropped first. A
/// semver range only matches a pre-release of the release it names, so
/// `>=0.26.0` would refuse `0.27.0-alpha.1`, and every alpha would refuse the
/// workers its release is meant to run. The scaffolder's `min_iii_version`
/// compares the same way.
fn check_engine_version(
    key: &str,
    manifest: &Path,
    required: &semver::VersionReq,
    running: Option<&str>,
) -> Result<()> {
    let Some(running) = running else {
        return Ok(());
    };
    let Ok(version) = semver::Version::parse(running.trim().trim_start_matches('v')) else {
        return Ok(());
    };
    let release = semver::Version::new(version.major, version.minor, version.patch);
    if required.matches(&release) {
        return Ok(());
    }
    Err(ComposeError::UnsupportedEngineVersion {
        container: key.to_string(),
        manifest: manifest.to_path_buf(),
        required: required.to_string(),
        running: running.to_string(),
    })
}

/// Checks the `iii_engine_version` of the bundle installed in `install_dir`.
/// Only this key is parsed, so a bundle manifest that starts today cannot fail
/// here over a key Compose does not use: the adapter reads the rest in the
/// guest.
pub(crate) fn check_bundle_engine_version(key: &str, install_dir: &Path) -> Result<()> {
    bundle_engine_version(key, install_dir, crate::host_version())
}

fn bundle_engine_version(key: &str, install_dir: &Path, running: Option<&str>) -> Result<()> {
    let path = install_dir.join(MANIFEST_FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(source) => return Err(ComposeError::Io { path, source }),
    };
    let raw: RawEngineRequirement =
        serde_yaml::from_str(&text).map_err(|err| ComposeError::InvalidManifest {
            path: path.clone(),
            message: err.to_string(),
        })?;
    let Some(value) = raw.iii_engine_version else {
        return Ok(());
    };
    let required = parse_engine_requirement(&value, &path)?;
    check_engine_version(key, &path, &required, running)
}

fn parse_engine_requirement(value: &serde_yaml::Value, path: &Path) -> Result<semver::VersionReq> {
    let invalid = |message: String| ComposeError::InvalidManifest {
        path: path.to_path_buf(),
        message,
    };
    let range = value.as_str().ok_or_else(|| {
        invalid(
            "`iii_engine_version` must be a quoted semver range, for example \">=0.26.0\""
                .to_string(),
        )
    })?;
    semver::VersionReq::parse(range.trim()).map_err(|err| {
        invalid(format!(
            "`iii_engine_version` \"{range}\" is not a semver range: {err}"
        ))
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
    /// Namespace the project's workers would register under.
    ///
    /// There is no separate project name beside it. `namespace:` in the file
    /// is the only thing the project declares about itself, and a second field
    /// carrying the same string under another word is one that drifts.
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
    let mut config_names = std::collections::HashMap::new();
    for (key, container) in &file.containers {
        let name = container.resolved_config_name(namespace, key)?;
        if let Some((prior_key, prior_explicit)) =
            config_names.insert(name.clone(), (key, container.config_name.is_some()))
            && (!prior_explicit || container.config_name.is_none())
        {
            return Err(ComposeError::ConfigNameCollision {
                name,
                first: prior_key.clone(),
                second: key.clone(),
            });
        }
    }

    for key in &start_order {
        let Some(container) = file.containers.get(key) else {
            continue;
        };
        // Ahead of the package split on purpose: an env file is as checkable
        // for a `package://` container as for a `path://` one, since nothing
        // here needs the worker on disk. Behind it, every rule below would
        // silently exempt half the catalogue.
        check_env_files(key, container)?;

        let worker_dir = match &container.worker {
            WorkerSource::Package { .. } => {
                deferred_packages.push(key.clone());
                continue;
            }
            WorkerSource::Path { dir, .. } => dir,
        };

        resolved.push(ContainerPlan {
            key: key.clone(),
            start: resolve_start(key, container)?,
            working_dir: crate::spawn::resolve_working_dir(
                container.working_dir.as_deref(),
                Some(worker_dir),
                &file.base_dir,
            ),
            config_name: Some(container.resolved_config_name(namespace, key)?),
            environment: container.environment.keys().cloned().collect(),
            env_file: container.env_file.clone(),
            startup_timeout: container.startup_timeout,
        });
    }

    Ok(ValidationReport {
        namespace: namespace.to_string(),
        start_order,
        resolved,
        deferred_packages,
    })
}

/// Everything an env file can be judged on without an engine: that it is
/// there, and that it does not claim a name the daemon owns.
///
/// The rule this follows is general, and worth stating once rather than
/// deciding per field: **whatever holds for `environment` is evaluated for
/// `env_file` at the same stage.** `environment` is checked when the compose
/// file parses, so an `env_file` saying the same thing has to fail then too.
/// Anything checked only at spawn time is a rule `compose::validate` cannot
/// see, which makes it a rule a CI job reports as passing and `up` discovers
/// with half the graph already running.
///
/// Contents are read and dropped inside this function. `resolve_user_env`
/// keeps them out of the daemon's memory deliberately — env files hold secrets
/// — so validating them must not be what puts them back; the values do not
/// outlive this call.
fn check_env_files(key: &str, container: &Container) -> Result<()> {
    for env_file in &container.env_file {
        if !env_file.is_file() {
            return Err(ComposeError::MissingEnvFile {
                container: key.to_string(),
                path: env_file.clone(),
            });
        }

        let text = std::fs::read_to_string(env_file).map_err(|source| ComposeError::Io {
            path: env_file.clone(),
            source,
        })?;
        for (name, _) in crate::config::parse_env_file(&text) {
            if crate::spawn::is_retired_config_env(&name) {
                return Err(ComposeError::RetiredConfigEnv {
                    container: key.to_string(),
                    name,
                });
            }
            if crate::spawn::is_reserved_env(&name) {
                return Err(ComposeError::ReservedEnvOverride {
                    container: key.to_string(),
                    name,
                });
            }
        }
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct RawManifest {
    /// Kept as a value so a name that is not a string stays ignored, as every
    /// key compose does not need is.
    #[serde(default)]
    name: Option<serde_yaml::Value>,
    #[serde(default)]
    scripts: Option<RawManifestScripts>,
    /// Kept as a value so legacy scalar forms remain ignored. Compose only
    /// consults the `base_image` key when `runtime` is a mapping.
    #[serde(default)]
    runtime: Option<serde_yaml::Value>,
    /// Kept as a value so a range written as a number is reported, not lost.
    #[serde(default)]
    iii_engine_version: Option<serde_yaml::Value>,
}

/// The one key of a bundle's manifest Compose checks on the host.
#[derive(Debug, Deserialize)]
struct RawEngineRequirement {
    #[serde(default)]
    iii_engine_version: Option<serde_yaml::Value>,
}

#[derive(Debug, Deserialize)]
struct RawManifestScripts {
    #[serde(default)]
    start: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(range: &str, running: Option<&str>) -> Result<()> {
        let required = semver::VersionReq::parse(range).unwrap();
        check_engine_version("api", Path::new(MANIFEST_FILE), &required, running)
    }

    fn bundle(manifest: Option<&str>) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        if let Some(manifest) = manifest {
            std::fs::write(dir.path().join(MANIFEST_FILE), manifest).unwrap();
        }
        dir
    }

    #[test]
    fn a_running_release_inside_the_range_is_accepted() {
        assert!(check(">=0.26.0", Some("0.26.0")).is_ok());
        assert!(check(">=0.26.0", Some("0.27.3")).is_ok());
    }

    #[test]
    fn a_running_release_outside_the_range_is_refused_naming_both_versions() {
        let error = check(">=0.27.0", Some("0.26.4")).unwrap_err();
        assert_eq!(error.code(), "UNSUPPORTED_ENGINE_VERSION");
        let message = error.to_string();
        assert!(
            message.contains(">=0.27.0") && message.contains("0.26.4"),
            "{message}"
        );
    }

    #[test]
    fn a_pre_release_counts_as_the_release_it_precedes() {
        // Plain semver matching refuses a later alpha, which is why the
        // running version's suffix is dropped before matching.
        let later_alpha = semver::Version::parse("0.27.0-alpha.1").unwrap();
        assert!(
            !semver::VersionReq::parse(">=0.26.0")
                .unwrap()
                .matches(&later_alpha)
        );

        assert!(check(">=0.26.0", Some("0.27.0-alpha.1")).is_ok());
        assert!(check(">=0.26.0", Some("0.26.0-alpha.3")).is_ok());
        assert!(check(">=0.27.0", Some("0.26.9-alpha.1")).is_err());
    }

    #[test]
    fn without_a_comparable_running_version_nothing_is_refused() {
        assert!(check(">=99.0.0", None).is_ok());
        assert!(check(">=99.0.0", Some("dev")).is_ok());
    }

    #[test]
    fn a_bundle_is_checked_on_its_engine_version_alone() {
        // A list `scripts.start` would fail the full manifest parse.
        let dir = bundle(Some(
            "name: b\nscripts:\n  start: [node, index.js]\niii_engine_version: \">=0.27.0\"\n",
        ));
        let error = bundle_engine_version("b", dir.path(), Some("0.26.0")).unwrap_err();
        assert_eq!(error.code(), "UNSUPPORTED_ENGINE_VERSION");
        assert!(bundle_engine_version("b", dir.path(), Some("0.27.0")).is_ok());
    }

    #[test]
    fn a_bundle_without_the_field_or_the_manifest_is_accepted() {
        let without = bundle(Some("name: b\nscripts:\n  start: node index.js\n"));
        assert!(bundle_engine_version("b", without.path(), Some("0.26.0")).is_ok());
        let missing = bundle(None);
        assert!(bundle_engine_version("b", missing.path(), Some("0.26.0")).is_ok());
    }

    #[test]
    fn a_bundle_with_a_malformed_range_is_an_invalid_manifest() {
        let dir = bundle(Some("name: b\niii_engine_version: \"newest\"\n"));
        let error = bundle_engine_version("b", dir.path(), Some("0.26.0")).unwrap_err();
        assert_eq!(error.code(), "INVALID_MANIFEST");
    }
}

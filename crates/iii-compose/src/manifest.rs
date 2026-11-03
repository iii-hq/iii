// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! `iii.worker.yaml` subset parser and start-command resolution.
//!
//! Compose reads only `scripts.start` and `runtime.base_image`. The manifest is
//! another tool's file, so unknown keys are tolerated here — the opposite of
//! the compose file's strictness.

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
    /// The `iii_engine_version` range, already checked against the grammar.
    pub iii_engine_version: Option<String>,
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
    let iii_engine_version = engine_range(raw.iii_engine_version, &path)?;

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

/// Refuses a manifest whose `iii_engine_version` excludes `running`. Without a
/// known engine version (Compose outside an engine) nothing is checked.
fn check_engine(
    key: &str,
    manifest: Option<&Manifest>,
    running: Option<&semver::Version>,
) -> Result<()> {
    let (Some(range), Some(running)) = (
        manifest.and_then(|manifest| manifest.iii_engine_version.as_deref()),
        running,
    ) else {
        return Ok(());
    };
    crate::engine_compat::check_iii_engine_requirement(key, range, running).map_err(|message| {
        ComposeError::IncompatibleEngine {
            container: key.to_string(),
            message,
        }
    })
}

/// The `iii_engine_version` value as a range in the supported grammar. Anything else is
/// `INVALID_MANIFEST`, an unquoted `0.26` included: YAML reads it as a float.
fn engine_range(value: Option<serde_yaml::Value>, path: &Path) -> Result<Option<String>> {
    match value {
        None | Some(serde_yaml::Value::Null) => Ok(None),
        Some(serde_yaml::Value::String(range))
            if crate::engine_compat::valid_iii_engine_version(&range) =>
        {
            Ok(Some(range))
        }
        Some(other) => Err(ComposeError::InvalidManifest {
            path: path.to_path_buf(),
            message: crate::engine_compat::malformed_iii_engine_version(other),
        }),
    }
}

/// Refuses a registry bundle whose own `iii_engine_version` excludes the running engine.
/// The registry filters by `iii_version` when it resolves, but a bundle installed from
/// `worker-compose.lock` (a frozen start) is never resolved, and the engine may have changed
/// since the lock was written. [`crate::registry::install_resolved`] calls this for every
/// bundle, cache hits included.
pub(crate) fn check_bundle_engine(key: &str, install_dir: &Path) -> Result<()> {
    bundle_engine(
        key,
        install_dir,
        crate::engine_compat::running_engine_version().as_ref(),
    )
}

/// Reads only the range: a bundle whose manifest Compose does not otherwise parse installs
/// exactly as before, unless it declares a range this engine is outside of.
fn bundle_engine(key: &str, install_dir: &Path, running: Option<&semver::Version>) -> Result<()> {
    let Some(running) = running else {
        return Ok(());
    };
    let path = install_dir.join(MANIFEST_FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(source) => return Err(ComposeError::Io { path, source }),
    };
    let declared: BundleRange =
        serde_yaml::from_str(&text).map_err(|err| ComposeError::InvalidManifest {
            path: path.clone(),
            message: err.to_string(),
        })?;
    let Some(range) = engine_range(declared.iii_engine_version, &path)? else {
        return Ok(());
    };
    crate::engine_compat::check_iii_engine_requirement(key, &range, running).map_err(|message| {
        ComposeError::IncompatibleEngine {
            container: key.to_string(),
            message,
        }
    })
}

/// The one field of a bundle's manifest [`bundle_engine`] reads.
#[derive(Deserialize)]
struct BundleRange {
    #[serde(default)]
    iii_engine_version: Option<serde_yaml::Value>,
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
    check_engine(
        key,
        manifest.as_ref(),
        crate::engine_compat::running_engine_version().as_ref(),
    )?;
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
    /// Kept as a value so a non-string (an unquoted `0.26` reads as a float)
    /// gets the range-grammar message instead of a serde type error.
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

    fn manifest(yaml: &str) -> Result<Option<Manifest>> {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(MANIFEST_FILE), yaml).unwrap();
        read_manifest(dir.path())
    }

    #[test]
    fn reads_a_valid_iii_engine_version() {
        let m = manifest("name: http\niii_engine_version: \">=0.26.0\"\n")
            .unwrap()
            .unwrap();
        assert_eq!(m.iii_engine_version.as_deref(), Some(">=0.26.0"));
        let m = manifest("name: http\n").unwrap().unwrap();
        assert_eq!(m.iii_engine_version, None);
    }

    #[test]
    fn refuses_a_malformed_iii_engine_version() {
        for bad in ["\"0.26\"", "\"v0.26.0\"", "0.26", "\">=0.26.0-rc.1\""] {
            let err = manifest(&format!("name: http\niii_engine_version: {bad}\n")).unwrap_err();
            assert_eq!(err.code(), "INVALID_MANIFEST", "{bad}");
            assert!(
                err.to_string()
                    .contains("iii_engine_version must be a semver range"),
                "{err}"
            );
        }
    }

    #[test]
    fn check_engine_refuses_an_excluded_engine() {
        let m = manifest("iii_engine_version: \">=0.26.0\"\n").unwrap();
        let old = semver::Version::parse("0.25.9").unwrap();
        let err = check_engine("http", m.as_ref(), Some(&old)).unwrap_err();
        assert_eq!(err.code(), "INCOMPATIBLE_ENGINE");
        assert_eq!(
            err.to_string(),
            "container 'http': http requires iii >=0.26.0, running 0.25.9; run: iii update"
        );
        let rc = semver::Version::parse("0.26.0-rc.2").unwrap();
        assert!(check_engine("http", m.as_ref(), Some(&rc)).is_ok());
    }

    #[test]
    fn check_engine_skips_without_a_range_or_an_engine() {
        let old = semver::Version::parse("0.25.9").unwrap();
        let none = manifest("name: http\n").unwrap();
        assert!(check_engine("http", none.as_ref(), Some(&old)).is_ok());
        let ranged = manifest("iii_engine_version: \">=0.26.0\"\n").unwrap();
        assert!(check_engine("http", ranged.as_ref(), None).is_ok());
        assert!(check_engine("http", None, Some(&old)).is_ok());
    }

    fn bundle(yaml: Option<&str>) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        if let Some(yaml) = yaml {
            std::fs::write(dir.path().join(MANIFEST_FILE), yaml).unwrap();
        }
        dir
    }

    fn version(text: &str) -> semver::Version {
        semver::Version::parse(text).unwrap()
    }

    #[test]
    fn a_locked_bundle_outside_its_range_is_refused() {
        let dir = bundle(Some("name: onboarding\niii_engine_version: \">=0.27.0\"\n"));
        let err = bundle_engine("onboarding", dir.path(), Some(&version("0.26.1"))).unwrap_err();
        assert_eq!(err.code(), "INCOMPATIBLE_ENGINE");
        assert_eq!(
            err.to_string(),
            "container 'onboarding': onboarding requires iii >=0.27.0, running 0.26.1; run: iii update"
        );
        let older = bundle(Some("iii_engine_version: \"^0.26.0\"\n"));
        let err = bundle_engine("onboarding", older.path(), Some(&version("0.27.0"))).unwrap_err();
        assert!(
            err.to_string()
                .contains("this iii is newer than onboarding supports"),
            "{err}"
        );
    }

    #[test]
    fn a_bundle_inside_its_range_or_without_one_installs() {
        let running = version("0.26.0-rc.2");
        let ranged = bundle(Some("iii_engine_version: \">=0.26.0\"\n"));
        assert!(bundle_engine("b", ranged.path(), Some(&running)).is_ok());
        let unranged = bundle(Some("name: b\n"));
        assert!(bundle_engine("b", unranged.path(), Some(&running)).is_ok());
        let no_manifest = bundle(None);
        assert!(bundle_engine("b", no_manifest.path(), Some(&running)).is_ok());
        let excluded = bundle(Some("iii_engine_version: \">=0.27.0\"\n"));
        assert!(
            bundle_engine("b", excluded.path(), None).is_ok(),
            "outside an engine nothing is checked"
        );
    }

    #[test]
    fn a_bundle_is_read_for_its_range_only() {
        // `read_manifest` refuses a non-string base image; the install check never reads it.
        let dir = bundle(Some(
            "runtime:\n  base_image: 5\niii_engine_version: \">=0.26.0\"\n",
        ));
        assert!(read_manifest(dir.path()).is_err());
        assert!(bundle_engine("b", dir.path(), Some(&version("0.26.0"))).is_ok());
    }

    #[test]
    fn a_bundle_with_a_malformed_range_is_an_invalid_manifest() {
        let dir = bundle(Some("iii_engine_version: 0.26\n"));
        let err = bundle_engine("b", dir.path(), Some(&version("0.26.0"))).unwrap_err();
        assert_eq!(err.code(), "INVALID_MANIFEST");
    }
}

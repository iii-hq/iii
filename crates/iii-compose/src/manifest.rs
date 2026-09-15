// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! `iii.worker.yaml` subset parser and start-command resolution.
//!
//! Compose reads only the manifest fields needed to start local workers and
//! host bundles. The manifest is another tool's file, so unknown keys are
//! tolerated here — the opposite of the compose file's strictness. This parser
//! is deliberately independent from `crates/iii-worker`: Compose must not
//! inherit the legacy lifecycle system.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use serde::Deserialize;

use crate::{
    config::{ComposeFile, Container, WorkerSource},
    error::{ComposeError, Result},
};

const MAX_BUNDLE_MANIFEST_BYTES: u64 = 64 * 1024;
pub const MANIFEST_FILE: &str = "iii.worker.yaml";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartSpec {
    /// A shell command: the compose `run` or the manifest's `scripts.start`.
    Shell(String),
    /// A resolved package binary. Package resolution is not implemented yet, so
    /// nothing produces this variant today.
    Exec { program: PathBuf, args: Vec<String> },
    /// An installed registry bundle explicitly allowed to run on the host.
    /// Its private workspace is prepared before this spec reaches spawn.
    HostBundle(HostBundleSpec),
    /// A worker started in a VM. The source decides which validation and
    /// workspace rules `iii-worker` applies before it builds the boot command.
    Vm(VmSpec),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VmSpec {
    /// An installed registry bundle. Its manifest command is publisher-owned
    /// and is always read and run inside the guest.
    Bundle { install_dir: PathBuf },
    /// A local project whose manifest selected an OCI rootfs. The compose
    /// command may replace the manifest's `scripts.start`, but still runs in
    /// the guest.
    Local {
        worker_dir: PathBuf,
        run_override: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostBundleSpec {
    pub install_dir: PathBuf,
    pub install: Option<String>,
    pub run: String,
    pub env: BTreeMap<String, String>,
    pub has_base_image: bool,
    pub has_resources: bool,
}

#[derive(Debug, Clone)]
pub struct Manifest {
    pub start: Option<String>,
    pub base_image: Option<String>,
    pub install: Option<String>,
    pub env: BTreeMap<String, String>,
    pub has_resources: bool,
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

    let scripts = raw.scripts.unwrap_or_default();
    Ok(Some(Manifest {
        start: scripts.start,
        install: None,
        base_image,
        env: BTreeMap::new(),
        has_resources: false,
    }))
}

/// Reads the installed bundle fields needed by host execution while preserving
/// the same mandatory rails as the VM path.
pub fn read_host_bundle_manifest(dir: &Path, expected_name: &str) -> Result<Manifest> {
    let path = dir.join(MANIFEST_FILE);
    let metadata = std::fs::symlink_metadata(&path).map_err(|source| ComposeError::Io {
        path: path.clone(),
        source,
    })?;
    if !metadata.is_file() {
        return Err(ComposeError::InvalidManifest {
            path: path.clone(),
            message: "bundle manifest must be a regular file".to_string(),
        });
    }
    if metadata.len() > MAX_BUNDLE_MANIFEST_BYTES {
        return Err(ComposeError::InvalidManifest {
            path: path.clone(),
            message: format!(
                "bundle manifest is {} bytes; maximum is {MAX_BUNDLE_MANIFEST_BYTES}",
                metadata.len()
            ),
        });
    }
    let text = std::fs::read_to_string(&path).map_err(|source| ComposeError::Io {
        path: path.clone(),
        source,
    })?;
    let raw: RawHostManifest =
        serde_yaml::from_str(&text).map_err(|err| ComposeError::InvalidManifest {
            path: path.clone(),
            message: err.to_string(),
        })?;

    let name = raw
        .name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty());
    if name != Some(expected_name) {
        return Err(ComposeError::InvalidManifest {
            path: path.clone(),
            message: format!(
                "`name` must match the container key {expected_name:?}, got {}",
                name.map(|name| format!("{name:?}"))
                    .unwrap_or_else(|| "nothing".to_string())
            ),
        });
    }

    let scripts = raw.scripts.unwrap_or_default();
    if scripts
        .setup
        .as_deref()
        .map(str::trim)
        .is_some_and(|setup| !setup.is_empty())
    {
        return Err(ComposeError::InvalidManifest {
            path: path.clone(),
            message: "bundle manifests must not declare `scripts.setup`".to_string(),
        });
    }
    let start = scripts
        .start
        .map(|start| start.trim().to_string())
        .filter(|start| !start.is_empty())
        .ok_or_else(|| ComposeError::InvalidManifest {
            path: path.clone(),
            message: "bundle manifest must declare `scripts.start` as a non-empty string"
                .to_string(),
        })?;
    let install = scripts
        .install
        .map(|install| install.trim().to_string())
        .filter(|install| !install.is_empty());
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

    let mut env = raw.env.unwrap_or_default();
    env.retain(|name, _| !crate::spawn::RESERVED_ENV.contains(&name.as_str()));
    Ok(Manifest {
        start: Some(start),
        install,
        base_image,
        env,
        has_resources: raw.resources.is_some(),
    })
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
            config_name: container.config_name.clone(),
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
            if crate::spawn::RESERVED_ENV.contains(&name.as_str()) {
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
    #[serde(default)]
    scripts: Option<RawManifestScripts>,
    /// Kept as a value so legacy scalar forms remain ignored. Compose only
    /// consults the `base_image` key when `runtime` is a mapping.
    #[serde(default)]
    runtime: Option<serde_yaml::Value>,
}

#[derive(Debug, Default, Deserialize)]
struct RawManifestScripts {
    #[serde(default)]
    start: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawHostManifest {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    scripts: Option<RawHostManifestScripts>,
    #[serde(default)]
    runtime: Option<serde_yaml::Value>,
    #[serde(default)]
    env: Option<BTreeMap<String, String>>,
    #[serde(default)]
    resources: Option<serde_yaml::Value>,
}

#[derive(Debug, Default, Deserialize)]
struct RawHostManifestScripts {
    #[serde(default)]
    setup: Option<String>,
    #[serde(default)]
    install: Option<String>,
    #[serde(default)]
    start: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_bundle_manifest_reads_install_start_env_and_vm_only_options() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(MANIFEST_FILE),
            r#"
name: state
runtime:
  base_image: docker.io/iiidev/node:latest
scripts:
  install: npm install
  start: npm start
env:
  FROM_MANIFEST: yes
resources:
  cpus: 2
"#,
        )
        .unwrap();

        let manifest = read_host_bundle_manifest(dir.path(), "state").unwrap();
        assert_eq!(manifest.install.as_deref(), Some("npm install"));
        assert_eq!(manifest.start.as_deref(), Some("npm start"));
        assert_eq!(
            manifest.env.get("FROM_MANIFEST").map(String::as_str),
            Some("yes")
        );
        assert!(manifest.base_image.is_some());
        assert!(manifest.has_resources);
    }

    #[test]
    fn host_bundle_manifest_rejects_setup_and_wrong_name() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(MANIFEST_FILE),
            "name: other\nscripts:\n  setup: apt install curl\n  start: node index.js\n",
        )
        .unwrap();
        assert!(read_host_bundle_manifest(dir.path(), "state").is_err());

        std::fs::write(
            dir.path().join(MANIFEST_FILE),
            "name: state\nscripts:\n  setup: apt install curl\n  start: node index.js\n",
        )
        .unwrap();
        let error = read_host_bundle_manifest(dir.path(), "state").unwrap_err();
        assert!(error.to_string().contains("scripts.setup"));
    }

    #[test]
    fn host_bundle_manifest_limits_size_and_filters_reserved_environment() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(MANIFEST_FILE),
            "name: state\nscripts:\n  start: node index.js\nenv:\n  III_URL: ws://publisher\n  III_ISOLATION: publisher\n  ORDINARY: kept\n",
        )
        .unwrap();
        let manifest = read_host_bundle_manifest(dir.path(), "state").unwrap();
        assert_eq!(
            manifest.env.get("ORDINARY").map(String::as_str),
            Some("kept")
        );
        assert!(!manifest.env.contains_key("III_URL"));
        assert!(!manifest.env.contains_key("III_ISOLATION"));

        std::fs::write(
            dir.path().join(MANIFEST_FILE),
            vec![b'a'; MAX_BUNDLE_MANIFEST_BYTES as usize + 1],
        )
        .unwrap();
        let error = read_host_bundle_manifest(dir.path(), "state").unwrap_err();
        assert!(error.to_string().contains("maximum is 65536"));
    }

    #[test]
    fn local_manifest_keeps_ignoring_host_only_fields() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(MANIFEST_FILE),
            "name: [not, a, string]\nscripts:\n  install: {not: a string}\n  start: node index.js\nenv: [not, a, map]\nresources: invalid-but-ignored\n",
        )
        .unwrap();
        let manifest = read_manifest(dir.path()).unwrap().unwrap();
        assert_eq!(manifest.start.as_deref(), Some("node index.js"));
    }

    #[cfg(unix)]
    #[test]
    fn host_bundle_manifest_must_not_be_a_symlink() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(
            outside.path(),
            "name: state\nscripts:\n  start: node index.js\n",
        )
        .unwrap();
        symlink(outside.path(), dir.path().join(MANIFEST_FILE)).unwrap();
        let error = read_host_bundle_manifest(dir.path(), "state").unwrap_err();
        assert!(error.to_string().contains("regular file"));
    }
}

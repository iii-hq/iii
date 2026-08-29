// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Start-command resolution from compiled package descriptors.

use std::path::PathBuf;

use crate::{
    config::{ComposeFile, Container, WorkerSource},
    error::{ComposeError, Result},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartSpec {
    /// A shell command: the compose `run` or the manifest's `scripts.start`.
    Shell(String),
    /// A resolved package binary. Package resolution is not implemented yet, so
    /// nothing produces this variant today.
    Exec { program: PathBuf, args: Vec<String> },
    /// A digest-pinned OCI image executed by the host OCI runtime.
    Oci { image: String, exec: Vec<String> },
    /// A worker started in a VM. The source decides which validation and
    /// workspace rules `iii-worker` applies before it builds the boot command.
    Vm(VmSpec),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VmSpec {
    /// An installed registry bundle. Its manifest command is publisher-owned
    /// and is always read and run inside the guest.
    Bundle {
        install_dir: PathBuf,
        exec: Vec<String>,
        base_image: Option<String>,
    },
    /// A local project whose manifest selected an OCI rootfs. The compose
    /// command may replace the manifest's `scripts.start`, but still runs in
    /// the guest.
    Local {
        worker_dir: PathBuf,
        exec: Vec<String>,
        base_image: String,
    },
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
    let (dir, descriptor) = match &container.worker {
        // A package has no start command until its artefact is installed, and
        // installing needs the network. `lifecycle::start_one` does that and
        // builds the `Exec` itself; nothing else should reach here.
        WorkerSource::Package { .. } => {
            return Err(ComposeError::PackageNotInstalled {
                container: key.to_string(),
            });
        }
        WorkerSource::Path {
            dir, descriptor, ..
        } => (dir, descriptor),
    };

    if !dir.is_dir() {
        return Err(ComposeError::MissingWorkerDirectory {
            container: key.to_string(),
            path: dir.clone(),
        });
    }

    let Some(exec) = descriptor.runtime.exec.clone() else {
        return Err(ComposeError::MissingStartCommand {
            container: key.to_string(),
            manifest: container
                .worker_dir()
                .unwrap_or(dir)
                .join("worker-compose.yaml"),
        });
    };
    let Some((program, args)) = exec.split_first() else {
        return Err(ComposeError::MissingStartCommand {
            container: key.to_string(),
            manifest: dir.join("worker-compose.yaml"),
        });
    };

    if let Some(base_image) = descriptor.runtime.base_image.clone() {
        return Ok(StartSpec::Vm(VmSpec::Local {
            worker_dir: dir.clone(),
            exec,
            base_image,
        }));
    }

    Ok(StartSpec::Exec {
        program: PathBuf::from(program),
        args: args.to_vec(),
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

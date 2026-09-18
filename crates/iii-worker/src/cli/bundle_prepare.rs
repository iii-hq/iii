// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Build a compose worker VM and print how to start it.
//!
//! The caller is compose, and it reaches this over a process boundary rather
//! than by linking. That is not indirection for its own sake: libkrun needs
//! glibc, and the engine ships a musl build by default so it installs on any
//! Linux. Linked, those two requirements do not fit in one binary — the musl
//! target does not compile at all. Split, each keeps what it needs, which is
//! the same arrangement the installer already makes by shipping `iii-worker`
//! as its own asset with its own platform rules.
//!
//! What crosses is a recipe, not a live object: a program, its arguments, and
//! the environment to start it with. Preparing a VM produces exactly that, so
//! the boundary costs a serialisation and nothing else.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// What compose asks for, on stdin.
#[derive(Debug, Deserialize)]
pub struct Request {
    /// The container key, which is also `III_WORKER_NAME`, not the package name.
    pub worker_name: String,
    /// Canonical identity from Compose's resolved registry metadata. Only bundles
    /// use it. Legacy callers omit it and retain worker-name matching.
    #[serde(default)]
    pub package_name: Option<String>,
    /// The bundle install or local worker directory. Older bundle callers used
    /// `install_dir`, which remains accepted across binary version skew.
    #[serde(alias = "install_dir")]
    pub worker_dir: PathBuf,
    /// Per-caller VM state. Keyed by the caller, not by worker name, so two
    /// projects using the same container key do not share a rootfs.
    pub state_dir: PathBuf,
    /// Where the guest reaches the engine.
    pub engine_url: String,
    /// Merged over the manifest's env, under the reserved keys the VM owns.
    #[serde(default)]
    pub extra_env: BTreeMap<String, String>,
    /// Published into the guest at `/run/iii/config`.
    #[serde(default)]
    pub config_dir: Option<PathBuf>,
    /// Compose's `scripts.run` override. Only local workers use it.
    #[serde(default)]
    pub run_override: Option<String>,
}

impl Request {
    fn expected_package_name(&self) -> &str {
        self.package_name.as_deref().unwrap_or(&self.worker_name)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum PrepareKind {
    /// Apply immutable registry-bundle validation and resource limits.
    Bundle,
    /// Apply local project validation and workspace behavior.
    Local,
}

/// How to start it, on stdout.
///
/// Spawning is the caller's: it supervises its own children, and a VM it did
/// not spawn is one it cannot stop or watch exit.
#[derive(Debug, Serialize, Deserialize)]
pub struct Plan {
    /// Confirms which package was validated. New Compose callers require this
    /// acknowledgement so old helpers cannot silently ignore `package_name`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_name: Option<String>,
    pub program: PathBuf,
    pub args: Vec<String>,
    /// Set on the boot process itself — not the guest env, which is already
    /// inside `args`. This is how the VM finds its firmware.
    pub env: BTreeMap<String, String>,
    /// Cleared on the boot process. A lifeline inherited from whoever spawned
    /// compose would tie the VM to the wrong process.
    pub env_remove: Vec<String>,
    /// Where the VM records its pid.
    pub pid_file: PathBuf,
}

/// Where a caller's config directory appears inside the guest.
pub use super::local_worker::GUEST_CONFIG_DIR;

/// Reads a [`Request`] from stdin and prints a [`Plan`], or the reason there is
/// none. Exit code 0 with a plan, 1 with a message on stderr.
pub async fn run(kind: PrepareKind) -> i32 {
    let mut input = String::new();
    if let Err(err) = std::io::Read::read_to_string(&mut std::io::stdin(), &mut input) {
        eprintln!("VM prepare: cannot read the request: {err}");
        return 1;
    }

    let request: Request = match serde_json::from_str(&input) {
        Ok(request) => request,
        Err(err) => {
            eprintln!("VM prepare: the request is not valid JSON: {err}");
            return 1;
        }
    };

    let expected_package_name = request.expected_package_name().to_string();
    let Request {
        worker_name,
        package_name: _,
        worker_dir,
        state_dir,
        engine_url,
        extra_env,
        config_dir,
        run_override,
    } = request;

    let over = super::local_worker::VmOverride {
        state_dir,
        engine_url: &engine_url,
        extra_env: extra_env.into_iter().collect(),
        config_dir,
    };

    let built = match kind {
        PrepareKind::Bundle => {
            super::local_worker::bundle_vm_command_for_package(
                &worker_name,
                &expected_package_name,
                &worker_dir,
                over,
            )
            .await
        }
        PrepareKind::Local => {
            super::local_worker::local_vm_command(
                &worker_name,
                &worker_dir,
                run_override.as_deref(),
                over,
            )
            .await
        }
    };
    let built = match built {
        Ok(built) => built,
        Err(err) => {
            eprintln!("{err}");
            return 1;
        }
    };

    let mut plan = describe(&built);
    if matches!(kind, PrepareKind::Bundle) {
        plan.package_name = Some(expected_package_name);
    }
    match serde_json::to_string(&plan) {
        Ok(json) => {
            println!("{json}");
            0
        }
        Err(err) => {
            eprintln!("VM prepare: cannot serialise the plan: {err}");
            1
        }
    }
}

/// Turns the built command into the recipe that crosses the boundary.
fn describe(built: &super::worker_manager::libkrun::VmCommand) -> Plan {
    let command = built.command.as_std();
    Plan {
        package_name: None,
        program: PathBuf::from(command.get_program()),
        args: command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect(),
        env: command
            .get_envs()
            .filter_map(|(key, value)| {
                value.map(|value| {
                    (
                        key.to_string_lossy().into_owned(),
                        value.to_string_lossy().into_owned(),
                    )
                })
            })
            .collect(),
        env_remove: command
            .get_envs()
            .filter(|(_, value)| value.is_none())
            .map(|(key, _)| key.to_string_lossy().into_owned())
            .collect(),
        pid_file: built.pid_file.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_preflight_accepts_aliases_and_preserves_guest_instance_identity() {
        let source = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let yaml = "name: math-worker\nscripts:\n  start: node index.js\nenv:\n  III_WORKER_NAME: publisher-name\n";
        std::fs::write(source.path().join("iii.worker.yaml"), yaml).unwrap();
        for key in ["math", "math-a", "math-b"] {
            let request: Request = serde_json::from_value(serde_json::json!({
                "worker_name": key,
                "package_name": "math-worker",
                "worker_dir": source.path(),
                "state_dir": state.path().join(key),
                "engine_url": "ws://localhost:49134",
                "extra_env": {"III_WORKER_NAME": key}
            }))
            .unwrap();
            super::super::local_worker::validate_start_manifest(
                &request.worker_dir,
                Some(request.expected_package_name()),
            )
            .expect("the package, not the instance key, must match");
            let project = super::super::project::load_project_info(&request.worker_dir).unwrap();
            let mut env = project.env;
            env.extend(request.extra_env);
            let env = super::super::local_worker::build_local_env(
                &request.engine_url,
                &request.worker_name,
                &env,
            );
            assert_eq!(env["III_WORKER_NAME"], key);
            assert_eq!(request.state_dir, state.path().join(key));
        }
        assert_eq!(
            std::fs::read_to_string(source.path().join("iii.worker.yaml")).unwrap(),
            yaml
        );
    }

    #[test]
    fn bundle_preflight_rejects_a_different_package_even_when_manifest_matches_alias() {
        let source = tempfile::tempdir().unwrap();
        for manifest_name in ["other-worker", "math"] {
            std::fs::write(
                source.path().join("iii.worker.yaml"),
                format!("name: {manifest_name}\nscripts:\n  start: node index.js\n"),
            )
            .unwrap();
            let request: Request = serde_json::from_value(serde_json::json!({
                "worker_name": "math", "package_name": "math-worker",
                "worker_dir": source.path(), "state_dir": "/tmp/math-state",
                "engine_url": "ws://localhost:49134"
            }))
            .unwrap();
            let error = super::super::local_worker::validate_start_manifest(
                &request.worker_dir,
                Some(request.expected_package_name()),
            )
            .unwrap_err();
            assert!(error.contains("math-worker"), "{error}");
            assert!(error.contains(manifest_name), "{error}");
        }
    }

    #[test]
    fn legacy_bundle_preflight_keeps_worker_name_matching() {
        let source = tempfile::tempdir().unwrap();
        std::fs::write(
            source.path().join("iii.worker.yaml"),
            "name: math-worker\nscripts:\n  start: node index.js\n",
        )
        .unwrap();
        for key in ["math-worker", "math"] {
            let request: Request = serde_json::from_value(serde_json::json!({
                "worker_name": key, "install_dir": source.path(),
                "state_dir": "/tmp/math-state", "engine_url": "ws://localhost:49134"
            }))
            .unwrap();
            let result = super::super::local_worker::validate_start_manifest(
                &request.worker_dir,
                Some(request.expected_package_name()),
            );
            assert_eq!(result.is_ok(), key == "math-worker");
        }
        // Local worker start still permits a manifest name different from its key.
        assert!(super::super::local_worker::validate_start_manifest(source.path(), None).is_ok());
    }

    #[test]
    fn request_accepts_the_legacy_bundle_install_dir_field() {
        let request: Request = serde_json::from_value(serde_json::json!({
            "worker_name": "probe",
            "install_dir": "/tmp/probe",
            "state_dir": "/tmp/state",
            "engine_url": "ws://localhost:49134"
        }))
        .expect("the previous bundle request must remain compatible");

        assert_eq!(request.worker_dir, PathBuf::from("/tmp/probe"));
        assert!(request.package_name.is_none());
        assert_eq!(request.expected_package_name(), "probe");
    }

    #[test]
    fn request_reads_the_local_run_override() {
        let request: Request = serde_json::from_value(serde_json::json!({
            "worker_name": "probe",
            "worker_dir": "/tmp/probe",
            "state_dir": "/tmp/state",
            "engine_url": "ws://localhost:49134",
            "run_override": "python src/dev.py"
        }))
        .expect("the local request should parse");

        assert_eq!(request.run_override.as_deref(), Some("python src/dev.py"));
    }
}

// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! `iii-worker __bundle-prepare`: build a bundle's VM, print how to start it.
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
    /// The container key, which is also `III_WORKER_NAME` and the name the
    /// bundle's manifest is checked against.
    pub worker_name: String,
    /// The extracted bundle: holds `iii.worker.yaml`.
    pub install_dir: PathBuf,
    /// Per-caller VM state. Keyed by the caller, not by worker name, so two
    /// projects running the same bundle do not share a rootfs.
    pub state_dir: PathBuf,
    /// Where the guest reaches the engine.
    pub engine_url: String,
    /// Merged over the manifest's env, under the reserved keys the VM owns.
    #[serde(default)]
    pub extra_env: BTreeMap<String, String>,
    /// Published into the guest at `/run/iii/config`.
    #[serde(default)]
    pub config_dir: Option<PathBuf>,
}

/// How to start it, on stdout.
///
/// Spawning is the caller's: it supervises its own children, and a VM it did
/// not spawn is one it cannot stop or watch exit.
#[derive(Debug, Serialize, Deserialize)]
pub struct Plan {
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
pub async fn run() -> i32 {
    let mut input = String::new();
    if let Err(err) = std::io::Read::read_to_string(&mut std::io::stdin(), &mut input) {
        eprintln!("__bundle-prepare: cannot read the request: {err}");
        return 1;
    }

    let request: Request = match serde_json::from_str(&input) {
        Ok(request) => request,
        Err(err) => {
            eprintln!("__bundle-prepare: the request is not valid JSON: {err}");
            return 1;
        }
    };

    let over = super::local_worker::VmOverride {
        state_dir: request.state_dir,
        engine_url: &request.engine_url,
        extra_env: request.extra_env.into_iter().collect(),
        config_dir: request.config_dir,
    };

    let built = match super::local_worker::bundle_vm_command(
        &request.worker_name,
        &request.install_dir,
        over,
    )
    .await
    {
        Ok(built) => built,
        Err(err) => {
            eprintln!("{err}");
            return 1;
        }
    };

    let plan = describe(&built);
    match serde_json::to_string(&plan) {
        Ok(json) => {
            println!("{json}");
            0
        }
        Err(err) => {
            eprintln!("__bundle-prepare: cannot serialise the plan: {err}");
            1
        }
    }
}

/// Turns the built command into the recipe that crosses the boundary.
fn describe(built: &super::worker_manager::libkrun::VmCommand) -> Plan {
    let command = built.command.as_std();
    Plan {
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

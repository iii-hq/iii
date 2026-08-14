// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! The child spawn contract.
//!
//! A child's environment is **built**, not inherited. The daemon composes it
//! from three layers — a host baseline, the container's declared
//! `env_file`/`environment`, and four reserved variables the daemon owns — and
//! the child is spawned with that map and nothing else. A compose project then
//! starts the same way regardless of which shell launched the daemon, and a
//! stale `III_URL` in the operator's environment can never point a child at the
//! wrong engine.
//!
//! The plan is computed as data ([`SpawnPlan`]) and only then turned into a
//! process, so the contract is assertable without spawning anything.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use crate::manifest::StartSpec;

/// Environment variables the daemon owns for every child.
///
/// Not because static configuration outranks an environment variable, which
/// would be the wrong way round for most settings. Because each of these five
/// is already declared in the compose file, and a second declaration of the
/// same thing is a disagreement nobody resolves. Each earns its place
/// separately, so adding a sixth is a decision, not a habit:
///
/// - `III_URL` is the daemon's own connection. Readiness is observed over it,
///   so a container pointed at another engine is invisible to the daemon that
///   started it and fails as a startup timeout over a worker that is running
///   and serving. Two engines mean two daemons.
/// - `III_NAMESPACE` and `III_WORKER_NAME` are the pair readiness watches.
///   An override would have to be threaded through readiness, the `ChildRecord`
///   and `compose::status` before it could work at all; short of that, compose
///   waits in one place while the child registers in another. Both are already
///   declared, by `namespace:` and by the container key.
/// - `III_CONFIG` and `III_CONFIG_NAME` are two halves of one delivery: the
///   merged value is written to the file and published to the entry. Pointing
///   the child at a different file leaves it reading one value while the
///   configuration worker holds another.
pub const RESERVED_ENV: [&str; 5] = [
    "III_URL",
    "III_NAMESPACE",
    "III_CONFIG",
    "III_CONFIG_NAME",
    "III_WORKER_NAME",
];

/// Host variables a child inherits. Everything else in the daemon's environment
/// is dropped: a compose project must start the same way whatever shell the
/// operator happened to launch the daemon from. Anything a worker actually
/// needs is declared in `environment` or `env_file`.
#[cfg(unix)]
pub const BASELINE_ENV: &[&str] = &[
    "PATH", "HOME", "USER", "LOGNAME", "SHELL", "TERM", "TMPDIR", "TZ", "LANG", "LC_ALL",
];

/// The windows baseline is longer because the platform genuinely fails without
/// it: no SystemRoot means no ws2_32, no COMSPEC means no `cmd /C`.
#[cfg(windows)]
pub const BASELINE_ENV: &[&str] = &[
    "PATH",
    "PATHEXT",
    "COMSPEC",
    "SystemRoot",
    "SystemDrive",
    "windir",
    "TEMP",
    "TMP",
    "USERPROFILE",
    "HOMEDRIVE",
    "HOMEPATH",
    "APPDATA",
    "LOCALAPPDATA",
    "PROCESSOR_ARCHITECTURE",
    "NUMBER_OF_PROCESSORS",
    "OS",
];

/// Cloneable so hooks can reuse a container's context with a different command.
#[derive(Debug, Clone)]
pub struct SpawnCtx<'a> {
    pub engine_url: &'a str,
    pub namespace: &'a str,
    pub container_key: &'a str,
    pub start: &'a StartSpec,
    /// Path of the resolved configuration file, when the container has config.
    pub config_path: Option<&'a Path>,
    /// Which configuration entry this container's value was written to, and
    /// therefore the one it should read from.
    ///
    /// A worker owns an id and hardcodes it, which makes the id a global
    /// scarce name: two projects each running `state` share one entry and
    /// overwrite each other. Telling the worker its id instead lets one
    /// project call it `state-finance` and another `state-hr`.
    pub config_name: Option<&'a str>,
    pub working_dir: &'a Path,
    /// Already-merged `env_file` + `environment` for this container. Reserved
    /// keys are rejected before they get here.
    pub user_env: &'a BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnPlan {
    pub program: String,
    pub args: Vec<String>,
    /// The child's complete environment. The daemon's own environment is not
    /// inherited, so this map is the whole story.
    pub env: BTreeMap<String, String>,
    pub working_dir: PathBuf,
}

/// Builds the spawn plan for one container.
///
/// Precedence, lowest to highest: host baseline, then the container's
/// `env_file`/`environment`, then the reserved contract. A user value can never
/// win over a reserved key — those are rejected at parse time rather than
/// silently dropped here.
pub fn spawn_plan(ctx: &SpawnCtx<'_>) -> SpawnPlan {
    let mut env: BTreeMap<String, String> = BASELINE_ENV
        .iter()
        .filter_map(|name| {
            std::env::var(name)
                .ok()
                .map(|value| (name.to_string(), value))
        })
        .collect();

    env.extend(ctx.user_env.clone());

    env.insert("III_URL".to_string(), ctx.engine_url.to_string());
    env.insert("III_NAMESPACE".to_string(), ctx.namespace.to_string());
    env.insert("III_WORKER_NAME".to_string(), ctx.container_key.to_string());
    match ctx.config_path {
        Some(config_path) => {
            env.insert(
                "III_CONFIG".to_string(),
                config_path.to_string_lossy().to_string(),
            );
        }
        // No config for this container: the key must be absent, not stale.
        None => {
            env.remove("III_CONFIG");
        }
    }
    match ctx.config_name {
        Some(name) => {
            env.insert("III_CONFIG_NAME".to_string(), name.to_string());
        }
        None => {
            env.remove("III_CONFIG_NAME");
        }
    }

    let (program, args) = match ctx.start {
        StartSpec::Shell(command) => shell_invocation(command),
        StartSpec::Exec { program, args } => (program.to_string_lossy().to_string(), args.clone()),
        // The host execs nothing for a VM container: the start command is the
        // bundle's own and runs inside the guest. Only the environment and the
        // working directory computed above carry over.
        StartSpec::Vm { .. } => (String::new(), Vec::new()),
    };

    SpawnPlan {
        program,
        args,
        env,
        working_dir: ctx.working_dir.to_path_buf(),
    }
}

#[cfg(unix)]
fn shell_invocation(command: &str) -> (String, Vec<String>) {
    (
        "sh".to_string(),
        vec!["-c".to_string(), command.to_string()],
    )
}

#[cfg(windows)]
fn shell_invocation(command: &str) -> (String, Vec<String>) {
    (
        "cmd".to_string(),
        vec!["/C".to_string(), command.to_string()],
    )
}

impl SpawnPlan {
    /// Turns the plan into a runnable command. Process-group placement and exit
    /// watching are the supervisor's job and are not applied here.
    ///
    /// `None` when the container runs in a VM. There is no host program then,
    /// and `env` is not this command's environment either: it belongs to the
    /// guest, and reaches it as boot arguments rather than through the boot
    /// process, which keeps the daemon's own environment to find its firmware.
    pub fn command(&self) -> Option<tokio::process::Command> {
        if self.program.is_empty() {
            return None;
        }
        let mut command = tokio::process::Command::new(&self.program);
        command.args(&self.args).current_dir(&self.working_dir);
        // Clear first: the plan is the child's entire environment, so nothing
        // from the daemon's shell can leak in behind it.
        command.env_clear();
        for (name, value) in &self.env {
            command.env(name, value);
        }
        Some(command)
    }
}

/// Directory a container runs in: its explicit `working_dir`, else the worker's
/// own directory, else the compose file's directory.
pub fn resolve_working_dir(
    declared: Option<&Path>,
    worker_dir: Option<&Path>,
    compose_dir: &Path,
) -> PathBuf {
    declared.or(worker_dir).unwrap_or(compose_dir).to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx<'a>(
        start: &'a StartSpec,
        config: Option<&'a Path>,
        user_env: &'a BTreeMap<String, String>,
    ) -> SpawnCtx<'a> {
        SpawnCtx {
            engine_url: "ws://127.0.0.1:49134",
            namespace: "orders-1234abcd",
            container_key: "api",
            start,
            config_path: config,
            config_name: None,
            working_dir: Path::new("/srv/app/workers/api"),
            user_env,
        }
    }

    fn env_of(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect()
    }

    #[test]
    fn injects_the_full_contract() {
        let start = StartSpec::Shell("cargo run".to_string());
        let user_env = BTreeMap::new();
        let plan = spawn_plan(&ctx(&start, None, &user_env));

        assert_eq!(plan.env["III_URL"], "ws://127.0.0.1:49134");
        assert_eq!(plan.env["III_NAMESPACE"], "orders-1234abcd");
        assert_eq!(plan.env["III_WORKER_NAME"], "api");
        assert!(!plan.env.contains_key("III_CONFIG"));
        assert_eq!(plan.working_dir, PathBuf::from("/srv/app/workers/api"));
    }

    #[test]
    fn config_path_becomes_iii_config() {
        let start = StartSpec::Shell("cargo run".to_string());
        let config = PathBuf::from("/run/iii/compose/api.yaml");
        let user_env = BTreeMap::new();
        let plan = spawn_plan(&ctx(&start, Some(&config), &user_env));

        assert_eq!(plan.env["III_CONFIG"], "/run/iii/compose/api.yaml");
    }

    #[test]
    fn the_daemon_environment_is_not_inherited_wholesale() {
        // Only the baseline crosses over; a variable the operator happened to
        // export must not silently become part of the project's contract.
        unsafe { std::env::set_var("COMPOSE_TEST_STRAY", "leaked") };
        let start = StartSpec::Shell("cargo run".to_string());
        let user_env = BTreeMap::new();
        let plan = spawn_plan(&ctx(&start, None, &user_env));
        unsafe { std::env::remove_var("COMPOSE_TEST_STRAY") };

        assert!(!plan.env.contains_key("COMPOSE_TEST_STRAY"));
        // PATH is in the baseline, so a child can still find its interpreter.
        assert!(plan.env.contains_key("PATH"), "baseline should carry PATH");
    }

    #[test]
    fn user_env_sits_above_the_baseline_and_below_the_contract() {
        let start = StartSpec::Shell("cargo run".to_string());
        let user_env = env_of(&[("PATH", "/only/this"), ("RUST_LOG", "debug")]);
        let plan = spawn_plan(&ctx(&start, None, &user_env));

        assert_eq!(
            plan.env["PATH"], "/only/this",
            "user env overrides baseline"
        );
        assert_eq!(plan.env["RUST_LOG"], "debug");
        assert_eq!(plan.env["III_NAMESPACE"], "orders-1234abcd");
    }

    #[test]
    fn shell_start_goes_through_a_shell() {
        let start = StartSpec::Shell("npm start".to_string());
        let user_env = BTreeMap::new();
        let plan = spawn_plan(&ctx(&start, None, &user_env));

        #[cfg(unix)]
        assert_eq!((plan.program.as_str(), plan.args[0].as_str()), ("sh", "-c"));
        #[cfg(windows)]
        assert_eq!(
            (plan.program.as_str(), plan.args[0].as_str()),
            ("cmd", "/C")
        );
        assert_eq!(plan.args[1], "npm start");
    }

    #[test]
    fn working_dir_precedence() {
        let declared = PathBuf::from("/srv/custom");
        let worker = PathBuf::from("/srv/app/workers/api");
        let compose = PathBuf::from("/srv/app");

        assert_eq!(
            resolve_working_dir(Some(&declared), Some(&worker), &compose),
            declared
        );
        assert_eq!(resolve_working_dir(None, Some(&worker), &compose), worker);
        assert_eq!(resolve_working_dir(None, None, &compose), compose);
    }
}

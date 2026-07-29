// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! The child spawn contract.
//!
//! The daemon and its children agree on four environment variables. They are
//! reserved: whatever the operator's shell had is stripped, so a stale
//! `III_URL` in the parent environment can never point a child at the wrong
//! engine.
//!
//! The plan is computed as data ([`SpawnPlan`]) and only then turned into a
//! process, so the contract is assertable without spawning anything.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use crate::manifest::StartSpec;

/// Environment variables the daemon owns for every child.
pub const RESERVED_ENV: [&str; 4] = ["III_URL", "III_NAMESPACE", "III_CONFIG", "III_WORKER_NAME"];

/// Cloneable so hooks can reuse a container's context with a different command.
#[derive(Debug, Clone)]
pub struct SpawnCtx<'a> {
    pub engine_url: &'a str,
    pub namespace: &'a str,
    pub container_key: &'a str,
    pub start: &'a StartSpec,
    /// Path of the resolved configuration file, when the container has config.
    pub config_path: Option<&'a Path>,
    pub working_dir: &'a Path,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnPlan {
    pub program: String,
    pub args: Vec<String>,
    /// Variables the daemon sets. Sorted for stable assertions and logs.
    pub env: BTreeMap<String, String>,
    /// Reserved variables the child must not inherit from the daemon.
    pub cleared_env: Vec<String>,
    pub working_dir: PathBuf,
}

/// Builds the spawn plan for one container.
pub fn spawn_plan(ctx: &SpawnCtx<'_>) -> SpawnPlan {
    let mut env = BTreeMap::new();
    env.insert("III_URL".to_string(), ctx.engine_url.to_string());
    env.insert("III_NAMESPACE".to_string(), ctx.namespace.to_string());
    env.insert("III_WORKER_NAME".to_string(), ctx.container_key.to_string());
    if let Some(config_path) = ctx.config_path {
        env.insert(
            "III_CONFIG".to_string(),
            config_path.to_string_lossy().to_string(),
        );
    }

    // Reserved variables the daemon did not set must be actively removed: an
    // inherited value would silently win over the contract.
    let cleared_env = RESERVED_ENV
        .iter()
        .filter(|name| !env.contains_key(**name))
        .map(|name| name.to_string())
        .collect();

    let (program, args) = match ctx.start {
        StartSpec::Shell(command) => shell_invocation(command),
        StartSpec::Exec { program, args } => (program.to_string_lossy().to_string(), args.clone()),
    };

    SpawnPlan {
        program,
        args,
        env,
        cleared_env,
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
    pub fn command(&self) -> tokio::process::Command {
        let mut command = tokio::process::Command::new(&self.program);
        command.args(&self.args).current_dir(&self.working_dir);
        for name in &self.cleared_env {
            command.env_remove(name);
        }
        for (name, value) in &self.env {
            command.env(name, value);
        }
        command
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

    fn ctx<'a>(start: &'a StartSpec, config: Option<&'a Path>) -> SpawnCtx<'a> {
        SpawnCtx {
            engine_url: "ws://127.0.0.1:49134",
            namespace: "orders-1234abcd",
            container_key: "api",
            start,
            config_path: config,
            working_dir: Path::new("/srv/app/workers/api"),
        }
    }

    #[test]
    fn injects_the_full_contract_and_clears_unset_reserved_vars() {
        let start = StartSpec::Shell("cargo run".to_string());
        let plan = spawn_plan(&ctx(&start, None));

        assert_eq!(plan.env["III_URL"], "ws://127.0.0.1:49134");
        assert_eq!(plan.env["III_NAMESPACE"], "orders-1234abcd");
        assert_eq!(plan.env["III_WORKER_NAME"], "api");
        assert!(!plan.env.contains_key("III_CONFIG"));
        assert_eq!(plan.cleared_env, vec!["III_CONFIG".to_string()]);
        assert_eq!(plan.working_dir, PathBuf::from("/srv/app/workers/api"));
    }

    #[test]
    fn config_path_becomes_iii_config() {
        let start = StartSpec::Shell("cargo run".to_string());
        let config = PathBuf::from("/run/iii/compose/api.yaml");
        let plan = spawn_plan(&ctx(&start, Some(&config)));

        assert_eq!(plan.env["III_CONFIG"], "/run/iii/compose/api.yaml");
        assert!(plan.cleared_env.is_empty());
    }

    #[test]
    fn shell_start_goes_through_a_shell() {
        let start = StartSpec::Shell("npm start".to_string());
        let plan = spawn_plan(&ctx(&start, None));

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

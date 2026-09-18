// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! The child spawn contract.
//!
//! A child's environment starts with the machine environment visible to the
//! daemon. The container's `env_file`/`environment` values override it, followed
//! by the reserved variables the daemon owns. The resulting map is the entire
//! child environment, so a stale `III_URL` cannot point it at another engine.
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
/// would be the wrong way round for most settings. Because each of these eight
/// is already declared in the compose file, and a second declaration of the
/// same thing is a disagreement nobody resolves. Each earns its place
/// separately, so adding a ninth is a decision, not a habit:
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
/// - `III_COMPOSE_NAMESPACE`, `III_COMPOSE_FILE`, and `III_COMPOSE_DIR` let a
///   managed worker reach the daemon and project that started it, and resolve
///   project-owned paths. The daemon namespace is not the project namespace,
///   and one daemon may own several compose files, so the namespace and file
///   are required for an unambiguous control-plane call. The directory is the
///   canonical parent of that file.
/// - `III_CONFIG` and `III_CONFIG_NAME` are two halves of one delivery: the
///   merged value is written to the file and published to the entry. Pointing
///   the child at a different file leaves it reading one value while the
///   configuration worker holds another.
pub const RESERVED_ENV: [&str; 8] = [
    "III_URL",
    "III_NAMESPACE",
    "III_COMPOSE_NAMESPACE",
    "III_COMPOSE_FILE",
    "III_COMPOSE_DIR",
    "III_CONFIG",
    "III_CONFIG_NAME",
    "III_WORKER_NAME",
];

/// Whether a key claims a daemon-owned variable under the host OS's rules.
pub(crate) fn is_reserved_env(name: &str) -> bool {
    #[cfg(windows)]
    {
        RESERVED_ENV.iter().any(|key| windows_env_key_eq(name, key))
    }
    #[cfg(not(windows))]
    {
        RESERVED_ENV.contains(&name)
    }
}

/// Cloneable so hooks can reuse a container's context with a different command.
#[derive(Debug, Clone)]
pub struct SpawnCtx<'a> {
    pub engine_url: &'a str,
    pub namespace: &'a str,
    pub compose_namespace: &'a str,
    pub compose_file: &'a Path,
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
    /// The child's complete environment, including the machine environment
    /// captured when the plan was built.
    pub env: BTreeMap<String, String>,
    pub working_dir: PathBuf,
}

/// The device id `iii project init` wrote beside the compose file. The same
/// value the engine reports telemetry under, so a worker's own events land on
/// the machine that produced them rather than on an identity of their own.
///
/// Its own small parse rather than a shared one: `iii-compose` does not depend
/// on the engine crate that owns telemetry, and this is four lines of INI.
const HOST_USER_ID_ENV: &str = "III_HOST_USER_ID";

/// Identify the project-scoped telemetry key using the host's naming rules.
fn is_host_user_id(name: &str) -> bool {
    #[cfg(windows)]
    {
        windows_env_key_eq(name, HOST_USER_ID_ENV)
    }
    #[cfg(not(windows))]
    {
        name == HOST_USER_ID_ENV
    }
}

fn project_device_id(compose_dir: &Path) -> Option<String> {
    let contents = std::fs::read_to_string(compose_dir.join(".iii").join("project.ini")).ok()?;
    contents.lines().find_map(|line| {
        line.trim()
            .strip_prefix("device_id=")
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

/// Builds the spawn plan for one container.
///
/// Precedence, lowest to highest: machine environment, then the container's
/// `env_file`/`environment`, then the reserved contract. A user value can never
/// win over a reserved key: those are rejected before the plan is built.
pub fn spawn_plan(ctx: &SpawnCtx<'_>) -> SpawnPlan {
    // Plans also carry environment values into VMs, whose protocol uses UTF-8.
    // Ignore non-Unicode entries instead of panicking as std::env::vars would.
    let env = std::env::vars_os()
        .filter_map(|(name, value)| Some((name.into_string().ok()?, value.into_string().ok()?)))
        .collect();
    spawn_plan_with_env(ctx, env)
}

/// Match the OS's ordinal case folding, including non-ASCII environment names.
#[cfg(windows)]
pub(crate) fn windows_env_key_eq(left: &str, right: &str) -> bool {
    use windows_sys::Win32::Globalization::{CSTR_EQUAL, CompareStringOrdinal};

    let left: Vec<u16> = left.encode_utf16().collect();
    let right: Vec<u16> = right.encode_utf16().collect();
    // Windows folds individual UTF-16 code units without changing their count.
    if left.len() != right.len() {
        return false;
    }
    if left.is_empty() {
        return true;
    }
    let len = i32::try_from(left.len()).expect("environment key exceeds Windows API length limit");
    // SAFETY: Both pointers reference initialized UTF-16 buffers with `len`
    // elements, and both buffers remain alive for the duration of the call.
    let result = unsafe { CompareStringOrdinal(left.as_ptr(), len, right.as_ptr(), len, 1) };
    assert_ne!(
        result,
        0,
        "comparing environment keys failed: {}",
        std::io::Error::last_os_error()
    );
    result == CSTR_EQUAL
}

fn spawn_plan_with_env(ctx: &SpawnCtx<'_>, mut env: BTreeMap<String, String>) -> SpawnPlan {
    // Telemetry identity belongs to this project, not the daemon's parent.
    // Explicit container values are applied below and may still override it.
    env.retain(|name, _| !is_host_user_id(name));
    // Windows environment names are case-insensitive. Remove host spellings
    // before overlaying explicit values, rather than relying on map sort order.
    #[cfg(windows)]
    env.retain(|name, _| {
        !is_reserved_env(name) && !ctx.user_env.keys().any(|key| windows_env_key_eq(name, key))
    });

    env.extend(ctx.user_env.clone());

    env.insert("III_URL".to_string(), ctx.engine_url.to_string());
    env.insert("III_NAMESPACE".to_string(), ctx.namespace.to_string());
    env.insert(
        "III_COMPOSE_NAMESPACE".to_string(),
        ctx.compose_namespace.to_string(),
    );
    let compose_file = ctx
        .compose_file
        .canonicalize()
        .or_else(|_| std::path::absolute(ctx.compose_file))
        .unwrap_or_else(|_| ctx.compose_file.to_path_buf());
    env.insert(
        "III_COMPOSE_FILE".to_string(),
        compose_file.to_string_lossy().to_string(),
    );
    let compose_dir = compose_file
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    env.insert(
        "III_COMPOSE_DIR".to_string(),
        compose_dir.to_string_lossy().to_string(),
    );
    // The project's device id, for workers that report telemetry of their own.
    // Not reserved: `environment:` still wins, which is what a test or a CI run
    // wants. A project with no `.iii/project.ini` — one not scaffolded by
    // `iii project init` — simply has no id to publish, so the variable is
    // absent and a worker must treat it as optional.
    if !env.keys().any(|name| is_host_user_id(name))
        && let Some(device_id) = project_device_id(compose_dir)
    {
        env.insert(HOST_USER_ID_ENV.to_string(), device_id);
    }
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
        // The host execs nothing for a VM container: the start command runs
        // inside the guest. Only the environment and the working directory
        // computed above carry over.
        StartSpec::Vm(_) => (String::new(), Vec::new()),
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
            compose_namespace: "compose-host",
            compose_file: Path::new("/srv/app/worker-compose.yaml"),
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

    fn normalize_path(path: &Path) -> PathBuf {
        path.canonicalize()
            .or_else(|_| std::path::absolute(path))
            .unwrap_or_else(|_| path.to_path_buf())
    }

    #[test]
    fn injects_the_full_contract() {
        let start = StartSpec::Shell("cargo run".to_string());
        let user_env = BTreeMap::new();
        let plan = spawn_plan(&ctx(&start, None, &user_env));
        let expected_compose_file = normalize_path(Path::new("/srv/app/worker-compose.yaml"));
        let expected_compose_dir = expected_compose_file
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));

        assert_eq!(plan.env["III_URL"], "ws://127.0.0.1:49134");
        assert_eq!(plan.env["III_NAMESPACE"], "orders-1234abcd");
        assert_eq!(plan.env["III_COMPOSE_NAMESPACE"], "compose-host");
        assert_eq!(
            Path::new(&plan.env["III_COMPOSE_FILE"]),
            expected_compose_file.as_path()
        );
        assert_eq!(
            Path::new(&plan.env["III_COMPOSE_DIR"]),
            expected_compose_dir
        );
        assert_eq!(plan.env["III_WORKER_NAME"], "api");
        assert!(!plan.env.contains_key("III_CONFIG"));
        assert_eq!(plan.working_dir, PathBuf::from("/srv/app/workers/api"));
    }

    #[test]
    fn relative_compose_file_contract_is_canonical() {
        let canonical_cwd = std::env::current_dir().unwrap().canonicalize().unwrap();
        let temp = tempfile::tempdir_in(&canonical_cwd).unwrap();
        let absolute_file = temp.path().join("worker-compose.yaml");
        std::fs::write(&absolute_file, "containers: {}").unwrap();
        let relative_file = absolute_file.strip_prefix(&canonical_cwd).unwrap();
        let expected_file = absolute_file.canonicalize().unwrap();
        let expected_dir = expected_file.parent().unwrap();

        let start = StartSpec::Shell("cargo run".to_string());
        let user_env = BTreeMap::new();
        let mut context = ctx(&start, None, &user_env);
        context.compose_file = relative_file;
        let plan = spawn_plan(&context);

        assert_eq!(Path::new(&plan.env["III_COMPOSE_FILE"]), expected_file);
        assert_eq!(Path::new(&plan.env["III_COMPOSE_DIR"]), expected_dir);
    }

    /// Machine identity is never inherited; only the project or an explicit
    /// container value may provide it, including native aliases on Windows.
    #[test]
    fn host_user_id_comes_from_the_project_file_and_yields_to_the_container() {
        #[cfg(windows)]
        let names = [HOST_USER_ID_ENV, "iii_host_user_id"];
        #[cfg(not(windows))]
        let names = [HOST_USER_ID_ENV];
        for machine_key in names {
            let temp = tempfile::tempdir().unwrap();
            let compose_file = temp.path().join("worker-compose.yaml");
            std::fs::write(&compose_file, "containers: {}").unwrap();
            let start = StartSpec::Shell("cargo run".to_string());
            let user_env = BTreeMap::new();
            let mut context = ctx(&start, None, &user_env);
            context.compose_file = &compose_file;
            let machine = env_of(&[(machine_key, "other-project")]);

            let plan = spawn_plan_with_env(&context, machine.clone());
            assert!(!plan.env.keys().any(|name| is_host_user_id(name)));

            std::fs::create_dir_all(temp.path().join(".iii")).unwrap();
            std::fs::write(
                temp.path().join(".iii/project.ini"),
                "[project]\nproject_id=p-1\ndevice_id=device-abc\n",
            )
            .unwrap();
            let plan = spawn_plan_with_env(&context, machine.clone());
            assert_eq!(plan.env[HOST_USER_ID_ENV], "device-abc");
            assert_eq!(
                plan.env.keys().filter(|name| is_host_user_id(name)).count(),
                1
            );

            // Explicit overrides, even empty ones, prevent the project fallback.
            for declared_key in names {
                for value in ["from-compose", ""] {
                    let declared = env_of(&[(declared_key, value)]);
                    let mut context = ctx(&start, None, &declared);
                    context.compose_file = &compose_file;
                    let plan = spawn_plan_with_env(&context, machine.clone());
                    assert_eq!(plan.env[declared_key], value);
                    assert_eq!(
                        plan.env.keys().filter(|name| is_host_user_id(name)).count(),
                        1
                    );
                }
            }
        }
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
    fn machine_environment_is_inherited_without_container_overrides() {
        let start = StartSpec::Shell("cargo run".to_string());
        let user_env = BTreeMap::new();
        let plan = spawn_plan_with_env(
            &ctx(&start, None, &user_env),
            env_of(&[("COMPOSE_TEST_MACHINE", "from-machine")]),
        );

        assert_eq!(plan.env["COMPOSE_TEST_MACHINE"], "from-machine");
    }

    #[test]
    fn user_env_sits_above_the_machine_environment_and_below_the_contract() {
        let start = StartSpec::Shell("cargo run".to_string());
        let user_env = env_of(&[("PATH", "/only/this"), ("RUST_LOG", "debug")]);
        let plan = spawn_plan(&ctx(&start, None, &user_env));

        assert_eq!(
            plan.env["PATH"], "/only/this",
            "user env overrides the machine environment"
        );
        assert_eq!(plan.env["RUST_LOG"], "debug");
        assert_eq!(plan.env["III_NAMESPACE"], "orders-1234abcd");
    }

    #[test]
    fn machine_environment_is_overridden_by_env_files_then_environment() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("base.env"),
            "FROM_FILE=file\nOVERRIDE=base\n",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("last.env"),
            "OVERRIDE=last\nFROM_COMPOSE=file\n",
        )
        .unwrap();
        let file = crate::ComposeFile::parse(
            "containers:\n  api:\n    worker: path://./api\n    env_file: [base.env, last.env]\n    environment:\n      FROM_COMPOSE: compose\n      OPTIONAL: ''\n",
            tmp.path().join("worker-compose.yaml"),
        ).unwrap();
        let user_env = file.containers["api"].resolve_user_env("api").unwrap();
        let start = StartSpec::Shell("cargo run".to_string());
        let plan = spawn_plan_with_env(
            &ctx(&start, None, &user_env),
            env_of(&[
                ("ONLY_MACHINE", "machine"),
                ("FROM_FILE", "machine"),
                ("OVERRIDE", "machine"),
                ("FROM_COMPOSE", "machine"),
                ("OPTIONAL", "machine"),
            ]),
        );

        assert_eq!(plan.env["ONLY_MACHINE"], "machine");
        assert_eq!(plan.env["FROM_FILE"], "file");
        assert_eq!(plan.env["OVERRIDE"], "last");
        assert_eq!(plan.env["FROM_COMPOSE"], "compose");
        assert_eq!(plan.env["OPTIONAL"], "");
    }

    #[test]
    fn reserved_values_replace_the_machine_environment_and_absent_config_is_removed() {
        let start = StartSpec::Shell("cargo run".to_string());
        let user_env = BTreeMap::new();
        let context = ctx(&start, None, &user_env);
        let host = RESERVED_ENV
            .iter()
            .map(|name| (name.to_string(), "stale".to_string()))
            .collect();
        let plan = spawn_plan_with_env(&context, host);
        let expected = spawn_plan_with_env(&context, BTreeMap::new());

        assert_eq!(plan.env, expected.env);
        assert!(!plan.env.contains_key("III_CONFIG"));
        assert!(!plan.env.contains_key("III_CONFIG_NAME"));
    }

    #[cfg(windows)]
    #[test]
    fn windows_host_names_do_not_bypass_explicit_or_reserved_values() {
        let start = StartSpec::Shell("echo ready".to_string());
        let user_env = env_of(&[("TOKEN", "compose")]);
        let plan = spawn_plan_with_env(
            &ctx(&start, None, &user_env),
            env_of(&[
                ("token", "machine"),
                ("iii_url", "stale"),
                ("iii_config", "stale"),
            ]),
        );

        assert_eq!(plan.env["TOKEN"], "compose");
        assert_eq!(plan.env["III_URL"], "ws://127.0.0.1:49134");
        assert!(!plan.env.contains_key("token"));
        assert!(!plan.env.contains_key("iii_url"));
        assert!(!plan.env.contains_key("iii_config"));
    }

    #[cfg(windows)]
    #[test]
    fn windows_unicode_host_names_yield_to_explicit_values() {
        let start = StartSpec::Shell("echo ready".to_string());
        for (host_key, explicit_key) in [("föo", "FÖO"), ("FÖO", "föo")] {
            let user_env = env_of(&[(explicit_key, "compose")]);
            let plan = spawn_plan_with_env(
                &ctx(&start, None, &user_env),
                env_of(&[(host_key, "machine")]),
            );

            assert_eq!(plan.env[explicit_key], "compose");
            assert!(!plan.env.contains_key(host_key));
            let command = plan.command().unwrap();
            let values: Vec<_> = command
                .as_std()
                .get_envs()
                .filter(|(key, _)| windows_env_key_eq(key.to_str().unwrap(), explicit_key))
                .map(|(_, value)| value.unwrap().to_str().unwrap())
                .collect();
            assert_eq!(values, ["compose"]);
        }
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

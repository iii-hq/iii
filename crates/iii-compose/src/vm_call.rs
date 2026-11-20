// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! What a VM container's scripts get from Compose (tech-specs/2026-11-19-iii-call-sandbox,
//! section 4.2).
//!
//! A VM container's `scripts.build` is `iii call sandbox::build worker=<container>` and its
//! `scripts.run` is `iii call sandbox::start worker=<container>`. Everything else those calls
//! need comes from Compose: `sandbox::layer_own_vm_env` lays the VM's context over the
//! container's variables, and once the process environment is final, [`lay_call_defaults`]
//! turns it into the `sandbox::build` and `sandbox::start` entries of `III_CALL_DEFAULTS`, which
//! `iii call` merges under the line's own arguments. The token and the worker's variables never
//! reach the process's arguments.

use std::{collections::BTreeMap, path::Path, time::Duration};

use iii_sandbox_contract::{SandboxBuildRequest, SandboxStartRequest, WorkerSource};
use serde_json::{Map, Value};

/// The scoped `worker::*` token of the container's own VM.
pub const TOKEN_ENV: &str = "III_SANDBOX_TOKEN";
/// The VM's state key: the same key boots the same persistent state.
pub const STATE_KEY_ENV: &str = "III_SANDBOX_STATE_KEY";
/// The adapter's name, e.g. `sandbox-libkrun`.
pub const ADAPTER_ENV: &str = "III_SANDBOX_ADAPTER";
/// The namespace the adapter's functions are registered in.
pub const ADAPTER_NAMESPACE_ENV: &str = "III_SANDBOX_NAMESPACE";
/// A `path://` worker's directory, synced into the VM.
pub const WORKER_DIR_ENV: &str = "III_WORKER_DIR";
/// A registry package's directory, `.iii/workers/<container>`.
pub const PACKAGE_DIR_ENV: &str = "III_PACKAGE_DIR";
/// A registry package's manifest name, which for an alias is the package it stands for.
pub const PACKAGE_NAME_ENV: &str = "III_PACKAGE_NAME";
/// A package container's package kind, `bundle` or `binary`: a binary in a VM boots as
/// `WorkerSource::Binary`.
pub const PACKAGE_KIND_ENV: &str = "III_PACKAGE_KIND";
/// The container's build directory, `<project>/.iii/build/<container>`.
pub const BUILD_DIR_ENV: &str = "III_BUILD_DIR";
/// The compose file's canonical path, the same as `III_COMPOSE_FILE`.
pub const BUILD_COMPOSE_FILE_ENV: &str = "III_BUILD_COMPOSE_FILE";
/// `scripts.build_timeout` in milliseconds.
pub const BUILD_TIMEOUT_ENV: &str = "III_BUILD_TIMEOUT_MS";
/// The container's already-expanded image override from the Compose plan; empty means none.
pub const BUILD_BASE_IMAGE_ENV: &str = "III_BUILD_BASE_IMAGE";
/// The daemon's resolved worker URI, never re-expanded in the container's environment.
pub(crate) const BUILD_WORKER_ENV: &str = "III_BUILD_WORKER";
/// Comma-separated names of the variables that belong to the worker and reach the guest.
pub const WORKER_ENV_KEYS_ENV: &str = "III_WORKER_ENV_KEYS";
/// How long Compose waits after its SIGTERM before it kills the process.
pub const STOP_TIMEOUT_ENV: &str = "III_STOP_TIMEOUT_MS";
/// The default payloads `iii call` takes by function id.
pub const CALL_DEFAULTS_ENV: &str = "III_CALL_DEFAULTS";
/// The source a build started from was replaced before its `build.json` was written: nothing is
/// recorded, and building again fixes it.
pub const BUILD_INPUT_CHANGED: &str = "BUILD_INPUT_CHANGED";

const URL_ENV: &str = "III_URL";
const WORKER_NAME_ENV: &str = "III_WORKER_NAME";
const PROJECT_TOKEN_ENV: &str = "III_SANDBOX_WORKER_TOKEN";
/// A stop's budget when Compose names none.
const DEFAULT_STOP_GRACE: Duration = Duration::from_secs(10);

/// Never forwarded into the guest, even when listed: Compose's own inputs, the host's engine URL
/// (the adapter gives the guest its own) and the project token.
const NEVER_FORWARDED: [&str; 18] = [
    TOKEN_ENV,
    STATE_KEY_ENV,
    ADAPTER_ENV,
    ADAPTER_NAMESPACE_ENV,
    WORKER_DIR_ENV,
    PACKAGE_DIR_ENV,
    PACKAGE_NAME_ENV,
    PACKAGE_KIND_ENV,
    BUILD_DIR_ENV,
    BUILD_COMPOSE_FILE_ENV,
    BUILD_TIMEOUT_ENV,
    BUILD_BASE_IMAGE_ENV,
    BUILD_WORKER_ENV,
    WORKER_ENV_KEYS_ENV,
    STOP_TIMEOUT_ENV,
    URL_ENV,
    PROJECT_TOKEN_ENV,
    CALL_DEFAULTS_ENV,
];

/// The variables `III_CALL_DEFAULTS` replaces in the process's environment.
const REPLACED: [&str; 11] = [
    TOKEN_ENV,
    STATE_KEY_ENV,
    ADAPTER_ENV,
    ADAPTER_NAMESPACE_ENV,
    WORKER_DIR_ENV,
    WORKER_ENV_KEYS_ENV,
    STOP_TIMEOUT_ENV,
    BUILD_TIMEOUT_ENV,
    BUILD_BASE_IMAGE_ENV,
    BUILD_WORKER_ENV,
    PACKAGE_KIND_ENV,
];

fn present<'a>(vars: &'a BTreeMap<String, String>, name: &str) -> Option<&'a str> {
    vars.get(name)
        .map(String::as_str)
        .filter(|value| !value.is_empty())
}

fn required(vars: &BTreeMap<String, String>, name: &str) -> Result<String, String> {
    present(vars, name)
        .map(str::to_string)
        .ok_or_else(|| format!("{name} is not set"))
}

fn millis(vars: &BTreeMap<String, String>, name: &str) -> Result<Option<u64>, String> {
    present(vars, name)
        .map(|ms| {
            ms.parse()
                .map_err(|_| format!("{name} must be a number of milliseconds, not {ms:?}"))
        })
        .transpose()
}

/// The variables `III_WORKER_ENV_KEYS` names, without [`NEVER_FORWARDED`]: the guest's whole
/// environment.
pub(crate) fn worker_env(vars: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    present(vars, WORKER_ENV_KEYS_ENV)
        .into_iter()
        .flat_map(|keys| keys.split(','))
        .map(str::trim)
        .filter(|key| !key.is_empty() && !NEVER_FORWARDED.contains(key))
        .filter_map(|key| Some((key.to_string(), vars.get(key)?.clone())))
        .collect()
}

/// What the VM boots: a `path://` worker's directory, or a package's, as a bundle or a binary.
fn worker_source(vars: &BTreeMap<String, String>) -> Result<WorkerSource, String> {
    if let Some(dir) = present(vars, WORKER_DIR_ENV) {
        return Ok(WorkerSource::Local {
            host_path: dir.to_string(),
            run_override: None,
        });
    }
    let Some(dir) = present(vars, PACKAGE_DIR_ENV) else {
        return Err(format!(
            "neither {WORKER_DIR_ENV} nor {PACKAGE_DIR_ENV} is set, so there is no worker to boot"
        ));
    };
    let package = required(vars, PACKAGE_NAME_ENV)?;
    if present(vars, PACKAGE_KIND_ENV) == Some("binary") {
        let worker_name = required(vars, WORKER_NAME_ENV)?;
        return crate::vm_binary::worker_source(&worker_name, &package, Path::new(dir))
            .map_err(|error| error.to_string());
    }
    Ok(WorkerSource::Bundle {
        package,
        host_path: dir.to_string(),
    })
}

/// The image the container's last build wrote, as its `build.json` records it; `None` without
/// one, and `sandbox::start` is then left out: Compose builds a container before it runs it.
fn built_image(build_dir: &str) -> Option<String> {
    crate::build_record::read(Path::new(build_dir))
        .ok()
        .flatten()
        .and_then(|record| record.image)
        .map(|image| image.build_id)
}

/// The `sandbox::build` and `sandbox::start` entries of `III_CALL_DEFAULTS`, from the variables
/// `sandbox::layer_own_vm_env` laid in the final environment `vars`.
pub(crate) fn call_defaults(vars: &BTreeMap<String, String>) -> Result<Value, String> {
    let worker = required(vars, WORKER_NAME_ENV)?;
    let adapter = required(vars, ADAPTER_ENV)?;
    let namespace = required(vars, ADAPTER_NAMESPACE_ENV)?;
    let auth = required(vars, TOKEN_ENV)?;
    let state_key = required(vars, STATE_KEY_ENV)?;
    let build_dir = required(vars, BUILD_DIR_ENV)?;
    let source = worker_source(vars)?;
    let env = worker_env(vars);
    let stop_grace =
        millis(vars, STOP_TIMEOUT_ENV)?.map_or(DEFAULT_STOP_GRACE, Duration::from_millis) * 9 / 10;

    let mut defaults = Map::new();
    let build = SandboxBuildRequest {
        worker: worker.clone(),
        adapter: adapter.clone(),
        namespace: namespace.clone(),
        auth: auth.clone(),
        state_key: state_key.clone(),
        source: source.clone(),
        env: env.clone(),
        build_dir: build_dir.clone(),
        base_image: present(vars, BUILD_BASE_IMAGE_ENV).map(str::to_string),
        timeout_ms: Some(
            millis(vars, BUILD_TIMEOUT_ENV)?
                .unwrap_or(crate::config::DEFAULT_BUILD_TIMEOUT.as_millis() as u64),
        ),
        stdout: None,
        stderr: None,
    };
    defaults.insert(
        iii_sandbox_contract::SANDBOX_BUILD.to_string(),
        serde_json::to_value(build).map_err(|error| error.to_string())?,
    );
    if let Some(build_id) = built_image(&build_dir) {
        let start = SandboxStartRequest {
            worker,
            adapter,
            namespace,
            auth,
            state_key,
            source,
            env,
            build_dir,
            build_id,
            engine_url: required(vars, URL_ENV)?,
            stop_grace_ms: Some(stop_grace.as_millis() as u64),
            command: None,
            stdout: None,
            stderr: None,
        };
        defaults.insert(
            iii_sandbox_contract::SANDBOX_START.to_string(),
            serde_json::to_value(start).map_err(|error| error.to_string())?,
        );
    }
    Ok(Value::Object(defaults))
}

/// Turns the VM context `sandbox::layer_own_vm_env` laid in the final environment `env` into
/// `III_CALL_DEFAULTS`, and removes the variables it replaces. An environment without that context
/// (any container that does not run in a VM) is left as it is. A context that does not read is
/// logged and left out: the call then names what it lacks.
pub(crate) fn lay_call_defaults(env: &mut BTreeMap<String, String>) {
    if !env.contains_key(TOKEN_ENV) {
        return;
    }
    match call_defaults(env) {
        Ok(defaults) => {
            env.insert(CALL_DEFAULTS_ENV.to_string(), defaults.to_string());
        }
        Err(error) => {
            let worker = env.get(WORKER_NAME_ENV).cloned().unwrap_or_default();
            crate::report::daemon_line(
                &format!("{worker}: the VM context for iii call is incomplete: {error}"),
                true,
            );
        }
    }
    for name in REPLACED {
        env.remove(name);
    }
}

/// The `code` and `message` of the `Error:` line `iii call` prints for a failed call, on one line
/// when its stderr is not a terminal, as it is under Compose.
pub(crate) fn call_failure(line: &str) -> Option<(String, String)> {
    let error: Value = serde_json::from_str(line.trim().strip_prefix("Error: ")?).ok()?;
    let code = error.get("code")?.as_str()?.to_string();
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    Some((code, message))
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn compose_vars() -> BTreeMap<String, String> {
        BTreeMap::from(
            [
                (TOKEN_ENV, "scoped"),
                (STATE_KEY_ENV, "state"),
                (ADAPTER_ENV, "sandbox-libkrun"),
                (ADAPTER_NAMESPACE_ENV, "shop"),
                (WORKER_DIR_ENV, "/work/api"),
                (WORKER_NAME_ENV, "api"),
                (URL_ENV, "ws://127.0.0.1:49134"),
                (BUILD_DIR_ENV, "/work/.iii/build/api"),
                (BUILD_TIMEOUT_ENV, "60000"),
                (STOP_TIMEOUT_ENV, "10000"),
                (
                    WORKER_ENV_KEYS_ENV,
                    "KEY,III_WORKER_NAME,III_URL,III_SANDBOX_TOKEN",
                ),
                ("KEY", "secret"),
                ("HOST_ONLY", "x"),
            ]
            .map(|(name, value)| (name.to_string(), value.to_string())),
        )
    }

    #[test]
    fn the_defaults_carry_the_vm_and_only_the_workers_variables() {
        let defaults = call_defaults(&compose_vars()).unwrap();
        let build = &defaults["sandbox::build"];
        assert_eq!(build["worker"], "api");
        assert_eq!(build["namespace"], "shop");
        assert_eq!(build["auth"], "scoped");
        assert_eq!(build["timeout_ms"], 60000);
        assert_eq!(build["source"]["kind"], "local");
        assert_eq!(build["source"]["host_path"], "/work/api");
        assert_eq!(
            build["env"],
            serde_json::json!({ "KEY": "secret", "III_WORKER_NAME": "api" }),
            "neither the host's URL, the token nor a host variable reaches the guest"
        );
        assert!(
            defaults.get("sandbox::start").is_none(),
            "no build yet, so nothing to start"
        );
    }

    #[test]
    fn start_is_there_once_a_build_is_recorded() {
        let dir = tempfile::tempdir().unwrap();
        let build_dir = dir.path().join("api");
        std::fs::create_dir_all(&build_dir).unwrap();
        std::fs::write(
            build_dir.join("build.json"),
            serde_json::json!({
                "schema": 1, "container": "api",
                "source": { "kind": "path", "dir": "./api" },
                "command": "iii call sandbox::build worker=api",
                "dependency_files": {},
                "image": { "file": "image.erofs", "build_id": "sha256:1",
                    "base_image": { "declared": null, "reference": "python", "digest": null },
                    "adapter": { "name": "sandbox-libkrun", "version": "0.5.0" },
                    "contract": { "major": 1, "minor": 4 } },
                "built_at": "2026-11-19T00:00:00Z"
            })
            .to_string(),
        )
        .unwrap();
        let mut vars = compose_vars();
        vars.insert(
            BUILD_DIR_ENV.to_string(),
            build_dir.to_string_lossy().into_owned(),
        );
        let defaults = call_defaults(&vars).unwrap();
        let start = &defaults["sandbox::start"];
        assert_eq!(start["build_id"], "sha256:1");
        assert_eq!(start["engine_url"], "ws://127.0.0.1:49134");
        assert_eq!(start["stop_grace_ms"], 9000);
    }

    #[test]
    fn the_defaults_replace_the_vm_variables_in_the_environment() {
        let mut env = compose_vars();
        lay_call_defaults(&mut env);
        assert!(env.contains_key(CALL_DEFAULTS_ENV));
        for name in REPLACED {
            assert!(!env.contains_key(name), "{name}");
        }
        assert_eq!(env["KEY"], "secret");
        assert_eq!(env[BUILD_DIR_ENV], "/work/.iii/build/api");

        let mut host = BTreeMap::from([("KEY".to_string(), "v".to_string())]);
        lay_call_defaults(&mut host);
        assert!(!host.contains_key(CALL_DEFAULTS_ENV));
    }

    #[test]
    fn a_failed_call_is_read_from_its_error_line() {
        assert_eq!(
            call_failure(
                r#"Error: {"code":"S507","message":"setup exited with 1","stacktrace":null}"#
            ),
            Some(("S507".to_string(), "setup exited with 1".to_string()))
        );
        assert_eq!(call_failure("npm ERR! code 1"), None);
        assert_eq!(call_failure("Error: not json"), None);
    }
}

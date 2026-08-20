// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! `up` and `down`.
//!
//! Three rules shape everything here:
//!
//! 1. **A container is up only when the engine has seen it.** Spawning is not
//!    starting: readiness is the child appearing under `(namespace, container)`
//!    in the engine, which is the same fact its dependents rely on.
//! 2. **Rollback undoes this operation, not the world.** A failed `up` stops
//!    what it started, in reverse order, and leaves containers that were
//!    already running untouched.
//! 3. **Teardown follows the graph backwards.** Dependents stop before the
//!    things they depend on, so nothing is talking to a worker that just went
//!    away.

use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use futures::StreamExt;
use serde::Serialize;

use crate::{
    config::{ComposeFile, Container},
    configuration::{ConfigFile, merge},
    dag,
    engine::EngineClient,
    error::{ComposeError, Result},
    hooks,
    manifest::{StartSpec, resolve_start},
    process::{Outcome, Supervised, spawn_supervised_piped},
    report,
    spawn::{SpawnCtx, resolve_working_dir, spawn_plan},
    state::{ChildRecord, ChildStatus},
};

/// Outcome of one `up` or `down`. The shape is the JSON that `compose::*`
/// returns, so it is a contract: fields are added, never repurposed.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct OpResult {
    pub operation_id: String,
    pub status: OpStatus,
    /// False when the operation was a no-op — every requested container was
    /// already in the desired state.
    pub changed: bool,
    pub containers: Vec<ContainerResult>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OpStatus {
    Ok,
    Failed,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ContainerResult {
    pub container: String,
    pub state: ChildStatus,
    pub changed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<OpError>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct OpError {
    pub code: String,
    pub message: String,
}

impl From<&ComposeError> for OpError {
    fn from(error: &ComposeError) -> Self {
        Self {
            code: error.code().to_string(),
            message: error.to_string(),
        }
    }
}

/// Everything `up`/`down` need that is not the compose file itself.
pub struct LifecycleCtx<'a> {
    pub file: &'a ComposeFile,
    pub engine: &'a EngineClient,
    /// Namespace the *children* register in — not the daemon's own.
    pub project_namespace: &'a str,
    pub engine_url: &'a str,
    /// Directory for resolved config files, owner-only.
    pub config_dir: &'a std::path::Path,
    /// Where installed packages live, shared across projects on this machine.
    pub package_cache: &'a std::path::Path,
    /// Where a child's own output is written. Compose neither prints nor
    /// serves it — this is the record for a worker that dies before it can
    /// tell the engine anything.
    pub log_dir: &'a std::path::Path,
    /// Root of the per-container VM state for bundle containers.
    pub vm_dir: &'a std::path::Path,
}

/// What one container's start produced: which one, how long it took, and
/// whether it came up.
type StartOutcome = (String, Duration, Result<(ChildRecord, Supervised)>);

/// How many containers may start at once inside one wave.
///
/// Not unbounded: a wave can be the whole file, and each start may download an
/// artefact, spawn a process, or boot a VM.
const STARTS_AT_ONCE: usize = 8;

/// Containers currently supervised by this daemon, keyed by container id.
pub type Children = BTreeMap<String, Supervised>;

/// Starts `target` (or the whole graph) in dependency order.
///
/// Containers already running are left alone and reported `changed: false`, so
/// a repeated `up` is a no-op.
pub async fn up(
    ctx: &LifecycleCtx<'_>,
    children: &mut Children,
    records: &mut BTreeMap<String, ChildRecord>,
    target: Option<&str>,
    operation_id: String,
) -> OpResult {
    let began = Instant::now();
    let order = match plan_targets(ctx.file, target) {
        Ok(order) => order,
        Err(error) => {
            report::summary_failed("up", error.code(), began.elapsed());
            return failed_op(operation_id, target, &error);
        }
    };

    let mut results: Vec<ContainerResult> = Vec::new();
    // Only what *this* operation started may be rolled back.
    let mut started: Vec<String> = Vec::new();

    // Everything this operation will touch, drawn before any of it moves, so an
    // operator sees the shape rather than a line at a time.
    report::plan(&dag::outline(ctx.file, &order));

    // Grouped rather than listed: only a declared dependency has to wait, and
    // in a project of fourteen where one worker calls the other thirteen, the
    // thirteen have nothing to wait for.
    for wave in dag::waves(ctx.file, &order) {
        let mut starting: Vec<String> = Vec::new();
        for key in &wave {
            if is_running(children, key) {
                report::unchanged(key, "already running");
                results.push(ContainerResult {
                    container: key.clone(),
                    state: ChildStatus::Ready,
                    changed: false,
                    error: None,
                });
                continue;
            }
            report::starting(key, "starting");
            starting.push(key.clone());
        }
        if starting.is_empty() {
            continue;
        }

        // Bounded: a wave may hold every container in the file, and each one
        // can be a download, a process, or a VM. An operator's machine should
        // not have to survive all of them at once.
        let outcomes: Vec<StartOutcome> =
            futures::stream::iter(starting.into_iter().map(|key| async move {
                let began = Instant::now();
                let outcome = start_one(ctx, &key).await;
                (key, began.elapsed(), outcome)
            }))
            .buffer_unordered(STARTS_AT_ONCE)
            .collect()
            .await;

        // The whole wave is awaited before a failure is acted on. Stopping
        // early would leave containers half-started, which is a worse state to
        // describe than one more container that came up and is then rolled
        // back.
        let mut failure: Option<(String, ComposeError)> = None;
        for (key, took, outcome) in outcomes {
            let key = &key;
            match outcome {
                Ok((record, child)) => {
                    report::ready(key, took);
                    records.insert(key.clone(), record);
                    children.insert(key.clone(), child);
                    started.push(key.clone());
                    results.push(ContainerResult {
                        container: key.clone(),
                        state: ChildStatus::Ready,
                        changed: true,
                        error: None,
                    });
                }
                Err(error) => {
                    report::failed(
                        key,
                        error.code(),
                        &strip_container_prefix(&error.to_string(), key),
                    );
                    results.push(ContainerResult {
                        container: key.clone(),
                        state: ChildStatus::Failed,
                        changed: false,
                        error: Some(OpError::from(&error)),
                    });
                    if failure.is_none() {
                        failure = Some((key.clone(), error));
                    }
                }
            }
        }

        if let Some((_, error)) = failure {
            report::plan_done();
            rollback(ctx, children, records, &started, &mut results).await;
            report::summary_failed("up", error.code(), began.elapsed());
            return OpResult {
                operation_id,
                status: OpStatus::Failed,
                changed: !started.is_empty(),
                containers: results,
            };
        }
    }

    // Declaration order, whatever order they finished in: the JSON is a
    // contract, and a caller diffing two runs should not see the machine's
    // scheduling.
    results.sort_by_key(|result| {
        ctx.file
            .containers
            .get_index_of(&result.container)
            .unwrap_or(usize::MAX)
    });

    report::plan_done();
    let changed = results.iter().filter(|result| result.changed).count();
    report::summary_ok("up", changed, results.len(), began.elapsed());
    OpResult {
        operation_id,
        status: OpStatus::Ok,
        changed: changed > 0,
        containers: results,
    }
}

/// Drops a leading `container '<name>': ` from a message that is already being
/// printed under that container's name.
fn strip_container_prefix(message: &str, container: &str) -> String {
    let prefix = format!("container '{container}': ");
    message.strip_prefix(&prefix).unwrap_or(message).to_string()
}

/// Stops one container and starts it again, touching nothing else.
///
/// Deliberately narrower than `down` and `up` with a target, which take the
/// container's dependents and dependencies with them. Restarting a worker to
/// pick up an edit or a new version is a local act: the operator names one
/// container and expects one container to bounce, not a graph.
///
/// What that costs is real and left to the operator: a dependent holding a
/// connection to this worker sees it drop. Compose does not restart the
/// dependents to hide that, because deciding which of them can tolerate it is
/// not compose's to make.
pub async fn restart_one(
    ctx: &LifecycleCtx<'_>,
    children: &mut Children,
    records: &mut BTreeMap<String, ChildRecord>,
    key: &str,
    operation_id: String,
) -> OpResult {
    let began = Instant::now();
    if !ctx.file.containers.contains_key(key) {
        let error = ComposeError::UnknownContainer {
            container: key.to_string(),
        };
        report::summary_failed("restart", error.code(), began.elapsed());
        return failed_op(operation_id, Some(key), &error);
    }

    report::plan(&[(key.to_string(), 0)]);
    stop_one(ctx, children, records, key).await;

    // The child is gone, but the engine learns that from a socket closing and
    // not from us. Starting into that window makes the replacement collide
    // with the corpse of its predecessor and fail CONTAINER_NAME_TAKEN, which
    // is the honest answer to the wrong question. `down` then `up` never saw
    // this because re-reading the project happened to take long enough.
    if let Err(error) = await_name_release(ctx, key).await {
        report::failed(key, error.code(), &error.to_string());
        report::plan_done();
        report::summary_failed("restart", error.code(), began.elapsed());
        return failed_op(operation_id, Some(key), &error);
    }

    report::starting(key, "starting");
    let started = Instant::now();
    let outcome = start_one(ctx, key).await;
    let took = started.elapsed();

    let result = match outcome {
        Ok((record, child)) => {
            report::ready(key, took);
            records.insert(key.to_string(), record);
            children.insert(key.to_string(), child);
            ContainerResult {
                container: key.to_string(),
                state: ChildStatus::Ready,
                changed: true,
                error: None,
            }
        }
        Err(error) => {
            report::failed(
                key,
                error.code(),
                &strip_container_prefix(&error.to_string(), key),
            );
            report::plan_done();
            report::summary_failed("restart", error.code(), began.elapsed());
            return OpResult {
                operation_id,
                status: OpStatus::Failed,
                changed: false,
                containers: vec![ContainerResult {
                    container: key.to_string(),
                    state: ChildStatus::Failed,
                    changed: false,
                    error: Some(OpError::from(&error)),
                }],
            };
        }
    };

    report::plan_done();
    report::summary_ok("restart", 1, 1, began.elapsed());
    OpResult {
        operation_id,
        status: OpStatus::Ok,
        changed: true,
        containers: vec![result],
    }
}

/// How long a name may stay registered after the process holding it exits.
/// Generous, because the cost of being wrong is refusing a restart that would
/// have worked a moment later.
const NAME_RELEASE_WITHIN: Duration = Duration::from_secs(10);

/// Waits for the engine to forget a worker that has already exited.
///
/// Times out into `CONTAINER_NAME_TAKEN`, which by then is true rather than a
/// race: something else is holding the name.
async fn await_name_release(ctx: &LifecycleCtx<'_>, key: &str) -> Result<()> {
    let deadline = Instant::now() + NAME_RELEASE_WITHIN;
    loop {
        if !ctx.engine.is_registered(ctx.project_namespace, key).await? {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(ComposeError::ContainerNameTaken {
                container: key.to_string(),
                namespace: ctx.project_namespace.to_string(),
            });
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Stops `target` (or every local container), dependents first.
pub async fn down(
    ctx: &LifecycleCtx<'_>,
    children: &mut Children,
    records: &mut BTreeMap<String, ChildRecord>,
    target: Option<&str>,
    operation_id: String,
) -> OpResult {
    let began = Instant::now();
    let mut order = match plan_targets(ctx.file, target) {
        Ok(order) => order,
        Err(error) => {
            report::summary_failed("down", error.code(), began.elapsed());
            return failed_op(operation_id, target, &error);
        }
    };

    // Both branches must end up dependents-first: nothing may stop while
    // something that depends on it is still running.
    match target {
        // A single target takes its local dependents with it.
        // `transitive_dependents` returns them in stop order, so the target
        // goes last and the list needs no reversing.
        Some(target) => {
            let mut with_dependents = dag::transitive_dependents(ctx.file, target);
            with_dependents.push(target.to_string());
            order = with_dependents;
        }
        // The full plan comes back dependencies-first; teardown is its mirror.
        None => order.reverse(),
    }
    order.dedup();

    let mut results = Vec::new();
    for key in order {
        if !children.contains_key(&key) {
            report::unchanged(&key, "not running");
            results.push(ContainerResult {
                container: key,
                state: ChildStatus::Stopped,
                changed: false,
                error: None,
            });
            continue;
        }
        report::starting(&key, "stopping");
        stop_one(ctx, children, records, &key).await;
        report::stopped(&key);
        results.push(ContainerResult {
            container: key,
            state: ChildStatus::Stopped,
            changed: true,
            error: None,
        });
    }

    let changed = results.iter().filter(|result| result.changed).count();
    report::summary_ok("down", changed, results.len(), began.elapsed());
    OpResult {
        operation_id,
        status: OpStatus::Ok,
        changed: changed > 0,
        containers: results,
    }
}

/// Resolves config, runs `pre_run`, spawns, and waits for the engine to see
/// the child. Every step before the spawn can fail without leaving a process
/// behind.
/// Starts one container and hands back the child rather than filing it.
///
/// The caller owns the map, because containers with no dependency between them
/// start together and a shared `&mut` would serialise exactly what this exists
/// to overlap.
async fn start_one(ctx: &LifecycleCtx<'_>, key: &str) -> Result<(ChildRecord, Supervised)> {
    let container = ctx
        .file
        .containers
        .get(key)
        .ok_or_else(|| ComposeError::UnknownContainer {
            container: key.to_string(),
        })?;

    // Readiness is "a worker named `key` is registered in this namespace". If
    // one already is before we spawn, that check would pass on the *stranger*:
    // the container would be reported ready in milliseconds while the process
    // we started loses the `(namespace, worker_name)` lease and dies rejected.
    // Refuse instead, and say who is holding the name.
    if ctx.engine.is_registered(ctx.project_namespace, key).await? {
        return Err(ComposeError::ContainerNameTaken {
            container: key.to_string(),
            namespace: ctx.project_namespace.to_string(),
        });
    }

    // Taken here, at the last moment nothing of ours is running yet. Readiness
    // reports differences against it, and a package install below can take
    // seconds — long enough for an unrelated worker to arrive and be mistaken
    // for the child we are about to start.
    let baseline = ctx
        .engine
        .readiness_baseline(ctx.project_namespace, key)
        .await?;

    // A package is fetched here rather than at validation time: resolving it
    // needs the registry, and `validate` is offline by contract.
    let (start, shipped_config) = match &container.worker {
        crate::config::WorkerSource::Package { reference } => {
            let range = container.version.as_deref().unwrap_or("*");
            report::starting(key, &format!("installing {reference}@{range}"));
            let installed =
                crate::registry::install(key, reference, range, ctx.package_cache).await?;
            report::starting(
                key,
                &format!("starting {} {}", installed.name, installed.version),
            );
            match installed.payload {
                crate::registry::Payload::Binary(program) => (
                    StartSpec::Exec {
                        program,
                        args: Vec::new(),
                    },
                    installed.default_config,
                ),
                // The start command is the bundle's own, read from its manifest
                // inside the VM. Nothing on the host runs it.
                crate::registry::Payload::Bundle(install_dir) => {
                    (StartSpec::Vm { install_dir }, installed.default_config)
                }
            }
        }
        crate::config::WorkerSource::Path { .. } => (resolve_start(key, container)?, None),
    };

    let user_env = container.resolve_user_env(key)?;
    let config = resolve_config(ctx, container, key, shipped_config).await?;
    let worker_dir = container.worker_dir();
    let working_dir = resolve_working_dir(
        container.working_dir.as_deref(),
        worker_dir,
        &ctx.file.base_dir,
    );

    let spawn_ctx = SpawnCtx {
        engine_url: ctx.engine_url,
        namespace: ctx.project_namespace,
        container_key: key,
        start: &start,
        config_path: config.as_ref().map(|resolved| resolved.file.path()),
        config_name: config
            .as_ref()
            .and_then(|resolved| resolved.name.as_deref()),
        working_dir: &working_dir,
        user_env: &user_env,
    };

    if let Some(script) = &container.scripts.pre_run {
        hooks::await_pre_run(&spawn_ctx, script, container.scripts.pre_run_timeout)
            .await
            .map_err(|err| ComposeError::HookFailed {
                container: key.to_string(),
                hook_code: err.code(),
                message: err.to_string(),
            })?;
    }

    let plan = spawn_plan(&spawn_ctx);
    let command = match plan.command() {
        Some(command) => command,
        // A bundle: publisher-controlled code, so it is booted in a VM instead
        // of run here. The handle that comes back is an ordinary child, so
        // readiness, stop, crash cascade and log capture below are unchanged.
        None => vm_command(ctx, key, &start, &plan, config.as_ref())
            .await
            .map_err(|message| ComposeError::SpawnFailed {
                container: key.to_string(),
                message,
            })?,
    };

    let (child, output) =
        spawn_supervised_piped(command).map_err(|err| ComposeError::SpawnFailed {
            container: key.to_string(),
            message: err.to_string(),
        })?;
    // Capture before waiting on readiness: whatever the child prints while
    // starting is exactly what an operator needs when it does not.
    let capture = report::capture_output(key, output.stdout, output.stderr, ctx.log_dir);

    let readiness = ctx
        .engine
        .wait_until_ready(
            ctx.project_namespace,
            key,
            &child,
            container.startup_timeout,
            &baseline,
            ctx.log_dir,
        )
        .await;

    if let Err(error) = readiness {
        // The child is ours whether or not it registered; it must not outlive
        // the failed attempt.
        child.stop(ctx.file.stop_timeout).await;
        fire_post_run(&spawn_ctx, container);
        return Err(error);
    }

    // Registered: the engine can hear it now, so its own logging is the record
    // and compose stops keeping a second one. What stays on disk is the boot,
    // which is the part the engine never saw.
    capture.stop();

    let record = ChildRecord::from_supervised(&child, ChildStatus::Ready);
    Ok((record, child))
}

/// Builds the boot command for a bundle container, by asking `iii-worker` for
/// it.
///
/// A process boundary rather than a call: libkrun needs glibc, and the engine
/// ships a musl build so it installs on any Linux. Linked, the two do not fit
/// in one binary — the musl target does not compile. Split, the engine stays
/// portable and the VM machinery stays where its platform rules already live,
/// which is the same split the installer makes by shipping `iii-worker` as its
/// own asset.
///
/// The environment sent is the one a host container would get, with one
/// substitution: `III_CONFIG` names a host path, and the guest cannot open it.
/// The container's config directory is published into the VM and the variable
/// is repointed at the file inside it, so a worker reads its configuration the
/// same way whichever side of the boundary it runs on.
async fn vm_command(
    ctx: &LifecycleCtx<'_>,
    key: &str,
    start: &StartSpec,
    plan: &crate::spawn::SpawnPlan,
    config: Option<&ResolvedConfig>,
) -> std::result::Result<tokio::process::Command, String> {
    let StartSpec::Vm { install_dir } = start else {
        return Err("not a VM container".to_string());
    };

    let mut env: BTreeMap<String, String> = plan.env.clone();
    let config_dir = match config {
        Some(config) => {
            // Published through a directory of this container's own, not the
            // project's `config/`. virtiofs shares a whole tree, so mounting
            // the shared one would put every sibling's resolved secrets inside
            // this guest. Beside the rootfs rather than inside it: the rootfs
            // is the guest's `/`, and a `config` directory there would collide
            // with whatever the image already has.
            let path = config.file.path();
            let Some(name) = path.file_name() else {
                return Err(format!("config file has no name: {}", path.display()));
            };
            let dir = ctx.vm_dir.join(format!("{key}-config"));
            std::fs::create_dir_all(&dir)
                .map_err(|err| format!("cannot make {}: {err}", dir.display()))?;
            let published = dir.join(name);
            std::fs::copy(path, &published)
                .map_err(|err| format!("cannot publish the config for the VM: {err}"))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ =
                    std::fs::set_permissions(&published, std::fs::Permissions::from_mode(0o600));
            }
            env.insert(
                "III_CONFIG".to_string(),
                format!("{GUEST_CONFIG_DIR}/{}", name.to_string_lossy()),
            );
            Some(dir)
        }
        None => None,
    };

    let request = serde_json::json!({
        "worker_name": key,
        "install_dir": install_dir,
        "state_dir": ctx.vm_dir.join(key),
        "engine_url": ctx.engine_url,
        "extra_env": env,
        "config_dir": config_dir,
    });

    let plan = prepare_vm(&request).await?;
    let mut command = tokio::process::Command::new(&plan.program);
    command.args(&plan.args);
    for (name, value) in &plan.env {
        command.env(name, value);
    }
    // Not inherited: a lifeline from whoever started compose would tie the VM
    // to the wrong process.
    for name in &plan.env_remove {
        command.env_remove(name);
    }
    command.stdin(std::process::Stdio::null());
    Ok(command)
}

/// Where a container's config directory appears inside the guest. The same
/// constant `iii-worker` mounts it at; it is part of the request contract.
const GUEST_CONFIG_DIR: &str = "/run/iii/config";

/// What `iii-worker __bundle-prepare` answers: a program, its arguments, and
/// the environment to start it with.
#[derive(serde::Deserialize)]
struct VmPlan {
    program: std::path::PathBuf,
    args: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    #[serde(default)]
    env_remove: Vec<String>,
}

/// Runs `iii-worker __bundle-prepare` and reads the plan back.
async fn prepare_vm(request: &serde_json::Value) -> std::result::Result<VmPlan, String> {
    let program = worker_binary()?;
    let mut child = tokio::process::Command::new(&program)
        .arg("__bundle-prepare")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|err| format!("cannot run {}: {err}", program.display()))?;

    let body = request.to_string();
    if let Some(mut stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        stdin
            .write_all(body.as_bytes())
            .await
            .map_err(|err| format!("cannot send the request to iii-worker: {err}"))?;
        // Dropped here: __bundle-prepare reads to end of file, so it would wait
        // forever on a pipe compose still holds open.
        drop(stdin);
    }

    let output = child
        .wait_with_output()
        .await
        .map_err(|err| format!("iii-worker did not answer: {err}"))?;

    if !output.status.success() {
        let reason = String::from_utf8_lossy(&output.stderr);
        let reason = reason.trim();
        // The one failure whose message says nothing useful. Two binaries with
        // one contract between them can be installed apart, and clap answers a
        // subcommand it does not know by printing usage — which reads as a bug
        // in compose rather than as the version skew it is.
        if reason.contains("unrecognized subcommand") {
            return Err(format!(
                "{} is too old: it does not know `__bundle-prepare`, which compose uses to \
                 build a bundle's VM. Install it from the same release as iii — they are \
                 shipped together and are updated together",
                program.display()
            ));
        }
        // Otherwise its stderr is the diagnosis — the kill switch, a tampered
        // manifest, a rootfs that could not be prepared — already written for a
        // human, so it is passed through rather than summarised.
        return Err(if reason.is_empty() {
            format!("iii-worker could not prepare the VM ({})", output.status)
        } else {
            reason.to_string()
        });
    }

    serde_json::from_slice(&output.stdout)
        .map_err(|err| format!("iii-worker answered something unreadable: {err}"))
}

/// The `iii-worker` next to this binary, else one on `PATH`.
///
/// The sibling wins: the installer puts both in the same directory and keeps
/// them at one version, while a copy earlier on `PATH` may belong to another
/// install and boot a VM built for a different engine.
fn worker_binary() -> std::result::Result<std::path::PathBuf, String> {
    let name = format!("iii-worker{}", std::env::consts::EXE_SUFFIX);
    if let Ok(current) = std::env::current_exe() {
        let sibling = current.with_file_name(&name);
        if sibling.is_file() {
            return Ok(sibling);
        }
    }
    std::env::var_os("PATH")
        .iter()
        .flat_map(std::env::split_paths)
        .map(|dir| dir.join(&name))
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| {
            format!(
                "{name} is not installed. Bundle containers run in a VM, and it is the binary \
                 that boots one; install it beside iii, or run this project without bundles"
            )
        })
}

async fn stop_one(
    ctx: &LifecycleCtx<'_>,
    children: &mut Children,
    records: &mut BTreeMap<String, ChildRecord>,
    key: &str,
) {
    let Some(child) = children.remove(key) else {
        return;
    };
    child.stop(ctx.file.stop_timeout).await;

    // post_run fires after the exit is confirmed, on every path out. Any step
    // that cannot be rebuilt (a container that vanished from the file, an
    // env_file that is gone) simply means no hook runs — teardown never fails
    // because of its own cleanup hook.
    //
    // The spec is the hook's own. A hook is a host shell command whatever the
    // container was, and asking `resolve_start` for one instead gated the hook
    // on an answer only a `path://` container has: a package container's spec
    // comes from what was installed, which teardown cannot resolve offline. So
    // post_run never fired for the containers most likely to need it.
    if let Some(container) = ctx.file.containers.get(key)
        && let Some(script) = container.scripts.post_run.clone()
        && let Ok(user_env) = container.resolve_user_env(key)
    {
        let start = StartSpec::Shell(script);
        let working_dir = resolve_working_dir(
            container.working_dir.as_deref(),
            container.worker_dir(),
            &ctx.file.base_dir,
        );
        let spawn_ctx = SpawnCtx {
            engine_url: ctx.engine_url,
            namespace: ctx.project_namespace,
            container_key: key,
            start: &start,
            config_path: None,
            config_name: None,
            working_dir: &working_dir,
            user_env: &user_env,
        };
        fire_post_run(&spawn_ctx, container);
    }

    if let Some(record) = records.get_mut(key) {
        record.status = ChildStatus::Stopped;
    }
}

/// Undoes one failed `up`: stops what this operation started, in reverse.
async fn rollback(
    ctx: &LifecycleCtx<'_>,
    children: &mut Children,
    records: &mut BTreeMap<String, ChildRecord>,
    started: &[String],
    results: &mut [ContainerResult],
) {
    for key in started.iter().rev() {
        stop_one(ctx, children, records, key).await;
        report::rolled_back(key);
        if let Some(result) = results.iter_mut().find(|r| &r.container == key) {
            result.state = ChildStatus::Stopped;
            result.changed = false;
        }
    }
}

/// Fetch-or-fail, then merge `config_override` on top and hand the result over
/// as an owner-only file.
/// What a container's configuration resolved to: the file it is handed, and
/// the entry that value lives in.
pub struct ResolvedConfig {
    pub file: ConfigFile,
    /// The configuration entry the value was written to, when the file named
    /// one. Absent means the value went to the file only, and no global id was
    /// claimed on the container's behalf.
    pub name: Option<String>,
}

async fn resolve_config(
    ctx: &LifecycleCtx<'_>,
    container: &Container,
    key: &str,
    shipped: Option<serde_yaml::Value>,
) -> Result<Option<ResolvedConfig>> {
    // Lowest to highest: what the worker ships, what the configuration worker
    // holds, what the compose file overrides.
    let mut value = shipped;

    // Only an entry the file named. Falling back to the container key looks
    // helpful and is the collision itself: a container called `state` would
    // claim the global `state` entry, so two projects would take turns
    // overwriting one another — and every `state` worker on the engine would
    // reload on each write, because the id it watches is the one being
    // written. A configuration entry is claimed deliberately or not at all.
    //
    // Absent is not empty: an entry nobody has registered yet contributes
    // nothing, and the container starts on what the compose file declares.
    if let Some(name) = &container.config_name
        && let Some(fetched) = ctx.engine.fetch_config(name).await?
    {
        value = Some(match value {
            Some(base) => merge(base, fetched),
            None => fetched,
        });
    }

    if let Some(overrides) = &container.config_override {
        value = Some(match value {
            Some(base) => merge(base, overrides.clone()),
            None => overrides.clone(),
        });
    }

    let Some(value) = value else {
        return Ok(None);
    };

    // Delivered twice, on purpose, because workers read their configuration in
    // two different places and both have to be right.
    //
    // Into the configuration worker, which is where a worker built before
    // compose existed looks: re-registering its own schema without a value
    // reuses what is stored, so this is the value it boots on, with nothing in
    // the fleet changed.
    if let Some(name) = &container.config_name {
        ctx.engine.publish_config(name, &value).await?;
    }

    // And as a file, which is what a worker written for compose reads. The two
    // carry the same value, so whichever a worker trusts, it gets the same
    // answer.
    let file = ConfigFile::write(ctx.config_dir, key, &value)?;
    Ok(Some(ResolvedConfig {
        file,
        name: container.config_name.clone(),
    }))
}

fn fire_post_run(spawn_ctx: &SpawnCtx<'_>, container: &Container) {
    if let Some(script) = &container.scripts.post_run {
        hooks::fire_post_run(spawn_ctx, script);
    }
}

fn is_running(children: &Children, key: &str) -> bool {
    children
        .get(key)
        .is_some_and(|child| matches!(child.poll(), Outcome::Running))
}

/// Containers to act on, in dependency order: the whole graph, or the target
/// plus everything it depends on.
fn plan_targets(file: &ComposeFile, target: Option<&str>) -> Result<Vec<String>> {
    let order = file.start_order()?;
    let Some(target) = target else {
        return Ok(order);
    };
    if !file.containers.contains_key(target) {
        return Err(ComposeError::UnknownContainer {
            container: target.to_string(),
        });
    }

    let closure = dag::dependency_closure(file, target);
    Ok(order
        .into_iter()
        .filter(|key| closure.contains(key))
        .collect())
}

fn failed_op(operation_id: String, target: Option<&str>, error: &ComposeError) -> OpResult {
    OpResult {
        operation_id,
        status: OpStatus::Failed,
        changed: false,
        containers: vec![ContainerResult {
            container: target.unwrap_or("*").to_string(),
            state: ChildStatus::Failed,
            changed: false,
            error: Some(OpError::from(error)),
        }],
    }
}

/// Grace used when the daemon itself is going down.
pub const SHUTDOWN_GRACE: Duration = Duration::from_secs(10);

#[cfg(test)]
mod tests {
    use super::*;

    const PROJECT: &str = r#"
namespace: orders
containers:
  web:
    worker: path://./workers/web
    depends_on: [api]
  api:
    worker: path://./workers/api
    depends_on: [database]
  database:
    worker: path://./workers/database
  lonely:
    worker: path://./workers/lonely
"#;

    fn file() -> ComposeFile {
        ComposeFile::parse(PROJECT, "/srv/app/worker-compose.yaml").expect("fixture should parse")
    }

    #[test]
    fn a_bare_up_plans_the_whole_graph_in_dependency_order() {
        let plan = plan_targets(&file(), None).unwrap();
        assert_eq!(plan, vec!["database", "lonely", "api", "web"]);
    }

    #[test]
    fn a_targeted_up_plans_the_target_and_what_it_needs() {
        // `api` needs `database`, but nothing else in the project.
        let plan = plan_targets(&file(), Some("api")).unwrap();
        assert_eq!(plan, vec!["database", "api"]);
    }

    #[test]
    fn a_leaf_target_plans_only_itself() {
        assert_eq!(
            plan_targets(&file(), Some("lonely")).unwrap(),
            vec!["lonely"]
        );
    }

    #[test]
    fn an_unknown_target_is_rejected_before_anything_starts() {
        let err = plan_targets(&file(), Some("ghost")).unwrap_err();
        assert_eq!(err.code(), "UNKNOWN_CONTAINER");
    }

    #[test]
    fn a_failed_plan_reports_the_code_on_the_operation() {
        let error = ComposeError::UnknownContainer {
            container: "ghost".to_string(),
        };
        let result = failed_op("op-1".to_string(), Some("ghost"), &error);

        assert_eq!(result.status, OpStatus::Failed);
        assert!(!result.changed, "a rejected plan changed nothing");
        assert_eq!(
            result.containers[0].error.as_ref().unwrap().code,
            "UNKNOWN_CONTAINER"
        );
    }

    /// `OpResult` is the JSON `compose::up` returns; the field names are a
    /// contract, so this pins them.
    #[test]
    fn op_result_serializes_with_the_documented_shape() {
        let result = OpResult {
            operation_id: "op-1".to_string(),
            status: OpStatus::Ok,
            changed: true,
            containers: vec![ContainerResult {
                container: "api".to_string(),
                state: ChildStatus::Ready,
                changed: true,
                error: None,
            }],
        };

        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["operation_id"], "op-1");
        assert_eq!(json["status"], "ok");
        assert_eq!(json["changed"], true);
        assert_eq!(json["containers"][0]["container"], "api");
        assert_eq!(json["containers"][0]["state"], "ready");
        assert!(
            json["containers"][0].get("error").is_none(),
            "a successful container carries no error key"
        );
    }
}

#[cfg(test)]
mod teardown_order_tests {
    use super::*;

    /// Regression: a targeted `down` was reversing an already dependents-first
    /// list, stopping the target before the things that depend on it — the exact
    /// state `down` exists to prevent. Found by the smoke-test project.
    #[test]
    fn a_targeted_down_stops_dependents_before_their_dependency() {
        let file = ComposeFile::parse(
            r#"
namespace: orders
containers:
  web:
    worker: path://./workers/web
    depends_on: [api]
  api:
    worker: path://./workers/api
    depends_on: [database]
  database:
    worker: path://./workers/database
"#,
            "/srv/app/worker-compose.yaml",
        )
        .unwrap();

        let mut order = dag::transitive_dependents(&file, "api");
        order.push("api".to_string());
        assert_eq!(
            order,
            vec!["web", "api"],
            "web depends on api, so web stops first"
        );

        // And the whole-project teardown is the mirror of the start order.
        let mut full = file.start_order().unwrap();
        full.reverse();
        assert_eq!(full, vec!["web", "api", "database"]);
    }

    /// The case the test above cannot reach. With a depth-1 target, discovery
    /// order and reverse-topological order agree, so a breadth-first walk
    /// passes it while still being the wrong rule. Two levels below the target
    /// is where they diverge: discovery returns `[api, reports, web]` and
    /// stops `api` while `web` is still calling it.
    #[test]
    fn a_two_level_cascade_stops_the_far_dependent_first() {
        let file = ComposeFile::parse(
            r#"
namespace: orders
containers:
  web:
    worker: path://./workers/web
    depends_on: [api]
  api:
    worker: path://./workers/api
    depends_on: [database]
  reports:
    worker: path://./workers/reports
    depends_on: [database]
  database:
    worker: path://./workers/database
"#,
            "/srv/app/worker-compose.yaml",
        )
        .unwrap();

        let mut order = dag::transitive_dependents(&file, "database");
        order.push("database".to_string());

        let at = |key: &str| {
            order
                .iter()
                .position(|candidate| candidate == key)
                .unwrap_or_else(|| panic!("{key} missing from {order:?}"))
        };
        assert!(
            at("web") < at("api"),
            "web depends on api, so web stops first: {order:?}"
        );
        assert!(
            at("api") < at("database") && at("reports") < at("database"),
            "the target stops last: {order:?}"
        );
    }
}

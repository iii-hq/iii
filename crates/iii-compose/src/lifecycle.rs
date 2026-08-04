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
}

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

    for key in &order {
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
        let container_began = Instant::now();

        match start_one(ctx, children, key).await {
            Ok(record) => {
                report::ready(key, container_began.elapsed());
                records.insert(key.clone(), record);
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
    }

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
        // `transitive_dependents` already returns them nearest-first, so the
        // target goes last and the list needs no reversing.
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

/// Resolves config, runs `pre_start`, spawns, and waits for the engine to see
/// the child. Every step before the spawn can fail without leaving a process
/// behind.
async fn start_one(
    ctx: &LifecycleCtx<'_>,
    children: &mut Children,
    key: &str,
) -> Result<ChildRecord> {
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
            (
                StartSpec::Exec {
                    program: installed.program,
                    args: Vec::new(),
                },
                installed.default_config,
            )
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
        config_path: config.as_ref().map(|file| file.path()),
        working_dir: &working_dir,
        user_env: &user_env,
    };

    if let Some(script) = &container.scripts.pre_start {
        hooks::run_pre_start(&spawn_ctx, script, container.scripts.pre_start_timeout)
            .await
            .map_err(|err| ComposeError::HookFailed {
                container: key.to_string(),
                hook_code: err.code(),
                message: err.to_string(),
            })?;
    }

    let (child, output) =
        spawn_supervised_piped(spawn_plan(&spawn_ctx).command()).map_err(|err| {
            ComposeError::SpawnFailed {
                container: key.to_string(),
                message: err.to_string(),
            }
        })?;
    // Tag the child's output before waiting on readiness: whatever it prints
    // while starting is exactly what an operator needs when it does not.
    report::capture_output(key, output.stdout, output.stderr, ctx.log_dir);

    let readiness = ctx
        .engine
        .wait_until_ready(
            ctx.project_namespace,
            key,
            &child,
            container.startup_timeout,
            &baseline,
        )
        .await;

    if let Err(error) = readiness {
        // The child is ours whether or not it registered; it must not outlive
        // the failed attempt.
        child.stop(ctx.file.stop_timeout).await;
        fire_post_run(&spawn_ctx, container);
        return Err(error);
    }

    let record = ChildRecord::from_supervised(&child, ChildStatus::Ready);
    children.insert(key.to_string(), child);
    Ok(record)
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
    if let Some(container) = ctx.file.containers.get(key)
        && let Ok(user_env) = container.resolve_user_env(key)
        && let Ok(start) = resolve_start(key, container)
    {
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
async fn resolve_config(
    ctx: &LifecycleCtx<'_>,
    container: &Container,
    key: &str,
    shipped: Option<serde_yaml::Value>,
) -> Result<Option<ConfigFile>> {
    // Lowest to highest: what the worker ships, what the configuration worker
    // holds, what the compose file overrides.
    let mut value = shipped;

    if let Some(name) = &container.config_name {
        let fetched = ctx.engine.fetch_config(name).await?;
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

    match value {
        None => Ok(None),
        Some(value) => ConfigFile::write(ctx.config_dir, key, &value).map(Some),
    }
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
name: orders
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
name: orders
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
}

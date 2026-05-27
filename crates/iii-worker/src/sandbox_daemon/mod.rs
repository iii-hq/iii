// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0.

pub mod adapters;
pub mod auto_install;
pub mod catalog;
pub mod config;
pub mod create;
pub mod errors;
pub mod events;
pub mod exec;
pub mod fs;
pub mod list;
pub mod overlay;
pub mod reaper;
pub mod registry;
pub mod stop;

pub use errors::SandboxError;
pub use registry::SandboxRegistry;

use std::sync::Arc;

use iii_observability::OtelConfig;
use iii_sdk::{InitOptions, RegisterFunction, WorkerMetadata, register_worker};

use crate::sandbox_daemon::config::SandboxConfig;
use crate::sandbox_daemon::errors::SandboxErrorWire;

pub async fn run(config: SandboxConfig, engine_url: &str) -> anyhow::Result<()> {
    tracing::info!(url = %engine_url, "connecting to III engine");
    // Identify ourselves as `iii-sandbox` so the engine surfaces this
    // worker by its config-yaml name (and not the auto-detected
    // `<hostname>:<pid>`) in `engine::workers::list` and friends. The
    // publish workflow polls by this name to decide when the worker is
    // ready for interface collection.
    let iii = register_worker(
        engine_url,
        InitOptions {
            otel: Some(OtelConfig::default()),
            metadata: Some(WorkerMetadata {
                name: "iii-sandbox".to_string(),
                ..Default::default()
            }),
            ..Default::default()
        },
    );

    let sandbox_registry = Arc::new(crate::sandbox_daemon::SandboxRegistry::new());
    let sandbox_cfg = Arc::new(config);
    let launcher = Arc::new(crate::sandbox_daemon::adapters::IiiWorkerLauncher);
    let runner = Arc::new(crate::sandbox_daemon::adapters::ShellProtoRunner);
    let stopper = Arc::new(crate::sandbox_daemon::adapters::SignalStopper);

    register_sandbox_create(
        &iii,
        sandbox_registry.clone(),
        sandbox_cfg.clone(),
        launcher.clone(),
    );
    register_sandbox_exec(&iii, sandbox_registry.clone(), runner.clone());
    register_sandbox_stop(&iii, sandbox_registry.clone(), stopper.clone());
    register_sandbox_list(&iii, sandbox_registry.clone());

    {
        let fs_runner: std::sync::Arc<dyn fs::FsRunner> = std::sync::Arc::new(fs::IiiShellFsRunner);
        fs::register_all(&iii, sandbox_registry.clone(), fs_runner);
    }

    {
        let registry = (*sandbox_registry).clone();
        let stopper = stopper.clone();
        tokio::spawn(async move {
            crate::sandbox_daemon::reaper::run_reaper_loop(
                registry,
                stopper,
                std::time::Duration::from_secs(10),
            )
            .await;
        });
    }

    tracing::info!("sandbox-daemon ready");
    tokio::signal::ctrl_c().await?;
    tracing::info!("sandbox-daemon shutting down");
    iii.shutdown_async().await;
    Ok(())
}

fn register_sandbox_create(
    iii: &iii_sdk::III,
    registry: Arc<crate::sandbox_daemon::SandboxRegistry>,
    cfg: Arc<crate::sandbox_daemon::config::SandboxConfig>,
    launcher: Arc<crate::sandbox_daemon::adapters::IiiWorkerLauncher>,
) {
    let _ = iii.register_function(
        "sandbox::create",
        RegisterFunction::new_async(move |req: crate::sandbox_daemon::create::CreateRequest| {
            let registry = registry.clone();
            let cfg = cfg.clone();
            let launcher = launcher.clone();
            async move {
                crate::sandbox_daemon::create::handle_create(
                    req,
                    &cfg,
                    &registry,
                    &*launcher,
                    |e| {
                        tracing::info!(event = ?e, "sandbox create event");
                    },
                )
                .await
                .map_err(|e| SandboxErrorWire(e).into())
            }
        })
        .description("Create an ephemeral sandbox VM from a preset image"),
    );
}

fn register_sandbox_exec(
    iii: &iii_sdk::III,
    registry: Arc<crate::sandbox_daemon::SandboxRegistry>,
    runner: Arc<crate::sandbox_daemon::adapters::ShellProtoRunner>,
) {
    let _ = iii.register_function(
        "sandbox::exec",
        RegisterFunction::new_async(move |req: crate::sandbox_daemon::exec::ExecRequest| {
            let registry = registry.clone();
            let runner = runner.clone();
            async move {
                crate::sandbox_daemon::exec::handle_exec(req, &registry, &*runner)
                    .await
                    .map_err(|e| SandboxErrorWire(e).into())
            }
        })
        .description("Execute a command inside a live sandbox"),
    );
}

fn register_sandbox_stop(
    iii: &iii_sdk::III,
    registry: Arc<crate::sandbox_daemon::SandboxRegistry>,
    stopper: Arc<crate::sandbox_daemon::adapters::SignalStopper>,
) {
    let _ = iii.register_function(
        "sandbox::stop",
        RegisterFunction::new_async(move |req: crate::sandbox_daemon::stop::StopRequest| {
            let registry = registry.clone();
            let stopper = stopper.clone();
            async move {
                crate::sandbox_daemon::stop::handle_stop(req, &registry, &*stopper)
                    .await
                    .map_err(|e| SandboxErrorWire(e).into())
            }
        })
        .description("Stop and remove a running sandbox"),
    );
}

fn register_sandbox_list(
    iii: &iii_sdk::III,
    registry: Arc<crate::sandbox_daemon::SandboxRegistry>,
) {
    // Note: pre-migration this handler used
    // `serde_json::from_value(payload).unwrap_or_default()`, silently
    // coercing `null`/non-object payloads into an empty `ListRequest`.
    // `new_async` is strict, so a literal `null` payload now returns an
    // S001-style handler error instead of an empty list. The only
    // production caller (`cli::sandbox::handle_list`, which sends
    // `json!({})`) is unaffected; an empty object still deserializes to
    // the unit `ListRequest`.
    let _ = iii.register_function(
        "sandbox::list",
        RegisterFunction::new_async(move |req: crate::sandbox_daemon::list::ListRequest| {
            let registry = registry.clone();
            async move {
                // handle_list is infallible — Result wrapping is just
                // so the closure satisfies IntoAsyncHandler's
                // `Result<R, E>` shape. The Err arm is unreachable.
                Ok::<_, iii_sdk::IIIError>(
                    crate::sandbox_daemon::list::handle_list(req, &registry).await,
                )
            }
        })
        .description("List active sandboxes"),
    );
}

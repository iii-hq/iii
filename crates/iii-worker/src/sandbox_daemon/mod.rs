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

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use iii_sdk::{
    IIIError, InitOptions, OtelConfig, RegisterFunctionMessage, WorkerMetadata, register_worker,
};
use serde_json::Value;

use crate::sandbox_daemon::config::SandboxConfig;

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
    let handler = move |payload: Value| {
        let registry = registry.clone();
        let cfg = cfg.clone();
        let launcher = launcher.clone();
        Box::pin(async move {
            let req: crate::sandbox_daemon::create::CreateRequest = serde_json::from_value(payload)
                .map_err(|e| IIIError::Handler(format!("bad request: {e}")))?;
            match crate::sandbox_daemon::create::handle_create(
                req,
                &cfg,
                &registry,
                &*launcher,
                |e| {
                    tracing::info!(event = ?e, "sandbox create event");
                },
            )
            .await
            {
                Ok(resp) => serde_json::to_value(resp)
                    .map_err(|e| IIIError::Handler(format!("serialize: {e}"))),
                Err(e) => Err(IIIError::Handler(
                    serde_json::to_string(&e.to_payload()).unwrap_or_else(|_| e.to_string()),
                )),
            }
        }) as Pin<Box<dyn Future<Output = Result<Value, IIIError>> + Send>>
    };
    let _ = iii.register_function_with(
        RegisterFunctionMessage {
            id: "sandbox::create".to_string(),
            description: Some("Create an ephemeral sandbox VM from a preset image".to_string()),
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
        },
        handler,
    );
}

fn register_sandbox_exec(
    iii: &iii_sdk::III,
    registry: Arc<crate::sandbox_daemon::SandboxRegistry>,
    runner: Arc<crate::sandbox_daemon::adapters::ShellProtoRunner>,
) {
    let handler = move |payload: Value| {
        let registry = registry.clone();
        let runner = runner.clone();
        Box::pin(async move {
            let req: crate::sandbox_daemon::exec::ExecRequest = serde_json::from_value(payload)
                .map_err(|e| IIIError::Handler(format!("bad request: {e}")))?;
            match crate::sandbox_daemon::exec::handle_exec(req, &registry, &*runner).await {
                Ok(resp) => serde_json::to_value(resp)
                    .map_err(|e| IIIError::Handler(format!("serialize: {e}"))),
                Err(e) => Err(IIIError::Handler(
                    serde_json::to_string(&e.to_payload()).unwrap_or_else(|_| e.to_string()),
                )),
            }
        }) as Pin<Box<dyn Future<Output = Result<Value, IIIError>> + Send>>
    };
    let _ = iii.register_function_with(
        RegisterFunctionMessage {
            id: "sandbox::exec".to_string(),
            description: Some("Execute a command inside a live sandbox".to_string()),
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
        },
        handler,
    );
}

fn register_sandbox_stop(
    iii: &iii_sdk::III,
    registry: Arc<crate::sandbox_daemon::SandboxRegistry>,
    stopper: Arc<crate::sandbox_daemon::adapters::SignalStopper>,
) {
    let handler = move |payload: Value| {
        let registry = registry.clone();
        let stopper = stopper.clone();
        Box::pin(async move {
            let req: crate::sandbox_daemon::stop::StopRequest = serde_json::from_value(payload)
                .map_err(|e| IIIError::Handler(format!("bad request: {e}")))?;
            match crate::sandbox_daemon::stop::handle_stop(req, &registry, &*stopper).await {
                Ok(resp) => serde_json::to_value(resp)
                    .map_err(|e| IIIError::Handler(format!("serialize: {e}"))),
                Err(e) => Err(IIIError::Handler(
                    serde_json::to_string(&e.to_payload()).unwrap_or_else(|_| e.to_string()),
                )),
            }
        }) as Pin<Box<dyn Future<Output = Result<Value, IIIError>> + Send>>
    };
    let _ = iii.register_function_with(
        RegisterFunctionMessage {
            id: "sandbox::stop".to_string(),
            description: Some("Stop and remove a running sandbox".to_string()),
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
        },
        handler,
    );
}

fn register_sandbox_list(
    iii: &iii_sdk::III,
    registry: Arc<crate::sandbox_daemon::SandboxRegistry>,
) {
    let handler = move |payload: Value| {
        let registry = registry.clone();
        Box::pin(async move {
            let req: crate::sandbox_daemon::list::ListRequest =
                serde_json::from_value(payload).unwrap_or_default();
            let resp = crate::sandbox_daemon::list::handle_list(req, &registry).await;
            serde_json::to_value(resp).map_err(|e| IIIError::Handler(format!("serialize: {e}")))
        }) as Pin<Box<dyn Future<Output = Result<Value, IIIError>> + Send>>
    };
    let _ = iii.register_function_with(
        RegisterFunctionMessage {
            id: "sandbox::list".to_string(),
            description: Some("List active sandboxes".to_string()),
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
        },
        handler,
    );
}

// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! The `compose::*` control surface.
//!
//! Registered once, in `default`, where a trigger with no namespace flag lands:
//!
//! ```text
//! iii trigger compose::up id=shop file=./worker-compose.yaml
//! ```
//!
//! `id` is the whole addressing story. One daemon holds many projects and this
//! is what tells them apart — an id the operator chose, not a namespace they
//! have to look up. That leaves the namespace to be what the engine uses to
//! route workers, which is the only job it should have.
//!
//! `list` and `stop` are about the daemon itself and take no id. Every other
//! call needs one, and says so rather than guessing.

use std::sync::Arc;

use iii_sdk::{Error, RegisterFunction};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{daemon::Daemon, error::ComposeError};

/// Payload accepted by every `compose::*` function. All fields optional: the
/// bare call is the common one.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
#[serde(default)]
pub struct ComposeRequest {
    /// Which project. Chosen by the caller on the first `up` and used to reach
    /// it ever after: one daemon holds many, and this is what tells them apart.
    pub id: Option<String>,
    /// The compose file, needed once — on the call that first names a project.
    /// Giving a different one later is an error, not a rebind.
    pub file: Option<String>,
    /// Restrict the operation to one container and what it needs.
    pub container: Option<String>,
}

/// Registers the control surface. Every id is verbatim — compose never renames
/// a function.
pub fn register(daemon: &Arc<Daemon>) {
    let client = daemon.engine().client();

    for (name, kind) in [
        ("up", Operation::Up),
        ("down", Operation::Down),
        ("list", Operation::List),
        ("status", Operation::Status),
        ("stop", Operation::Stop),
        ("validate", Operation::Validate),
    ] {
        let daemon = Arc::clone(daemon);
        let function = format!("compose::{name}");
        let guard_name = name.to_string();

        client.register_function(
            function,
            RegisterFunction::new_async(move |request: ComposeRequest| {
                let daemon = Arc::clone(&daemon);
                let guard_name = guard_name.clone();
                async move { dispatch(daemon, kind, guard_name, request).await }
            }),
        );
    }
}

#[derive(Debug, Clone, Copy)]
enum Operation {
    Up,
    Down,
    List,
    Status,
    Stop,
    Validate,
}

async fn dispatch(
    daemon: Arc<Daemon>,
    operation: Operation,
    function: String,
    request: ComposeRequest,
) -> Result<Value, Error> {
    // Every project-scoped call needs one. `list` and `stop` are about the
    // daemon itself and take none.
    let id = || -> Result<String, Error> {
        request.id.clone().ok_or_else(|| Error::Remote {
            code: "MISSING_ID".to_string(),
            message: format!(
                "compose::{function} needs the project: iii trigger compose::{function} id=<id>"
            ),
            stacktrace: None,
        })
    };
    let file = request.file.as_ref().map(std::path::PathBuf::from);

    match operation {
        Operation::Up => match daemon
            .up(
                &id()?,
                file.as_deref(),
                request.container.as_deref(),
                operation_id(),
            )
            .await
        {
            Ok(result) => Ok(to_value(&result)),
            Err(err) => Err(compose_error(&err)),
        },
        Operation::Down => match daemon
            .down(&id()?, request.container.as_deref(), operation_id())
            .await
        {
            Ok(result) => Ok(to_value(&result)),
            Err(err) => Err(compose_error(&err)),
        },
        // Every project this daemon holds. No id: this is the call an operator
        // makes when they have forgotten what is running.
        Operation::List => Ok(json!({
            "daemon": daemon.worker_name,
            "daemon_pid": std::process::id(),
            "projects": daemon.list().await,
        })),
        Operation::Status => match daemon.project(&id()?, file.as_deref()).await {
            Ok(project) => Ok(json!({
                "id": project.id,
                "namespace": project.project_namespace,
                "file": project.file.path,
                // Which process is answering. `--detach` waits on this: another
                // daemon would otherwise answer for the one being launched.
                "daemon_pid": std::process::id(),
                "containers": project.status().await,
            })),
            Err(err) => Err(compose_error(&err)),
        },
        // Answers first, exits after: the serve loop picks the request up and
        // runs the same teardown a signal would.
        Operation::Stop => Ok(daemon.request_stop().await),
        // Validation is a question about a file, so it holds nothing: naming a
        // file here must not leave the daemon owning a project, and must not
        // write the durable state that would bind that id to it.
        Operation::Validate => match daemon
            .validate(request.id.as_deref(), file.as_deref())
            .await
        {
            Ok(report) => Ok(json!({
                "project": report.project,
                "namespace": report.namespace,
                "start_order": report.start_order,
                "deferred_packages": report.deferred_packages,
            })),
            Err(err) => Err(compose_error(&err)),
        },
    }
}

/// Compose errors cross the wire as their stable code plus the message, so a
/// caller can match on `UNKNOWN_PROJECT` without parsing prose. `Error::Remote`
/// is the only variant the SDK forwards verbatim into the wire `ErrorBody`.
fn compose_error(error: &ComposeError) -> Error {
    Error::Remote {
        code: error.code().to_string(),
        message: error.to_string(),
        stacktrace: None,
    }
}

fn to_value<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or_else(|err| json!({ "error": err.to_string() }))
}

fn operation_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

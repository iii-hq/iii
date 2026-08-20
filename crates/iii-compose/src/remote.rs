// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! The `compose::*` control surface.
//!
//! Registered in the daemon's own namespace, which is how one machine is
//! reached rather than another:
//!
//! ```text
//! iii trigger compose::up --namespace dev file=./worker-compose.yaml
//! ```
//!
//! The flag picks the daemon; `file=` picks the project. Nothing else names a
//! project — a name someone chose would be a second identity for the same
//! thing, and one that can be pointed at the wrong file.
//!
//! `list` and `stop` are about the daemon itself and take no file. The rest
//! fall back to `worker-compose.yaml` in the daemon's own directory, and say
//! so when there is none.

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
    /// Which daemon the caller believed they were reaching.
    ///
    /// A guard, never a route: the engine resolves a call by the `--namespace`
    /// flag and never reads the payload, so this cannot select a daemon — it
    /// can only catch having reached the wrong one. Sending it alone is the
    /// mistake it exists to name, since the call then lands wherever the flag
    /// (or its absence) pointed.
    pub namespace: Option<String>,
    /// Which project: its compose file, and the only thing that names one. A
    /// daemon started inside a project may leave it out.
    pub file: Option<String>,
    /// Restrict the operation to one container and what it needs.
    pub container: Option<String>,
    /// The worker a call is about.
    ///
    /// `compose::add` and `compose::update` read a spec: `name`,
    /// `name@version`, or a path. `compose::restart` reads a container key,
    /// where it is the spelling for `container` — an operator naming a worker
    /// should not have to know which of the two words this call wanted.
    pub worker: Option<String>,
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
        ("add", Operation::Add),
        ("restart", Operation::Restart),
        ("update", Operation::Update),
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
    Add,
    Restart,
    Update,
}

async fn dispatch(
    daemon: Arc<Daemon>,
    operation: Operation,
    function: String,
    request: ComposeRequest,
) -> Result<Value, Error> {
    // Checked before anything runs. A caller who named a daemon and reached a
    // different one meant a different machine, and acting on this one would be
    // the wrong project brought up somewhere nobody was looking.
    if let Some(addressed) = &request.namespace
        && addressed != &daemon.daemon_namespace
    {
        return Err(Error::Remote {
            code: "WRONG_DAEMON".to_string(),
            message: format!(
                "this daemon serves namespace '{}', not '{addressed}'. The namespace is a flag, \
                 not a payload field: iii trigger compose::{function} --namespace {addressed} …",
                daemon.daemon_namespace
            ),
            stacktrace: None,
        });
    }

    let file = request.file.as_ref().map(std::path::PathBuf::from);

    match operation {
        Operation::Up => match daemon
            .up(
                file.as_deref(),
                request.container.as_deref(),
                operation_id(),
            )
            .await
        {
            Ok(result) => Ok(to_value(&result)),
            Err(err) => Err(compose_error(&err)),
        },
        Operation::Add => match daemon
            .add(file.as_deref(), request.worker.as_deref(), operation_id())
            .await
        {
            Ok(result) => Ok(result),
            Err(err) => Err(compose_error(&err)),
        },
        Operation::Restart => match daemon
            .restart(
                file.as_deref(),
                // Either spelling names the same thing here.
                request.container.as_deref().or(request.worker.as_deref()),
                operation_id(),
            )
            .await
        {
            Ok(result) => Ok(result),
            Err(err) => Err(compose_error(&err)),
        },
        Operation::Update => match daemon
            .update(file.as_deref(), request.worker.as_deref(), operation_id())
            .await
        {
            Ok(result) => Ok(result),
            Err(err) => Err(compose_error(&err)),
        },
        Operation::Down => match daemon
            .down(
                file.as_deref(),
                request.container.as_deref(),
                operation_id(),
            )
            .await
        {
            Ok(result) => Ok(to_value(&result)),
            Err(err) => Err(compose_error(&err)),
        },
        // Every project this daemon holds. No id: this is the call an operator
        // makes when they have forgotten what is running.
        Operation::List => Ok(json!({
            "daemon": daemon.worker_name,
            // Which machine answered. Several daemons share an engine, so a
            // caller who reached one by namespace can confirm it was the one
            // they meant.
            "daemon_namespace": daemon.daemon_namespace,
            "daemon_pid": std::process::id(),
            "projects": daemon.list().await,
        })),
        Operation::Status => match daemon.status(file.as_deref()).await {
            Ok(project) => Ok(json!({
                "namespace": project.project_namespace,
                "file": project.file.path,
                // Derived from the file, so nobody can guess it: where this
                // project's state, delivered config and container logs live.
                "state_dir": project.state_dir(),
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
        Operation::Validate => match daemon.validate(file.as_deref()).await {
            Ok(report) => Ok(json!({
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

// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! The `compose::*` control surface.
//!
//! Registered in the daemon's own namespace, so two daemons never compete for
//! the same ids and the namespace is what addresses one:
//!
//! ```text
//! iii trigger compose::up --namespace host-a
//! ```
//!
//! `id` in the payload is an optional guard. The namespace already selected the
//! daemon; passing a mismatched id is a mistake worth reporting rather than a
//! request worth executing.

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
    /// Guard: when present it must equal the daemon's `--id`.
    pub id: Option<String>,
    /// Restrict the operation to one container and what it needs.
    pub container: Option<String>,
    /// Log lines to return, for `compose::logs`.
    pub tail: Option<usize>,
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
        ("logs", Operation::Logs),
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

/// Lines returned when the caller does not say. Enough to see a failure and
/// what led to it without paging a terminal off the screen.
const DEFAULT_TAIL: usize = 50;

#[derive(Debug, Clone, Copy)]
enum Operation {
    Up,
    Down,
    List,
    Status,
    Logs,
    Stop,
    Validate,
}

async fn dispatch(
    daemon: Arc<Daemon>,
    operation: Operation,
    function: String,
    request: ComposeRequest,
) -> Result<Value, Error> {
    if let Err(err) = daemon.check_id(request.id.as_deref(), &function) {
        return Err(compose_error(&err));
    }

    match operation {
        Operation::Up => {
            let result = daemon
                .up(request.container.as_deref(), operation_id())
                .await;
            Ok(to_value(&result))
        }
        Operation::Down => {
            let result = daemon
                .down(request.container.as_deref(), operation_id())
                .await;
            Ok(to_value(&result))
        }
        Operation::List => Ok(json!({
            "project": daemon.file.name,
            "daemon_id": daemon.id,
            "namespace": daemon.project_namespace,
            "file": daemon.file.path,
            "containers": daemon.file.containers.keys().collect::<Vec<_>>(),
        })),
        Operation::Status => Ok(json!({
            "daemon_id": daemon.id,
            // Which process is answering. `--detach` waits on this: a second
            // daemon started with a taken `--id` would otherwise see the first
            // one's answer and call itself started.
            "daemon_pid": std::process::id(),
            "namespace": daemon.project_namespace,
            "containers": daemon.status().await,
        })),
        // An unknown container answers empty rather than erroring: it may
        // simply not have started yet, and a caller polling for output should
        // not have to tell those two apart.
        Operation::Logs => Ok(json!({
            "daemon_id": daemon.id,
            "containers": daemon.logs(request.container.as_deref(), request.tail.unwrap_or(DEFAULT_TAIL)),
        })),
        // Answers first, exits after: the serve loop picks the request up and
        // runs the same teardown a signal would.
        Operation::Stop => Ok(daemon.request_stop().await),
        Operation::Validate => {
            match crate::manifest::validate_offline(&daemon.file, &daemon.project_namespace) {
                Ok(report) => Ok(json!({
                    "project": report.project,
                    "namespace": report.namespace,
                    "start_order": report.start_order,
                    "deferred_packages": report.deferred_packages,
                })),
                Err(err) => Err(compose_error(&err)),
            }
        }
    }
}

/// Compose errors cross the wire as their stable code plus the message, so a
/// caller can match on `WRONG_DAEMON` without parsing prose. `Error::Remote` is
/// the only variant the SDK forwards verbatim into the wire `ErrorBody`.
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

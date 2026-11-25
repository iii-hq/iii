// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Conversion/drop tests, not an integrated registered-handler wire test.
//! The real SDK mapper is private; its unchanged error_wire integration test
//! covers Remote preservation using a WebSocket mock. No converter is copied here.

use std::time::Duration;

use futures::FutureExt;
use tokio::sync::{Mutex, RwLock};

use super::*;

fn assert_public_remote(error: Error, code: &str, message: &str) {
    // This is Error's actual public representation, NOT a fabricated ErrorBody.
    assert!(matches!(error, Error::Remote { .. }));
    assert_eq!(
        serde_json::to_value(&error).unwrap(),
        json!({"Remote": {"code": code, "message": message, "stacktrace": null}})
    );
    let invocation = error.invocation_error().expect("structured remote error");
    assert_eq!(invocation.code, code);
    assert_eq!(invocation.message, message);
    assert_eq!(invocation.stacktrace, None);
    assert_eq!(invocation.function_id, None);
}

#[tokio::test]
async fn status_timeout_is_remote_and_drops_the_snapshot_wait() {
    assert_eq!(op_metadata("compose::status").0, 10_000);
    let inner = Mutex::new(());
    let file = RwLock::new(());
    let writer = file.write().await;
    let mut snapshot = Box::pin(async {
        let _inner = inner.lock().await;
        let _file = file.read().await;
        Ok::<_, ComposeError>(Vec::<ContainerStatus>::new())
    });
    assert!(futures::poll!(snapshot.as_mut()).is_pending());
    assert!(inner.try_lock().is_err());

    // The writer stays held, so the work cannot complete. No sleep or paused
    // clock is needed; the zero budget expires the real Tokio timeout future.
    let result = tokio::time::timeout(Duration::ZERO, snapshot)
        .await
        .map_err(read_timeout)
        .and_then(|result| result.map_err(|error| compose_error(&error)));
    assert_public_remote(result.unwrap_err(), "TIMEOUT", "invocation timed out");
    assert!(inner.try_lock().is_ok(), "timeout must drop the held guard");
    drop(writer);
    assert!(file.try_write().is_ok());
}

#[tokio::test]
async fn list_timeout_is_remote_and_discards_partial_rows_and_wait() {
    assert_eq!(op_metadata("compose::list").0, 10_000);
    let inner = Mutex::new(());
    let file = RwLock::new(());
    let writer = file.write().await;
    let mut list = Box::pin(async {
        // Model one completed project followed by an acquisition still waiting.
        // This exercises whole-future failure, not Daemon::list integration.
        let mut rows = vec![json!({"file": "first", "containers": []})];
        let _inner = inner.lock().await;
        let _file = file.read().await;
        rows.push(json!({"file": "second", "containers": []}));
        Ok::<_, ComposeError>(rows)
    });
    assert!(futures::poll!(list.as_mut()).is_pending());
    assert!(inner.try_lock().is_err());

    let result = tokio::time::timeout(Duration::ZERO, list)
        .await
        .map_err(read_timeout)
        .and_then(|result| result.map_err(|error| compose_error(&error)));
    // No successful empty/partial Vec is returned instead of the failure.
    assert_public_remote(result.unwrap_err(), "TIMEOUT", "invocation timed out");
    assert!(inner.try_lock().is_ok(), "timeout must drop the held guard");
    drop(writer);
    assert!(file.try_write().is_ok());
}

#[tokio::test]
async fn cancelled_snapshot_keeps_remote_cancellation_without_rows_or_backtrace() {
    let shutdown = crate::shutdown::ShutdownController::default();
    let inner = Mutex::new(());
    let file = RwLock::new(());
    let writer = file.write().await;
    let mut snapshot = Box::pin(async {
        shutdown
            .signal()
            .run(async {
                let _inner = inner.lock().await;
                let _file = file.read().await;
                Vec::<ContainerStatus>::new()
            })
            .await
            .ok_or_else(|| ComposeError::OperationCancelled {
                operation_id: "status".into(),
            })
    });
    assert!(futures::poll!(snapshot.as_mut()).is_pending());
    assert!(inner.try_lock().is_err());
    shutdown.cancel();

    let result = tokio::time::timeout(
        Duration::from_millis(op_metadata("compose::status").0),
        snapshot,
    )
    .now_or_never()
    .expect("latched cancellation must complete without advancing time")
    .map_err(read_timeout)
    .and_then(|result| result.map_err(|error| compose_error(&error)));
    assert_public_remote(
        result.unwrap_err(),
        "OPERATION_CANCELLED",
        "operation cancelled",
    );
    assert!(inner.try_lock().is_ok());
    drop(writer);
    assert!(file.try_write().is_ok());
}

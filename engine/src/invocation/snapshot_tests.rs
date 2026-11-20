// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use super::*;
use crate::{
    engine::{Engine, InvocationKind},
    observer::FunctionOptions,
    workers::observability::metrics::{InvocationCounts, ensure_default_meter},
};
use std::{
    future::Future,
    task::{Context, Poll},
};

async fn call(
    handler: &InvocationHandler,
    id: Uuid,
    name: &str,
    function: Arc<HandlerFn>,
) -> Result<Result<Option<Value>, ErrorBody>, RecvError> {
    handler
        .handle_invocation_with_handler(
            Some(id),
            None,
            name.to_owned(),
            "default",
            serde_json::json!({"private_payload": true}),
            function,
            None,
            None,
            None,
            None,
            InvocationCtx::engine_internal(),
        )
        .await
}

#[tokio::test]
async fn outcome_paths_keep_counts_and_first_user_semantics() {
    ensure_default_meter();
    let acc = Arc::new(MetricsAccumulator::default());
    let handler = InvocationHandler::with_metrics_for_test(acc.clone());
    let no_result: Arc<HandlerFn> =
        Arc::new(|_, _, _, _| Box::pin(async { FunctionResult::NoResult }));
    assert!(matches!(
        call(&handler, Uuid::new_v4(), "user::void", no_result).await,
        Ok(Ok(None))
    ));
    assert!(
        acc.first_user_success_fn.get().is_none(),
        "NoResult never set this marker"
    );
    let success: Arc<HandlerFn> =
        Arc::new(|_, _, _, _| Box::pin(async { FunctionResult::Success(Some(Value::Null)) }));
    for name in ["engine::probe", "user::first", "user::second"] {
        assert!(matches!(
            call(&handler, Uuid::new_v4(), name, success.clone()).await,
            Ok(Ok(_))
        ));
    }
    let failure: Arc<HandlerFn> = Arc::new(|_, _, _, _| {
        Box::pin(async {
            FunctionResult::Failure(ErrorBody {
                code: "TEST".into(),
                message: "test".into(),
                stacktrace: None,
            })
        })
    });
    for name in ["engine::error", "user::error", "user::later-error"] {
        assert!(matches!(
            call(&handler, Uuid::new_v4(), name, failure.clone()).await,
            Ok(Err(_))
        ));
    }
    assert_eq!(
        acc.invocation_counts(),
        InvocationCounts {
            total: 7,
            success: 4,
            error: 3,
            deferred: 0
        }
    );
    assert_eq!(
        acc.first_user_success_fn.get().map(String::as_str),
        Some("user::first")
    );
    assert_eq!(
        acc.first_user_failure_fn.get().map(String::as_str),
        Some("user::error")
    );
    let view = acc.snapshot_invocations();
    assert_eq!(view.by_function.len(), 7);
    assert!(view.by_function.values().all(|value| *value == 1));
}

#[tokio::test]
async fn deferred_is_committed_before_pending_receiver_without_recount() {
    ensure_default_meter();
    let acc = Arc::new(MetricsAccumulator::default());
    let handler = InvocationHandler::with_metrics_for_test(acc.clone());
    let function: Arc<HandlerFn> =
        Arc::new(|_, _, _, _| Box::pin(async { FunctionResult::Deferred }));
    let id = Uuid::new_v4();
    let mut pending = Box::pin(call(&handler, id, "user::deferred", function));
    let waker = futures::task::noop_waker();
    let mut cx = Context::from_waker(&waker);
    assert!(matches!(pending.as_mut().poll(&mut cx), Poll::Pending));
    // Synchronous reads complete while the invocation future is pending: no
    // guard crosses receiver.await. Manual polling, not sleep or scheduling luck.
    assert_eq!(
        acc.invocation_counts(),
        InvocationCounts {
            total: 1,
            deferred: 1,
            ..Default::default()
        }
    );
    assert_eq!(acc.get_function_count("user::deferred"), Some(1));
    assert!(acc.first_user_success_fn.get().is_none());
    handler
        .remove(&id)
        .unwrap()
        .sender
        .send(Completion::from(Ok(Some(Value::Null))))
        .unwrap();
    assert!(matches!(pending.await, Ok(Ok(_))));
    assert_eq!(acc.invocation_counts().total, 1);
    assert_eq!(acc.invocation_counts().deferred, 1);
    assert!(acc.first_user_success_fn.get().is_none());
}

#[tokio::test]
async fn unobservable_still_counts_and_private_refusal_captures_nothing() {
    ensure_default_meter();
    let engine = Engine::new();
    let acc = Arc::new(MetricsAccumulator::default());
    let handler = InvocationHandler::with_metrics_for_test(acc.clone());
    handler.set_observers(engine.observers.clone());
    engine.observers.set_options(
        "default",
        "hidden::fn",
        FunctionOptions {
            observable: false,
            ..Default::default()
        },
    );
    let function: Arc<HandlerFn> =
        Arc::new(|_, _, _, _| Box::pin(async { FunctionResult::NoResult }));
    let copies = engine.observers.payload_copies();
    assert!(matches!(
        call(&handler, Uuid::new_v4(), "hidden::fn", function.clone()).await,
        Ok(Ok(_))
    ));
    assert_eq!(acc.get_function_count("hidden::fn"), Some(1));
    let owner = Uuid::new_v4();
    engine.function_owners.insert(
        ("default".into(), "private::fn".into()),
        (owner, InvocationKind::Regular),
    );
    engine
        .observers
        .set_options("default", "private::fn", FunctionOptions::private());
    let refused = call(&handler, Uuid::new_v4(), "private::fn", function)
        .await
        .unwrap()
        .unwrap_err();
    assert_eq!(refused.code, "FORBIDDEN");
    assert_eq!(acc.get_function_count("private::fn"), None);
    assert_eq!(acc.invocation_counts().total, 1);
    assert_eq!(engine.observers.payload_copies(), copies);
}

// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! End-to-end tests for the `INFRASTRUCTURE_FUNCTIONS` carve-out in RBAC.
//! Wires up a real `Engine` plus an in-process `WorkerConnection` that holds
//! a session with a restricted `expose_functions`, and dispatches
//! `Message::InvokeFunction` through `Engine::router_msg` — the same path a
//! real worker would use over the WebSocket. Asserts:
//! - infrastructure IDs are allowed even when `expose_functions` does not
//!   cover them;
//! - discovery IDs stay gated;
//! - `forbidden_functions` still wins over the carve-out;
//! - the middleware bypass for `engine::*` is preserved so infrastructure
//!   calls do not recurse through user middleware.

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use tokio::sync::mpsc;
use uuid::Uuid;

use iii::{
    engine::{Engine, EngineTrait, Handler, Outbound, RegisterFunctionRequest},
    function::FunctionResult,
    protocol::{ErrorBody, Message},
    worker_connections::WorkerConnection,
    workers::{
        observability::metrics::ensure_default_meter,
        worker::{
            WorkerManagerConfig,
            rbac_config::{FunctionFilter, RbacConfig, WildcardPattern},
            rbac_session::Session,
        },
    },
};

fn register_echo_handler(engine: &Engine, function_id: &str) {
    engine.register_function_handler(
        RegisterFunctionRequest {
            function_id: function_id.to_string(),
            description: Some(format!("echo handler for {function_id}")),
            request_format: None,
            response_format: None,
            metadata: None,
        },
        Handler::new(
            |input| async move { FunctionResult::Success(Some(json!({ "echo": input }))) },
        ),
    );
}

fn session_with(
    engine: Arc<Engine>,
    rbac: RbacConfig,
    forbidden: Vec<String>,
    middleware: Option<String>,
) -> Session {
    Session {
        engine,
        config: Arc::new(WorkerManagerConfig {
            port: 0,
            host: "127.0.0.1".to_string(),
            middleware_function_id: middleware,
            rbac: Some(rbac),
            ..Default::default()
        }),
        ip_address: "127.0.0.1".to_string(),
        session_id: Uuid::new_v4(),
        allowed_functions: vec![],
        forbidden_functions: forbidden,
        allowed_trigger_types: None,
        allow_function_registration: true,
        allow_trigger_type_registration: true,
        context: json!({}),
        function_registration_prefix: None,
    }
}

fn restrictive_rbac() -> RbacConfig {
    RbacConfig {
        auth_function_id: None,
        expose_functions: vec![FunctionFilter::Match(WildcardPattern::new("api::*"))],
        on_trigger_registration_function_id: None,
        on_trigger_type_registration_function_id: None,
        on_function_registration_function_id: None,
    }
}

/// Helper: send an InvokeFunction and collect the single `InvocationResult`
/// the engine emits in response. Panics if anything else is received first.
async fn expect_result(
    engine: &Engine,
    worker: &WorkerConnection,
    rx: &mut mpsc::Receiver<Outbound>,
    function_id: &str,
) -> (String, Option<serde_json::Value>, Option<ErrorBody>) {
    let invocation_id = Uuid::new_v4();
    let msg = Message::InvokeFunction {
        invocation_id: Some(invocation_id),
        function_id: function_id.to_string(),
        data: json!({ "hello": "world" }),
        traceparent: None,
        baggage: None,
        action: None,
        metadata: None,
        namespace: None,
    };
    engine
        .router_msg(worker, &msg)
        .await
        .expect("router_msg must not error");

    loop {
        let outbound = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("timed out waiting for InvocationResult")
            .expect("worker channel closed");
        match outbound {
            Outbound::Protocol(Message::InvocationResult {
                invocation_id: got_invocation_id,
                function_id: got_id,
                result,
                error,
                ..
            }) if got_invocation_id == invocation_id => {
                return (got_id, result, error);
            }
            // Unrelated protocol or telemetry messages (or results from other
            // invocations in a shared engine) can appear — skip them and keep
            // waiting for the one we dispatched.
            _ => continue,
        }
    }
}

#[tokio::test]
async fn infrastructure_function_allowed_under_restricted_expose() {
    ensure_default_meter();
    let engine = Arc::new(Engine::new());
    register_echo_handler(&engine, "engine::log::info");

    let (tx, mut rx) = mpsc::channel::<Outbound>(16);
    let session = session_with(engine.clone(), restrictive_rbac(), vec![], None);
    let worker = WorkerConnection::with_session(tx, session);

    let (function_id, result, error) =
        expect_result(&engine, &worker, &mut rx, "engine::log::info").await;

    assert_eq!(function_id, "engine::log::info");
    assert!(
        error.is_none(),
        "expected engine::log::info to be allowed under restricted expose_functions; got error {error:?}"
    );
    assert!(
        result.is_some(),
        "expected echo handler to return a result for engine::log::info"
    );
}

#[tokio::test]
async fn discovery_function_denied_under_restricted_expose() {
    ensure_default_meter();
    let engine = Arc::new(Engine::new());
    register_echo_handler(&engine, "engine::functions::list");

    let (tx, mut rx) = mpsc::channel::<Outbound>(16);
    let session = session_with(engine.clone(), restrictive_rbac(), vec![], None);
    let worker = WorkerConnection::with_session(tx, session);

    let (function_id, result, error) =
        expect_result(&engine, &worker, &mut rx, "engine::functions::list").await;

    assert_eq!(function_id, "engine::functions::list");
    let err =
        error.expect("expected FORBIDDEN for engine::functions::list under restricted expose");
    assert_eq!(err.code, "FORBIDDEN");
    assert!(
        err.message.contains("engine::functions::list"),
        "FORBIDDEN message must name the offending function_id; got: {}",
        err.message
    );
    assert!(
        err.message.contains("rbac.expose_functions"),
        "FORBIDDEN message must include a remediation phrase pointing at rbac.expose_functions; got: {}",
        err.message
    );
    assert!(
        result.is_none(),
        "no result should be returned for a FORBIDDEN invocation"
    );
}

/// The new `::info` and `registered-triggers::*` discovery IDs added in the
/// engine_fn rework must be gated identically to the existing list IDs.
#[tokio::test]
async fn new_discovery_functions_denied_under_restricted_expose() {
    ensure_default_meter();

    let new_discovery_ids = [
        "engine::functions::info",
        "engine::workers::info",
        "engine::triggers::info",
        "engine::registered-triggers::list",
        "engine::registered-triggers::info",
    ];

    for id in new_discovery_ids {
        let engine = Arc::new(Engine::new());
        register_echo_handler(&engine, id);

        let (tx, mut rx) = mpsc::channel::<Outbound>(16);
        let session = session_with(engine.clone(), restrictive_rbac(), vec![], None);
        let worker = WorkerConnection::with_session(tx, session);

        let (function_id, _result, error) = expect_result(&engine, &worker, &mut rx, id).await;

        assert_eq!(function_id, id);
        let err =
            error.unwrap_or_else(|| panic!("expected FORBIDDEN for {id} under restricted expose"));
        assert_eq!(err.code, "FORBIDDEN", "discovery id {id} must be gated");
        assert!(
            err.message.contains(id),
            "FORBIDDEN message for {id} must name the offending function_id; got: {}",
            err.message
        );
    }
}

#[tokio::test]
async fn forbidden_list_still_wins_over_infrastructure_carve_out() {
    ensure_default_meter();
    let engine = Arc::new(Engine::new());
    register_echo_handler(&engine, "engine::log::error");

    let (tx, mut rx) = mpsc::channel::<Outbound>(16);
    let session = session_with(
        engine.clone(),
        restrictive_rbac(),
        vec!["engine::log::error".to_string()],
        None,
    );
    let worker = WorkerConnection::with_session(tx, session);

    let (_function_id, result, error) =
        expect_result(&engine, &worker, &mut rx, "engine::log::error").await;

    let err = error.expect("forbidden_functions must still deny even infrastructure IDs");
    assert_eq!(err.code, "FORBIDDEN");
    assert!(err.message.contains("engine::log::error"));
    assert!(
        err.message.contains("rbac.forbidden_functions"),
        "explicit-forbid remediation must point at rbac.forbidden_functions, not rbac.expose_functions; got: {}",
        err.message
    );
    assert!(
        !err.message.contains("rbac.expose_functions"),
        "must not mislead the operator toward expose_functions when the cause is an explicit forbid; got: {}",
        err.message
    );
    assert!(result.is_none());
}

/// The listener middleware (`session.config.middleware_function_id`) must be
/// resolved in the namespace the caller targeted, and must be told that
/// namespace so it can re-target the caller's function correctly. `mw::guard`
/// exists in both `orders` (echoes "from-orders-mw") and `default`
/// ("from-default-mw"); an invoke carrying `namespace = orders` through a
/// session with the middleware configured must hit the `orders` middleware and
/// receive `namespace = "orders"` in its input.
///
/// RED when reverted: `call_with_metadata` (default) hits "from-default-mw";
/// dropping `"namespace"` from `middleware_input` makes `received_ns` null.
/// Drives the real path via `router_msg`.
#[tokio::test]
async fn listener_middleware_runs_in_caller_namespace() {
    ensure_default_meter();
    let engine = Arc::new(Engine::new());

    for (ns, tag) in [("orders", "from-orders-mw"), ("default", "from-default-mw")] {
        engine.register_function_handler_ns(
            ns,
            RegisterFunctionRequest {
                function_id: "mw::guard".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(move |input: serde_json::Value| async move {
                FunctionResult::Success(Some(json!({
                    "mw": tag,
                    "received_ns": input.get("namespace").cloned(),
                })))
            }),
        );
    }

    let (tx, mut rx) = mpsc::channel::<Outbound>(16);
    let session = session_with(
        engine.clone(),
        restrictive_rbac(),
        vec![],
        Some("mw::guard".to_string()),
    );
    let worker = WorkerConnection::with_session(tx, session);

    let invocation_id = Uuid::new_v4();
    let msg = Message::InvokeFunction {
        invocation_id: Some(invocation_id),
        function_id: "api::thing".to_string(),
        data: json!({ "hello": "world" }),
        traceparent: None,
        baggage: None,
        action: None,
        metadata: None,
        namespace: Some("orders".to_string()),
    };
    engine
        .router_msg(&worker, &msg)
        .await
        .expect("router_msg must not error");

    let result = loop {
        let outbound = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("timed out waiting for InvocationResult")
            .expect("worker channel closed");
        if let Outbound::Protocol(Message::InvocationResult {
            invocation_id: got,
            result,
            error,
            ..
        }) = outbound
            && got == invocation_id
        {
            assert!(
                error.is_none(),
                "middleware invocation must succeed: {error:?}"
            );
            break result.expect("middleware must return a result");
        }
    };

    assert_eq!(
        result.get("mw").and_then(|v| v.as_str()),
        Some("from-orders-mw"),
        "the orders middleware must run for an orders-targeted invoke; got: {result:?}"
    );
    assert_eq!(
        result.get("received_ns").and_then(|v| v.as_str()),
        Some("orders"),
        "the middleware must receive the caller's namespace in its input; got: {result:?}"
    );
}

#[tokio::test]
async fn middleware_bypass_preserved_for_infrastructure_functions() {
    // Guards the existing prefix-based bypass at engine/src/engine/mod.rs
    // (`!function_id.starts_with("engine::")`). With the carve-out,
    // infrastructure calls become reachable on restricted sessions — if the
    // bypass ever regressed, those calls would start recursing through user
    // middleware, which is silent surprise behavior. Verify the middleware
    // handler is NOT invoked when the target is `engine::log::info`.

    ensure_default_meter();
    let engine = Arc::new(Engine::new());
    register_echo_handler(&engine, "engine::log::info");

    let middleware_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let middleware_calls_for_handler = middleware_calls.clone();
    engine.register_function_handler(
        RegisterFunctionRequest {
            function_id: "mw::guard".to_string(),
            description: Some("middleware that counts calls".to_string()),
            request_format: None,
            response_format: None,
            metadata: None,
        },
        Handler::new(move |_input| {
            let counter = middleware_calls_for_handler.clone();
            async move {
                counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                FunctionResult::Success(Some(json!({ "allow": true })))
            }
        }),
    );

    let (tx, mut rx) = mpsc::channel::<Outbound>(16);
    let session = session_with(
        engine.clone(),
        restrictive_rbac(),
        vec![],
        Some("mw::guard".to_string()),
    );
    let worker = WorkerConnection::with_session(tx, session);

    let (_function_id, _result, error) =
        expect_result(&engine, &worker, &mut rx, "engine::log::info").await;
    assert!(error.is_none(), "infrastructure call must succeed");
    assert_eq!(
        middleware_calls.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "middleware must NOT be invoked for engine::* infrastructure calls"
    );
}

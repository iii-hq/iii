//! Integration tests for HTTP middleware execution.
//!
//! Requires a running III engine. Set III_URL and III_HTTP_URL, or use localhost defaults.

mod common;

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::Mutex;
use tokio::time::sleep;

use iii_sdk::{RegisterFunctionMessage, RegisterTriggerInput};

#[tokio::test]
async fn middleware_continue_to_handler() {
    let iii = common::shared_iii();
    let mw_called = Arc::new(Mutex::new(false));
    let mw_called_clone = mw_called.clone();

    iii.register_function((
        RegisterFunctionMessage::with_id("test::mw::continue::rs".to_string()),
        move |_input: Value| {
            let flag = mw_called_clone.clone();
            async move {
                *flag.lock().await = true;
                Ok(json!({"action": "continue"}))
            }
        },
    ));

    iii.register_function((
        RegisterFunctionMessage::with_id("test::mw::continue::handler::rs".to_string()),
        |_input: Value| async move {
            Ok(json!({
                "status_code": 200,
                "body": {"message": "handler reached"},
            }))
        },
    ));

    iii.register_trigger(RegisterTriggerInput {
        trigger_type: "http".to_string(),
        function_id: "test::mw::continue::handler::rs".to_string(),
        config: json!({
            "api_path": "test/rs/mw/continue",
            "http_method": "GET",
            "middleware_function_ids": ["test::mw::continue::rs"],
        }),
    })
    .expect("register trigger");

    common::settle().await;
    sleep(Duration::from_millis(200)).await;

    let resp = common::http_client()
        .get(format!("{}/test/rs/mw/continue", common::engine_http_url()))
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status().as_u16(), 200);
    let data: Value = resp.json().await.expect("json parse");
    assert_eq!(data["message"], "handler reached");
    assert!(*mw_called.lock().await);
}

#[tokio::test]
async fn middleware_short_circuit() {
    let iii = common::shared_iii();
    let handler_called = Arc::new(Mutex::new(false));
    let handler_called_clone = handler_called.clone();

    iii.register_function((
        RegisterFunctionMessage::with_id("test::mw::block::rs".to_string()),
        |_input: Value| async move {
            Ok(json!({
                "action": "respond",
                "response": {
                    "status_code": 403,
                    "body": {"error": "Forbidden by middleware"},
                },
            }))
        },
    ));

    iii.register_function((
        RegisterFunctionMessage::with_id("test::mw::block::handler::rs".to_string()),
        move |_input: Value| {
            let flag = handler_called_clone.clone();
            async move {
                *flag.lock().await = true;
                Ok(json!({
                    "status_code": 200,
                    "body": {"message": "should not reach"},
                }))
            }
        },
    ));

    iii.register_trigger(RegisterTriggerInput {
        trigger_type: "http".to_string(),
        function_id: "test::mw::block::handler::rs".to_string(),
        config: json!({
            "api_path": "test/rs/mw/block",
            "http_method": "GET",
            "middleware_function_ids": ["test::mw::block::rs"],
        }),
    })
    .expect("register trigger");

    common::settle().await;
    sleep(Duration::from_millis(200)).await;

    let resp = common::http_client()
        .get(format!("{}/test/rs/mw/block", common::engine_http_url()))
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status().as_u16(), 403);
    let data: Value = resp.json().await.expect("json parse");
    assert_eq!(data["error"], "Forbidden by middleware");
    assert!(!*handler_called.lock().await);
}

#[tokio::test]
async fn multiple_middleware_ordering() {
    let iii = common::shared_iii();
    let call_order = Arc::new(Mutex::new(Vec::<String>::new()));
    let order1 = call_order.clone();
    let order2 = call_order.clone();
    let order3 = call_order.clone();

    iii.register_function((
        RegisterFunctionMessage::with_id("test::mw::order::first::rs".to_string()),
        move |_input: Value| {
            let order = order1.clone();
            async move {
                order.lock().await.push("mw1".to_string());
                Ok(json!({"action": "continue"}))
            }
        },
    ));

    iii.register_function((
        RegisterFunctionMessage::with_id("test::mw::order::second::rs".to_string()),
        move |_input: Value| {
            let order = order2.clone();
            async move {
                order.lock().await.push("mw2".to_string());
                Ok(json!({"action": "continue"}))
            }
        },
    ));

    iii.register_function((
        RegisterFunctionMessage::with_id("test::mw::order::handler::rs".to_string()),
        move |_input: Value| {
            let order = order3.clone();
            async move {
                order.lock().await.push("handler".to_string());
                Ok(json!({
                    "status_code": 200,
                    "body": {"message": "ok"},
                }))
            }
        },
    ));

    iii.register_trigger(RegisterTriggerInput {
        trigger_type: "http".to_string(),
        function_id: "test::mw::order::handler::rs".to_string(),
        config: json!({
            "api_path": "test/rs/mw/order",
            "http_method": "GET",
            "middleware_function_ids": [
                "test::mw::order::first::rs",
                "test::mw::order::second::rs",
            ],
        }),
    })
    .expect("register trigger");

    common::settle().await;
    sleep(Duration::from_millis(200)).await;

    let resp = common::http_client()
        .get(format!("{}/test/rs/mw/order", common::engine_http_url()))
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status().as_u16(), 200);
    let order = call_order.lock().await;
    assert_eq!(*order, vec!["mw1", "mw2", "handler"]);
}

#[tokio::test]
async fn no_middleware_regression() {
    let iii = common::shared_iii();

    iii.register_function((
        RegisterFunctionMessage::with_id("test::mw::none::rs".to_string()),
        |_input: Value| async move {
            Ok(json!({
                "status_code": 200,
                "body": {"message": "no middleware"},
            }))
        },
    ));

    iii.register_trigger(RegisterTriggerInput {
        trigger_type: "http".to_string(),
        function_id: "test::mw::none::rs".to_string(),
        config: json!({
            "api_path": "test/rs/mw/none",
            "http_method": "GET",
        }),
    })
    .expect("register trigger");

    common::settle().await;
    sleep(Duration::from_millis(200)).await;

    let resp = common::http_client()
        .get(format!("{}/test/rs/mw/none", common::engine_http_url()))
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status().as_u16(), 200);
    let data: Value = resp.json().await.expect("json parse");
    assert_eq!(data["message"], "no middleware");
}

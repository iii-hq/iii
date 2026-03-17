//! Integration tests for healthcheck endpoint registration.
//!
//! Requires a running III engine. Set III_URL and III_HTTP_URL, or use localhost defaults.

mod common;

use std::time::Duration;

use serde_json::{Value, json};

use iii_sdk::{RegisterFunctionMessage, RegisterTriggerInput};

async fn get_health_status(http_url: &str) -> u16 {
    common::http_client()
        .get(format!("{http_url}/health"))
        .send()
        .await
        .map(|r| r.status().as_u16())
        .unwrap_or(0)
}

#[tokio::test]
async fn register_healthcheck_function_and_trigger() {
    let iii = common::shared_iii();

    let fn_ref = iii.register_function(
        RegisterFunctionMessage {
            id: "test.healthcheck.rs".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
        },
        |_input: Value| async move {
            Ok(json!({
                "status_code": 200,
                "body": {
                    "status": "healthy",
                    "timestamp": "2026-01-01T00:00:00Z",
                    "service": "iii-sdk-test",
                },
            }))
        },
    );

    let status_before = get_health_status(&common::engine_http_url()).await;
    assert_eq!(
        status_before, 404,
        "expected 404 before trigger registration"
    );

    let trigger = iii
        .register_trigger(RegisterTriggerInput {
            trigger_type: "http".to_string(),
            function_id: fn_ref.id.clone(),
            config: json!({
                "api_path": "health",
                "http_method": "GET",
                "description": "Healthcheck endpoint",
            }),
        })
        .expect("register trigger");

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let resp = common::http_client()
            .get(format!("{}/health", common::engine_http_url()))
            .send()
            .await
            .expect("request failed");

        if resp.status().as_u16() == 200 {
            let data: Value = resp.json().await.expect("json parse");
            assert_eq!(data["status"], "healthy");
            assert_eq!(data["service"], "iii-sdk-test");
            assert!(data["timestamp"].is_string());
            break;
        }

        if std::time::Instant::now() >= deadline {
            panic!("healthcheck endpoint never returned 200");
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    fn_ref.unregister();
    trigger.unregister();

    common::settle().await;

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let status_after = get_health_status(&common::engine_http_url()).await;
        if status_after == 404 {
            break;
        }
        if std::time::Instant::now() >= deadline {
            panic!("expected 404 after unregister, got {status_after}");
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

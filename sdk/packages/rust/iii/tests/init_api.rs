use iii_sdk::{InitOptions, RegisterFunction, register_worker};
use serde_json::Value;

#[tokio::test]
async fn init_with_runtime_returns_sdk_instance() {
    let client = register_worker("ws://127.0.0.1:49134", InitOptions::default());
    // API should remain usable immediately after register_worker()
    client.register_function(
        "test::echo",
        RegisterFunction::new_async(|input: Value| async move { Ok(input) }),
    );
}

#[tokio::test]
async fn init_applies_otel_config_before_auto_connect() {
    use iii_observability::OtelConfig;

    let client = register_worker(
        "ws://127.0.0.1:49134",
        InitOptions {
            otel: Some(OtelConfig {
                service_name: Some("iii-rust-init-test".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        },
    );

    client.register_function(
        "test::echo::otel",
        RegisterFunction::new_async(|input: Value| async move { Ok(input) }),
    );
}

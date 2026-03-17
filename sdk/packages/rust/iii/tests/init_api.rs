use iii_sdk::{InitOptions, RegisterFunctionMessage, register_worker};

#[tokio::test]
async fn init_with_runtime_returns_sdk_instance() {
    let client = register_worker("ws://127.0.0.1:49134", InitOptions::default());
    // API should remain usable immediately after register_worker()
    client.register_function(
        RegisterFunctionMessage {
            id: "test.echo".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
        },
        |input| async move { Ok(input) },
    );
}

#[cfg(feature = "otel")]
#[tokio::test]
async fn init_applies_otel_config_before_auto_connect() {
    use iii_sdk::OtelConfig;

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
        RegisterFunctionMessage {
            id: "test.echo.otel".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
            invocation: None,
        },
        |input| async move { Ok(input) },
    );
}

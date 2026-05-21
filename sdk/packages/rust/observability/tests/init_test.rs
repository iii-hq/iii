use iii_observability::OtelConfig;
use iii_observability::{flush_otel, init_otel, shutdown_otel};

#[tokio::test]
async fn init_otel_with_disabled_config_is_noop() {
    let cfg = OtelConfig {
        enabled: Some(false),
        ..Default::default()
    };
    let initialized = init_otel(cfg).await;
    assert!(!initialized, "disabled config should return false");
    flush_otel().await;
    shutdown_otel().await;
}

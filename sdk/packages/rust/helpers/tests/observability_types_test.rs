use iii_helpers::observability::{OtelConfig, ReconnectionConfig};

#[test]
fn otel_config_default_disabled() {
    let cfg = OtelConfig::default();
    assert!(cfg.enabled.is_none()); // enabled is Option<bool>, None means not configured (disabled)
}

#[test]
fn reconnection_config_default_has_initial_delay() {
    let cfg = ReconnectionConfig::default();
    assert!(cfg.initial_delay_ms > 0);
}
